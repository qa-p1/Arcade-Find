//! Hidden-action attention only. Never put indexed names or raw errors in a
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
