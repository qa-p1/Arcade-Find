//! The overlay: model (state + keys), renderer, and window backends.

pub mod desktop;
pub mod dnd;
#[cfg(target_os = "macos")]
pub mod drag_mac;
#[cfg(windows)]
pub mod drag_win;
pub mod icons;
pub mod input;
pub mod keys;
pub mod model;
pub mod render;
pub mod text;
#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(target_os = "linux")]
pub mod xdnd;
