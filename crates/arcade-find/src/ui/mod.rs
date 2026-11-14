//! The overlay: model (state + keys), renderer, and window backends.

pub mod accessibility;
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
pub mod wl_drag;
#[cfg(target_os = "linux")]
pub mod xdnd;

#[cfg(test)]
mod lifecycle_tests {
    /// FD-2 / the fleet popup rule: nothing hides the popup because focus
    /// left it. Only Escape, a choice, the shortcut or an outside click do.
    #[test]
    fn focus_loss_never_closes_the_popup() {
        for (name, source) in [("desktop.rs", include_str!("desktop.rs")), ("wayland.rs", include_str!("wayland.rs"))] {
            for marker in ["focus_loss_delay", "leave_at", "leave_timer"] {
                assert!(!source.contains(marker), "FD-2: {name} schedules a hide after focus is lost ({marker})");
            }
        }
    }
}
