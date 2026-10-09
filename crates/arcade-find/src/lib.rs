//! Arcade Find: the app around `find-core` — the overlay, the resident
//! service, Arcade Link, and platform integration. `main.rs` is the CLI.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `ARCADE_FIND_DEBUG=1`: log UI events to stderr (never content or paths).
pub fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ARCADE_FIND_DEBUG").is_some_and(|v| v != "0"))
}

#[macro_export]
macro_rules! debug {
    ($($t:tt)*) => {
        if $crate::debug_enabled() {
            eprintln!("arcade-find: {}", format!($($t)*));
        }
    };
}

pub mod autostart;
pub mod hotkey;
pub mod instance;
pub mod link;
pub mod os;
pub mod service;
pub mod settings_ui;
pub mod snapshot;
pub mod theme;
pub mod tray;
pub mod ui;
