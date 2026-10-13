//! The tray (menu bar) icon, with the family menu (SPEC §8.3):
//! Open Arcade Find · Open Settings · Restart Arcade Find · — · Quit Arcade Find.
//! A click opens Settings (on macOS the click opens the menu). Linux uses
//! StatusNotifierItem (ksni) and shows up when a tray host appears later.

use std::sync::{Arc, Weak};

use crate::instance::Command;
use crate::service::{Service, UiMsg};
use arcade_link::trayhost::{TrayHostWatcher, TrayState, WatcherConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Open,
    Settings,
    Rescan,
    Restart,
    Quit,
}

fn run(svc: &Weak<Service>, item: Item) {
    let Some(s) = svc.upgrade() else { return };
    match item {
        Item::Open => s.send(UiMsg::Show { query: None, reveal: None }),
        Item::Settings => s.open_settings(),
        Item::Rescan => {
            let _ = s.command(Command::Rescan);
        }
        Item::Restart => {
            let reply = s.command(Command::Restart);
            if reply["ok"] != true {
                s.send(UiMsg::Toast(reply["error"].as_str().unwrap_or("Couldn't restart").into(), crate::ui::model::ToastKind::Error));
            }
        }
        Item::Quit => s.send(UiMsg::Quit),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::blocking::TrayMethods;

    struct FindTray {
        svc: Weak<Service>,
        icon: Vec<ksni::Icon>,
    }

    impl ksni::Tray for FindTray {
        fn id(&self) -> String {
            "arcade-find".into()
        }
        fn title(&self) -> String {
            "Arcade Find".into()
        }
        fn category(&self) -> ksni::Category {
            ksni::Category::ApplicationStatus
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: "Arcade Find".into(), ..Default::default() }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            run(&self.svc, Item::Settings);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, it: Item| -> ksni::MenuItem<Self> {
                StandardItem { label: label.into(), activate: Box::new(move |t: &mut Self| run(&t.svc, it)), ..Default::default() }.into()
            };
            vec![
                item("Open Arcade Find", Item::Open),
                item("Open Settings", Item::Settings),
                item("Rescan index", Item::Rescan),
                item("Restart Arcade Find", Item::Restart),
                ksni::MenuItem::Separator,
                item("Quit Arcade Find", Item::Quit),
            ]
        }
    }

    fn icon(px: u32) -> ksni::Icon {
        let rgba = crate::ui::icons::app_icon_rgba(px);
        // Straight RGBA → ARGB32, network byte order.
        let mut data = Vec::with_capacity(rgba.len());
        for p in rgba.as_chunks::<4>().0 {
            data.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
        }
        ksni::Icon { width: px as i32, height: px as i32, data }
    }

    pub fn start(svc: &Arc<Service>) -> Result<(), String> {
        // Both initial construction and D-Bus changes are off the UI thread.
        // Only Own registers an item. Hosts such as Caelestia and Waybar still
        // show Passive items, so a hosted icon is unregistered, not hidden.
        let weak = Arc::downgrade(svc);
        let mut handle: Option<ksni::blocking::Handle<FindTray>> = None;
        let watcher = watch(svc, move |state| {
            let Some(svc) = weak.upgrade() else { return };
            // Callback owns its state on the watcher's one worker thread.
            if state == TrayState::Own {
                if !matches!(&handle, Some(h) if !h.is_closed()) {
                    let tray = FindTray { svc: Arc::downgrade(&svc), icon: vec![icon(22), icon(32), icon(64)] };
                    match tray.assume_sni_available(true).spawn() {
                        Ok(h) => handle = Some(h),
                        Err(e) => {
                            eprintln!("arcade-find: tray: {e}");
                            svc.note_tray(TrayState::None);
                            return;
                        }
                    }
                }
            } else if let Some(h) = handle.take() {
                h.shutdown().wait();
            }
            svc.note_tray(state);
        })?;
        *svc.tray_watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
        Ok(())
    }

    pub fn set_state(_svc: &Arc<Service>, _state: TrayState) {}
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    struct NativeTray {
        prepared: Option<tray_icon::TrayIconBuilder>,
        icon: Option<tray_icon::TrayIcon>,
    }
    thread_local! { static TRAY: std::cell::RefCell<Option<NativeTray>> = const { std::cell::RefCell::new(None) }; }

    /// Only native tray mutation runs on the UI thread. The watcher gates the
    /// first icon without delaying the event loop, then uses live IPC events.
    pub fn start(svc: &Arc<Service>) -> Result<(), String> {
        let weak = Arc::downgrade(svc);
        let watcher = watch(svc, move |state| {
            if let Some(s) = weak.upgrade() {
                s.send(UiMsg::Tray(state));
            }
        })?;
        *svc.tray_watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
        Ok(())
    }

    fn prepare(svc: &Arc<Service>) -> Result<tray_icon::TrayIconBuilder, String> {
        let open = MenuItem::new("Open Arcade Find", true, None);
        let settings = MenuItem::new("Open Settings", true, None);
        let rescan = MenuItem::new("Rescan index", true, None);
        let restart = MenuItem::new("Restart Arcade Find", true, None);
        let quit = MenuItem::new("Quit Arcade Find", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &settings, &rescan, &restart, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let px = 32;
        let icon = Icon::from_rgba(crate::ui::icons::app_icon_rgba(px), px, px).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Arcade Find")
            .with_icon(icon)
            .with_menu_on_left_click(cfg!(target_os = "macos"));
        let ids = [
            (open.id().clone(), Item::Open),
            (settings.id().clone(), Item::Settings),
            (rescan.id().clone(), Item::Rescan),
            (restart.id().clone(), Item::Restart),
            (quit.id().clone(), Item::Quit),
        ];
        let weak = Arc::downgrade(svc);
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, it)) = ids.iter().find(|(id, _)| *id == e.id) {
                run(&weak, *it);
            }
        }));
        let weak = Arc::downgrade(svc);
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                if !cfg!(target_os = "macos") {
                    run(&weak, Item::Settings);
                }
            }
        }));
        Ok(tray)
    }

    pub fn set_state(svc: &Arc<Service>, state: TrayState) {
        let result = TRAY.with(|slot| {
            let mut tray = slot.borrow_mut();
            // Prepare the menu and bitmap on the first answer, even Hosted.
            // tray-icon has no hidden constructor: defer OS registration until
            // Own, then retain the native object for subsequent hide/show.
            if tray.is_none() && state != TrayState::None {
                *tray = Some(NativeTray { prepared: Some(prepare(svc)?), icon: None });
            }
            if let Some(tray) = tray.as_mut() {
                if tray.icon.is_none() && state == TrayState::Own {
                    tray.icon = Some(tray.prepared.take().unwrap().build().map_err(|e| e.to_string())?);
                }
                if let Some(icon) = &tray.icon {
                    icon.set_visible(state == TrayState::Own).map_err(|e| e.to_string())?;
                }
            }
            Ok::<_, String>(())
        });
        match result {
            Ok(()) => svc.note_tray(state),
            Err(e) => {
                // A failed native registration consumed its builder. Rebuild
                // on the next ownership change rather than keeping it empty.
                TRAY.with(|slot| *slot.borrow_mut() = None);
                eprintln!("arcade-find: tray: {e}");
                svc.note_tray(TrayState::None);
            }
        }
    }
}

/// Toolkit-independent watcher setup; callbacks are worker-owned and must
/// dispatch native tray updates through UiMsg::Tray on Windows and macOS.
pub fn watch(svc: &Arc<Service>, callback: impl FnMut(TrayState) + Send + 'static) -> Result<TrayHostWatcher, String> {
    let callback = std::sync::Mutex::new(callback);
    TrayHostWatcher::start(
        WatcherConfig {
            locations: svc.locations.clone(),
            app: crate::link::me(),
            link_enabled: svc.settings().link.enabled,
            has_tray: true,
        },
        move |state| {
            callback.lock().unwrap_or_else(|e| e.into_inner())(state);
        },
    )
    .map_err(|e| e.to_string())
}

pub use imp::{set_state, start};
