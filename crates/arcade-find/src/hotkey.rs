//! The global shortcut. X11, Windows and macOS: a native hotkey
//! (global-hotkey). Hyprland: a runtime key binding that runs
//! `arcade-find --toggle` (never written to the config; re-applied when
//! the config reloads). Other Wayland desktops: the user binds that
//! command in their desktop settings (shown in Settings).

use std::cell::RefCell;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use global_hotkey::hotkey::HotKey;
use serde_json::{json, Value};

use crate::service::{Service, UiMsg};

/// Normalizes user spellings (`Win`, `Meta`, `Cmd`, `Option`) and parses.
pub fn parse(s: &str) -> Result<HotKey, String> {
    let norm: Vec<String> = s
        .split('+')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "win" | "meta" | "super" | "logo" | "cmd" | "command" => "Super".to_string(),
            "control" | "ctrl" => "Ctrl".into(),
            "option" | "alt" => "Alt".into(),
            "esc" => "Escape".into(),
            _ => t.to_string(),
        })
        .collect();
    if norm.len() < 2 && !norm.first().is_some_and(|k| k.starts_with('F') && k.len() > 1) {
        return Err(format!("\"{s}\" needs a modifier (Ctrl, Alt, Shift or Super)"));
    }
    HotKey::from_str(&norm.join("+")).map_err(|e| format!("invalid shortcut \"{s}\": {e}"))
}

/// Shortcuts the desktop commonly owns, and what owns them.
const RESERVED: &[(&str, &str)] = &[
    ("Ctrl+Alt+Delete", "the system"),
    ("Ctrl+Alt+T", "open terminal (Ubuntu)"),
    ("Ctrl+Alt+L", "lock screen (GNOME/Ubuntu)"),
    ("Ctrl+Alt+D", "show desktop (some Linux desktops)"),
    ("Super+L", "lock screen"),
    ("Super+Space", "input source / Spotlight"),
    ("Super+E", "File Explorer (Windows)"),
    ("Super+S", "Windows search"),
    ("Ctrl+Space", "input method / Spotlight"),
    ("Alt+Tab", "window switching"),
    ("Super+Tab", "window switching"),
    ("Alt+F4", "closing windows"),
    ("Ctrl+F", "find in the focused app"),
];

pub fn known_conflict(s: &str) -> Option<&'static str> {
    let key = parse(s).ok()?;
    RESERVED.iter().find(|(r, _)| parse(r).ok() == Some(key)).map(|(_, what)| *what)
}

/// The command a desktop shortcut should run.
pub fn toggle_command() -> String {
    let exe = arcade_link::manifest::current_executable();
    format!("{} --toggle", hyprland::shell_quote(&exe))
}

/// How the shortcut is working now (shown in Settings).
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Off,
    Native(String),
    Hyprland(String),
    /// No automatic shortcut here: bind `command` in the desktop settings.
    Manual {
        reason: String,
        command: String,
    },
    Failed(String),
}

impl State {
    pub fn to_json(&self) -> Value {
        match self {
            State::Off => json!({ "mode": "off" }),
            State::Native(a) => json!({ "mode": "native", "accelerator": a }),
            State::Hyprland(a) => json!({ "mode": "hyprland", "accelerator": a }),
            State::Manual { reason, command } => json!({ "mode": "manual", "reason": reason, "command": command }),
            State::Failed(e) => json!({ "mode": "failed", "error": e, "command": toggle_command() }),
        }
    }
}

enum Active {
    Native(global_hotkey::GlobalHotKeyManager, HotKey),
    Hyprland(hyprland::Binding),
}

thread_local! {
    /// Lives on the main thread (global-hotkey requires it on macOS and Windows).
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

static HYPR_WATCH: Mutex<bool> = Mutex::new(false);

fn wayland_session() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn clear() {
    ACTIVE.with(|a| match a.borrow_mut().take() {
        Some(Active::Native(m, hk)) => {
            let _ = m.unregister(hk);
        }
        Some(Active::Hyprland(b)) => b.remove(),
        None => {}
    });
}

/// Registers the shortcut from settings (call on the main thread, at start
/// and after settings change). Records the state in the service.
pub fn apply(svc: &Arc<Service>) -> State {
    clear();
    let s = svc.settings();
    let accel = s.shortcut.trim().to_string();
    let state = if accel.is_empty() {
        State::Off
    } else if let Err(e) = parse(&accel) {
        State::Failed(e)
    } else if hyprland::active() && s.hyprland_runtime_bind {
        match hyprland::bind(&accel, &toggle_command()) {
            Ok(b) => {
                ACTIVE.with(|a| *a.borrow_mut() = Some(Active::Hyprland(b)));
                watch_hyprland_reloads(svc);
                State::Hyprland(accel.clone())
            }
            Err(e) => State::Failed(e),
        }
    } else if wayland_session() {
        State::Manual {
            reason:
                "This Wayland desktop doesn't let apps register global shortcuts. Add a custom shortcut in your desktop settings that runs:"
                    .into(),
            command: toggle_command(),
        }
    } else {
        native(svc, &accel)
    };
    svc.set_shortcut_state(state.to_json());
    state
}

fn native(svc: &Arc<Service>, accel: &str) -> State {
    let hk = match parse(accel) {
        Ok(h) => h,
        Err(e) => return State::Failed(e),
    };
    let manager = match global_hotkey::GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => return State::Failed(format!("Global shortcuts aren't available: {e}")),
    };
    if let Err(e) = manager.register(hk) {
        return State::Failed(match e {
            global_hotkey::Error::AlreadyRegistered(_) | global_hotkey::Error::FailedToRegister(_) => {
                format!("{accel} is already used by another application")
            }
            e => e.to_string(),
        });
    }
    let weak = Arc::downgrade(svc);
    let id = hk.id();
    global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(move |e: global_hotkey::GlobalHotKeyEvent| {
        if e.id == id && e.state == global_hotkey::HotKeyState::Pressed {
            if let Some(s) = weak.upgrade() {
                s.send(UiMsg::Toggle);
            }
        }
    }));
    ACTIVE.with(|a| *a.borrow_mut() = Some(Active::Native(manager, hk)));
    State::Native(accel.to_string())
}

/// Hyprland drops runtime binds when its config reloads: bind again then.
fn watch_hyprland_reloads(svc: &Arc<Service>) {
    let mut started = HYPR_WATCH.lock().unwrap_or_else(|e| e.into_inner());
    if *started {
        return;
    }
    *started = true;
    let weak = Arc::downgrade(svc);
    hyprland::on_config_reloaded(move || {
        if let Some(s) = weak.upgrade() {
            let settings = s.settings();
            if settings.hyprland_runtime_bind && !settings.shortcut.trim().is_empty() {
                let _ = hyprland::bind(settings.shortcut.trim(), &toggle_command());
            }
        }
    });
}

/// Removes the shortcut (on quit), including a Hyprland runtime bind.
pub fn release() {
    clear();
}

/// Hyprland runtime key bindings. Adapted from Arcade Lens
/// (`crates/lens-platform/src/hyprland.rs`, commit 26b3532, MIT OR
/// Apache-2.0). Hyprland ≥ 0.55 is configured in Lua (`hyprctl eval`);
/// older versions use `hyprctl keyword`. Both are runtime-only: nothing is
/// written to the user's configuration.
pub mod hyprland {
    use std::process::Command;
    use std::sync::OnceLock;

    use global_hotkey::hotkey::{Code, Modifiers};

    pub fn active() -> bool {
        cfg!(target_os = "linux") && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
    }

    fn hyprctl(args: &[&str]) -> Result<(), String> {
        let out = Command::new("hyprctl").args(args).output().map_err(|e| format!("hyprctl: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && text == "ok" {
            Ok(())
        } else {
            Err(format!("hyprctl {}: {text}", args.first().copied().unwrap_or_default()))
        }
    }

    fn lua() -> bool {
        static LUA: OnceLock<bool> = OnceLock::new();
        *LUA.get_or_init(|| hyprctl(&["eval", "return hl ~= nil"]).is_ok())
    }

    fn lua_string(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    }

    /// Single-quotes `s` for `sh`, which runs Hyprland's exec commands.
    pub fn shell_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// The X keysym name Hyprland binds by.
    fn keysym(code: Code) -> Option<String> {
        let name = format!("{code:?}");
        if let Some(k) = name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")) {
            return Some(k.to_string());
        }
        if name.len() > 1 && name.starts_with('F') && name[1..].parse::<u8>().is_ok() {
            return Some(name);
        }
        let s = match code {
            Code::Space => "space",
            Code::Enter => "Return",
            Code::Escape => "Escape",
            Code::Tab => "Tab",
            Code::Backspace => "BackSpace",
            Code::Delete => "Delete",
            Code::Insert => "Insert",
            Code::Home => "Home",
            Code::End => "End",
            Code::PageUp => "Prior",
            Code::PageDown => "Next",
            Code::ArrowLeft => "Left",
            Code::ArrowRight => "Right",
            Code::ArrowUp => "Up",
            Code::ArrowDown => "Down",
            Code::Minus => "minus",
            Code::Equal => "equal",
            Code::BracketLeft => "bracketleft",
            Code::BracketRight => "bracketright",
            Code::Backslash => "backslash",
            Code::Semicolon => "semicolon",
            Code::Quote => "apostrophe",
            Code::Backquote => "grave",
            Code::Comma => "comma",
            Code::Period => "period",
            Code::Slash => "slash",
            _ => return None,
        };
        Some(s.to_string())
    }

    fn combo(shortcut: &str) -> Result<(Vec<&'static str>, String), String> {
        let hk = super::parse(shortcut)?;
        let mut mods = Vec::new();
        for (m, name) in [(Modifiers::SUPER, "SUPER"), (Modifiers::CONTROL, "CTRL"), (Modifiers::ALT, "ALT"), (Modifiers::SHIFT, "SHIFT")] {
            if hk.mods.contains(m) {
                mods.push(name);
            }
        }
        let key = keysym(hk.key).ok_or_else(|| format!("{shortcut} can't be bound in Hyprland; choose another key"))?;
        Ok((mods, key))
    }

    /// A key binding Find added to the running compositor.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Binding {
        mods: Vec<&'static str>,
        key: String,
    }

    impl Binding {
        fn lua_keys(&self) -> String {
            let mut parts: Vec<&str> = self.mods.clone();
            parts.push(&self.key);
            lua_string(&parts.join(" + "))
        }

        fn legacy_keys(&self) -> String {
            format!("{}, {}", self.mods.join(" "), self.key)
        }

        pub fn remove(&self) {
            let _ = if lua() {
                hyprctl(&["eval", &format!("hl.unbind({})", self.lua_keys())])
            } else {
                hyprctl(&["keyword", "unbind", &self.legacy_keys()])
            };
        }
    }

    /// Binds `shortcut` to run the shell command `command`.
    pub fn bind(shortcut: &str, command: &str) -> Result<Binding, String> {
        let (mods, key) = combo(shortcut)?;
        let b = Binding { mods, key };
        // Drop an earlier copy (a restart, or a reload race) before binding.
        b.remove();
        if lua() {
            hyprctl(&["eval", &format!("hl.bind({}, hl.dsp.exec_cmd({}))", b.lua_keys(), lua_string(command))])?;
        } else {
            hyprctl(&["keyword", "bind", &format!("{}, exec, {command}", b.legacy_keys())])?;
        }
        Ok(b)
    }

    /// The manual line for `hyprland.conf` (documented, never written).
    pub fn config_line(shortcut: &str, command: &str) -> Option<String> {
        let (mods, key) = combo(shortcut).ok()?;
        Some(format!("bind = {}, {key}, exec, {command}", mods.join(" ")))
    }

    #[cfg(unix)]
    fn socket_dir() -> Option<std::path::PathBuf> {
        use std::path::PathBuf;
        let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("hypr").join(&sig));
        runtime.filter(|d| d.exists()).or_else(|| Some(PathBuf::from("/tmp/hypr").join(sig)))
    }

    /// Calls `f` on a background thread whenever Hyprland reloads its config.
    pub fn on_config_reloaded(f: impl Fn() + Send + 'static) {
        #[cfg(unix)]
        {
            use std::io::{BufRead, BufReader};
            use std::os::unix::net::UnixStream;
            let Some(dir) = socket_dir() else { return };
            let _ = std::thread::Builder::new().name("find-hyprland".into()).spawn(move || {
                let Ok(stream) = UnixStream::connect(dir.join(".socket2.sock")) else { return };
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if line.starts_with("configreloaded>>") {
                        f();
                    }
                }
            });
        }
        #[cfg(not(unix))]
        let _ = f;
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn shortcuts_translate_to_hyprland_keys() {
            let b = |s| combo(s).map(|(mods, key)| Binding { mods, key });
            let f = b("Ctrl+Alt+F").unwrap();
            assert_eq!(f.lua_keys(), "\"CTRL + ALT + F\"");
            assert_eq!(f.legacy_keys(), "CTRL ALT, F");
            assert_eq!(b("Super+Space").unwrap().key, "space");
            assert_eq!(
                config_line("Ctrl+Alt+F", "'/x/arcade-find' --toggle").as_deref(),
                Some("bind = CTRL ALT, F, exec, '/x/arcade-find' --toggle")
            );
        }

        #[test]
        fn quoting() {
            assert_eq!(shell_quote("/a b/it's"), "'/a b/it'\\''s'");
            assert_eq!(lua_string("say \"hi\" \\"), "\"say \\\"hi\\\" \\\\\"");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_conflicts() {
        assert!(parse("Ctrl+Alt+F").is_ok());
        assert_eq!(parse("ctrl+alt+f"), parse("Alt+Control+F"));
        assert!(parse("F").is_err(), "a bare letter would steal typing");
        assert!(parse("F8").is_ok());
        assert_eq!(known_conflict("Ctrl+Alt+F"), None, "the default has no known desktop owner");
        assert!(known_conflict("ctrl+alt+t").is_some());
    }

    /// The default must not clash with the Arcade family's defaults (SPEC §8.5).
    #[test]
    fn default_clashes_with_no_family_shortcut() {
        let family = [
            "Ctrl+Alt+Space",
            "Super+Shift+Space",
            "Ctrl+Alt+Shift+Space",
            "Ctrl+Alt+Shift+L",
            "Ctrl+Shift+Space",
            "Ctrl+Alt+V",
            "Super+Shift+V",
            "F8",
        ];
        let ours = arcade_link::registry::normalize_accelerator(find_core::settings::DEFAULT_SHORTCUT);
        for f in family {
            assert_ne!(arcade_link::registry::normalize_accelerator(f), ours, "{f}");
        }
    }
}
