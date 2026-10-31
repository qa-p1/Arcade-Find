//! Background attention and the one-time window-close explanation. Never put indexed names or raw errors in a
//! system notification. The detailed error stays in Find's persistent toast.
pub(crate) fn should_notify(enabled: bool, visible: bool, needs_attention: bool) -> bool {
    enabled && !visible && needs_attention
}

pub(crate) fn attention() {
    std::thread::spawn(|| {
        let mut notification = notify_rust::Notification::new();
        notification.appname("Arcade Find").summary("Arcade Find").body("Couldn't finish the action. Open Find for details.");
        #[cfg(windows)]
        notification.app_id(crate::os::APP_ID);
        if let Err(error) = notification.show() {
            crate::debug!("system notification unavailable: {error}");
        }
    });
}

/// This machine-local marker is outside the portable backup allowlist. A failed
/// notification does not consume it; the next close can explain tray lifetime.
pub(crate) fn background_once(paths: find_core::paths::AppPaths, enabled: bool) {
    if !enabled {
        return;
    }
    std::thread::spawn(move || {
        if find_core::paths::ensure_private_dir(&paths.data).is_err() {
            return;
        }
        let marker = paths.data.join("window-close-notice-seen");
        let Ok(claim) = std::fs::OpenOptions::new().write(true).create_new(true).open(&marker) else { return };
        drop(claim);
        let mut notification = notify_rust::Notification::new();
        notification.appname("Arcade Find").summary("Arcade Find").body("Arcade Find is still running. Quit it from the tray.");
        #[cfg(windows)]
        notification.app_id(crate::os::APP_ID);
        if let Err(error) = notification.show() {
            let _ = std::fs::remove_file(marker);
            crate::debug!("background notification unavailable: {error}");
        }
    });
}
