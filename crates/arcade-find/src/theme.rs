//! Colors (from Arcade Link's integration tokens plus Find's accent) and
//! the system light/dark preference.

use std::sync::atomic::{AtomicU8, Ordering};

use find_core::settings::Theme;

/// Arcade Find's accent (proposed for Link's tokens as `arcade.find`).
pub const ACCENT: u32 = 0x22C55E;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn hex(v: u32) -> Rgba {
        Rgba { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8, a: 255 }
    }
    pub const fn alpha(self, a: u8) -> Rgba {
        Rgba { a, ..self }
    }
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub panel: Rgba,
    pub border: Rgba,
    pub separator: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub accent: Rgba,
    /// Matched characters in names.
    pub highlight: Rgba,
    pub selection: Rgba,
    pub selection_multi: Rgba,
    pub text_selection: Rgba,
    pub chip: Rgba,
    pub danger: Rgba,
}

impl Palette {
    pub fn dark(translucent: bool) -> Palette {
        Palette {
            dark: true,
            panel: Rgba::hex(0x14171C).alpha(if translucent { 226 } else { 255 }),
            border: Rgba::hex(0x2A2F38),
            separator: Rgba::hex(0x2A2F38).alpha(200),
            text: Rgba::hex(0xE7EAF0),
            muted: Rgba::hex(0x9AA3B2),
            faint: Rgba::hex(0x6B7382),
            accent: Rgba::hex(ACCENT),
            highlight: Rgba::hex(0x4ADE80),
            selection: Rgba::hex(ACCENT).alpha(46),
            selection_multi: Rgba::hex(ACCENT).alpha(26),
            text_selection: Rgba::hex(ACCENT).alpha(90),
            chip: Rgba::hex(0x1C2027),
            danger: Rgba::hex(0xF87171),
        }
    }

    pub fn light(translucent: bool) -> Palette {
        Palette {
            dark: false,
            panel: Rgba::hex(0xFFFFFF).alpha(if translucent { 232 } else { 255 }),
            border: Rgba::hex(0xDDE1E7),
            separator: Rgba::hex(0xDDE1E7),
            text: Rgba::hex(0x161A20),
            muted: Rgba::hex(0x5B6472),
            faint: Rgba::hex(0x8A93A3),
            accent: Rgba::hex(0x16A34A),
            highlight: Rgba::hex(0x15803D),
            selection: Rgba::hex(ACCENT).alpha(40),
            selection_multi: Rgba::hex(ACCENT).alpha(22),
            text_selection: Rgba::hex(ACCENT).alpha(70),
            chip: Rgba::hex(0xF5F6F8),
            danger: Rgba::hex(0xDC2626),
        }
    }

    pub fn for_theme(theme: Theme, translucent: bool) -> Palette {
        let dark = match theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => system_dark(),
        };
        if dark {
            Palette::dark(translucent)
        } else {
            Palette::light(translucent)
        }
    }
}

/// 0 unknown, 1 dark, 2 light — refreshed off the UI thread.
static SYSTEM: AtomicU8 = AtomicU8::new(0);

pub fn system_dark() -> bool {
    match SYSTEM.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => true, // until detected: dark is the overlay's default look
    }
}

/// Reads the OS preference (blocking: call from a worker thread).
pub fn refresh_system_theme() -> bool {
    let dark = detect_dark();
    let v = match dark {
        Some(true) => 1,
        Some(false) => 2,
        None => 0,
    };
    SYSTEM.swap(v, Ordering::Relaxed) != v
}

#[cfg(target_os = "linux")]
fn detect_dark() -> Option<bool> {
    // The XDG Settings portal: org.freedesktop.appearance color-scheme (1 dark, 2 light).
    if let Some(v) = portal_color_scheme() {
        if v == 1 {
            return Some(true);
        }
        if v == 2 {
            return Some(false);
        }
    }
    if let Ok(t) = std::env::var("GTK_THEME") {
        return Some(t.to_lowercase().contains("dark"));
    }
    None
}

#[cfg(target_os = "linux")]
pub fn portal_color_scheme() -> Option<u32> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "color-scheme"),
        )
        .ok()?;
    let v: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    u32_from_value(&v)
}

#[cfg(target_os = "linux")]
fn u32_from_value(v: &zbus::zvariant::Value<'_>) -> Option<u32> {
    match v {
        zbus::zvariant::Value::U32(x) => Some(*x),
        zbus::zvariant::Value::Value(inner) => u32_from_value(inner),
        _ => None,
    }
}

/// Calls `f` whenever the portal reports a color-scheme change (a blocking
/// signal stream on its own thread; no polling).
#[cfg(target_os = "linux")]
pub fn watch_system_theme(f: impl Fn() + Send + 'static) {
    let _ = std::thread::Builder::new().name("find-theme".into()).spawn(move || {
        let Ok(conn) = zbus::blocking::Connection::session() else { return };
        let Ok(proxy) = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings") else {
            return;
        };
        let Ok(signals) = proxy.receive_signal("SettingChanged") else { return };
        for msg in signals {
            let body = msg.body();
            if let Ok((ns, key, _v)) = body.deserialize::<(String, String, zbus::zvariant::OwnedValue)>() {
                if ns == "org.freedesktop.appearance" && key == "color-scheme" && refresh_system_theme() {
                    f();
                }
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn watch_system_theme(_f: impl Fn() + Send + 'static) {}

#[cfg(windows)]
fn detect_dark() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
    let name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut data: u32 = 0;
    let mut len: u32 = 4;
    // SAFETY: valid NUL-terminated strings and out buffers.
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut len) };
    (r == 0).then_some(data == 0)
}

#[cfg(target_os = "macos")]
fn detect_dark() -> Option<bool> {
    let out = std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleInterfaceStyle"]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().eq_ignore_ascii_case("dark"))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn detect_dark() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_is_reasonable() {
        fn lum(c: Rgba) -> f32 {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
        }
        fn ratio(a: Rgba, b: Rgba) -> f32 {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        }
        for p in [Palette::dark(false), Palette::light(false)] {
            let bg = p.panel.alpha(255);
            assert!(ratio(p.text, bg) >= 7.0);
            assert!(ratio(p.muted, bg) >= 4.5, "muted {:?}", p.dark);
            assert!(ratio(p.highlight, bg) >= 4.5, "highlight {:?}", p.dark);
        }
    }
}
