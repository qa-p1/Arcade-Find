//! The tray (menu bar) icon, with the family menu (SPEC §8.3):
//! Open Arcade Find · Open Settings · Restart Arcade Find · — · Quit Arcade Find.
//! A click opens Settings (on macOS the click opens the menu). Linux uses
//! StatusNotifierItem (ksni) and shows up when a tray host appears later.

use std::sync::{Arc, Weak};

use crate::instance::Command;
use crate::service::{Service, UiMsg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Open,
    Settings,
    Restart,
    Quit,
}

fn run(svc: &Weak<Service>, item: Item) {
    let Some(s) = svc.upgrade() else { return };
    match item {
        Item::Open => s.send(UiMsg::Show { query: None, reveal: None }),
        Item::Settings => s.open_settings(),
        Item::Restart => {
            let _ = s.command(Command::Restart);
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
        for p in rgba.chunks_exact(4) {
            data.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
        }
        ksni::Icon { width: px as i32, height: px as i32, data }
    }

    pub fn start(svc: &Arc<Service>) -> Result<(), String> {
        let tray = FindTray { svc: Arc::downgrade(svc), icon: vec![icon(22), icon(32), icon(64)] };
        let handle = tray.assume_sni_available(true).spawn().map_err(|e| e.to_string())?;
        // The tray lives as long as the process.
        std::mem::forget(handle);
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    /// Call on the main thread once the event loop runs.
    pub fn start(svc: &Arc<Service>) -> Result<(), String> {
        let open = MenuItem::new("Open Arcade Find", true, None);
        let settings = MenuItem::new("Open Settings", true, None);
        let restart = MenuItem::new("Restart Arcade Find", true, None);
        let quit = MenuItem::new("Quit Arcade Find", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &settings, &restart, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let px = 32;
        let icon = Icon::from_rgba(crate::ui::icons::app_icon_rgba(px), px, px).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Arcade Find")
            .with_icon(icon)
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()
            .map_err(|e| e.to_string())?;
        std::mem::forget(tray);
        let ids = [
            (open.id().clone(), Item::Open),
            (settings.id().clone(), Item::Settings),
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
        Ok(())
    }
}

pub use imp::start;
