//! Who Find is to the desktop, Arcade Link and the user, in one place: the launcher, the window's app id, the tray,
//! notifications and the Link manifest all read these, so they can't drift apart.

/// The Arcade Link id; also the name of Find's config and data folders (`find-core`).
pub use find_core::paths::LINK_ID as ID;
pub const NAME: &str = "Arcade Find";
/// The OS identifier (STANDARD §10): bundle id, AppUserModelID, desktop file, Wayland app_id and X11 WM_CLASS.
pub const DESKTOP_ID: &str = "dev.arcade.Find";
pub const CLI_NAME: &str = "arcade-find";
pub const TAGLINE: &str = "Press a shortcut, type, and any file or folder shows up.";
