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

/// Ignore focus changes caused by mapping, then debounce device-change grabs.
pub(crate) fn focus_loss_delay(shown: Option<std::time::Instant>, now: std::time::Instant) -> Option<std::time::Duration> {
    shown.filter(|at| now.saturating_duration_since(*at) >= std::time::Duration::from_millis(400)).map(|_| std::time::Duration::from_millis(150))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn mapping_focus_loss_is_ignored_for_four_hundred_milliseconds() {
        let shown = Instant::now();
        assert_eq!(focus_loss_delay(None, shown), None);
        for millis in [0, 1, 150, 399] {
            assert_eq!(focus_loss_delay(Some(shown), shown + Duration::from_millis(millis)), None);
        }
        for millis in [400, 401, 1000] {
            assert_eq!(focus_loss_delay(Some(shown), shown + Duration::from_millis(millis)), Some(Duration::from_millis(150)));
        }
    }
}
