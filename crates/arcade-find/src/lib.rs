//! Arcade Find: the app around `find-core` — the overlay, the resident
//! service, Arcade Link, and platform integration. `main.rs` is the CLI.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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
