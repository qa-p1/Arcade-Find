//! Local window geometry, separated from portable preferences. Each monitor
//! keeps its own size/position; first show follows the cursor's monitor.
use std::collections::BTreeMap;
use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct Monitor {
    key: String,
    origin: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Geometry {
    position: [f32; 2],
    size: [f32; 2],
}

impl Geometry {
    fn on_monitor(self, monitor: &Monitor) -> Self {
        let minimum = [arcade_link::design::size::WINDOW_MIN_WIDTH, arcade_link::design::size::WINDOW_MIN_HEIGHT];
        let mut result = self;
        for (axis, min) in minimum.into_iter().enumerate() {
            if !result.size[axis].is_finite() {
                result.size[axis] = min;
            }
            result.size[axis] = result.size[axis].clamp(min, monitor.size[axis].max(min));
            let start = monitor.origin[axis];
            let end = (start + monitor.size[axis] - result.size[axis]).max(start);
            if !result.position[axis].is_finite() {
                result.position[axis] = start;
            }
            result.position[axis] = result.position[axis].clamp(start, end);
        }
        result
    }
}

pub(crate) struct WindowState {
    file: PathBuf,
    monitors: BTreeMap<String, Geometry>,
    monitor: Option<Monitor>,
    initial: Geometry,
}

impl WindowState {
    pub(crate) fn new(paths: &find_core::paths::AppPaths, role: &str, size: [f32; 2]) -> Self {
        let file = paths.data.join(format!("{role}-windows.json"));
        let monitors: BTreeMap<String, Geometry> = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let monitor = cursor_monitor();
        let initial = match &monitor {
            Some(monitor) => monitors
                .get(&monitor.key)
                .copied()
                .unwrap_or(Geometry {
                    position: [monitor.origin[0] + (monitor.size[0] - size[0]) / 2.0, monitor.origin[1] + (monitor.size[1] - size[1]) / 2.0],
                    size,
                })
                .on_monitor(monitor),
            None => monitors.get("compositor").copied().unwrap_or(Geometry { position: [0.0; 2], size }),
        };
        Self { file, monitors, monitor, initial }
    }

    pub(crate) fn viewport(&self, viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
        let viewport = viewport.with_inner_size(self.initial.size);
        if self.monitor.is_some() {
            viewport.with_position(self.initial.position)
        } else {
            viewport
        }
    }

    pub(crate) fn observe(&mut self, ctx: &egui::Context) {
        let (inner, outer) = ctx.input(|input| (input.viewport().inner_rect, input.viewport().outer_rect));
        let Some(inner) = inner else { return };
        // Viewport rectangles include text zoom; native window coordinates do not.
        let zoom = ctx.zoom_factor();
        let geometry = Geometry {
            size: [inner.width() * zoom, inner.height() * zoom],
            position: [outer.unwrap_or(inner).min.x * zoom, outer.unwrap_or(inner).min.y * zoom],
        };
        if !geometry.size.iter().chain(&geometry.position).all(|number| number.is_finite()) {
            return;
        }
        let key = self.monitor.as_ref().map_or("compositor", |monitor| monitor.key.as_str());
        // A moved window must never make a different monitor's saved position
        // restore off-screen. Clamp that monitor's record to its own bounds.
        self.monitors.insert(key.into(), self.monitor.as_ref().map_or(geometry, |monitor| geometry.on_monitor(monitor)));
    }
}

impl Drop for WindowState {
    fn drop(&mut self) {
        if self.monitors.is_empty() {
            return;
        }
        match serde_json::to_vec_pretty(&self.monitors)
            .map_err(std::io::Error::other)
            .and_then(|bytes| arcade_link::paths::write_atomic(&self.file, &bytes, true))
        {
            Ok(()) => {}
            Err(error) => eprintln!("arcade-find: Couldn't save the window position. {error}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn cursor_monitor() -> Option<Monitor> {
    if crate::hotkey::hyprland::active() {
        let query = |name| {
            let output = std::process::Command::new("hyprctl").args(["-j", name]).output().ok()?;
            output.status.success().then(|| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()).flatten()
        };
        let cursor = query("cursorpos")?;
        let point = [cursor["x"].as_f64()? as f32, cursor["y"].as_f64()? as f32];
        for monitor in query("monitors")?.as_array()? {
            let scale = monitor["scale"].as_f64().unwrap_or(1.0) as f32;
            let origin = [monitor["x"].as_f64()? as f32, monitor["y"].as_f64()? as f32];
            let size = [monitor["width"].as_f64()? as f32 / scale, monitor["height"].as_f64()? as f32 / scale];
            if (0..2).all(|axis| point[axis] >= origin[axis] && point[axis] < origin[axis] + size[axis]) {
                return Some(Monitor { key: monitor["name"].as_str()?.into(), origin, size });
            }
        }
        return None;
    }
    // Wayland intentionally leaves placement to the compositor; querying an
    // Xwayland root here would choose the wrong physical monitor.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    use x11rb::connection::Connection;
    use x11rb::protocol::{randr::ConnectionExt as _, xproto::ConnectionExt as _};
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots[screen].root;
    let pointer = connection.query_pointer(root).ok()?.reply().ok()?;
    let monitors = connection.randr_get_monitors(root, true).ok()?.reply().ok()?;
    let monitor = monitors.monitors.into_iter().find(|monitor| {
        i32::from(pointer.root_x) >= i32::from(monitor.x)
            && i32::from(pointer.root_x) < i32::from(monitor.x) + i32::from(monitor.width)
            && i32::from(pointer.root_y) >= i32::from(monitor.y)
            && i32::from(pointer.root_y) < i32::from(monitor.y) + i32::from(monitor.height)
    })?;
    let name = connection.get_atom_name(monitor.name).ok()?.reply().ok()?.name;
    Some(Monitor {
        key: String::from_utf8_lossy(&name).into(),
        origin: [monitor.x as f32, monitor.y as f32],
        size: [monitor.width as f32, monitor.height as f32],
    })
}

#[cfg(windows)]
fn cursor_monitor() -> Option<Monitor> {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST},
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::GetCursorPos,
        },
    };
    let mut point = POINT { x: 0, y: 0 };
    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: writable Win32 structs of the exact required size; the monitor
    // handle is only used synchronously and is owned by the OS.
    unsafe {
        if GetCursorPos(&mut point) == 0 {
            return None;
        }
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>()) == 0 {
            return None;
        }
        let (mut x, mut y) = (96, 96);
        let scale = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0 { x as f32 / 96.0 } else { 1.0 };
        let rect = info.monitorInfo.rcWork;
        let end = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
        Some(Monitor {
            key: String::from_utf16_lossy(&info.szDevice[..end]),
            origin: [rect.left as f32 / scale, rect.top as f32 / scale],
            size: [(rect.right - rect.left) as f32 / scale, (rect.bottom - rect.top) as f32 / scale],
        })
    }
}

#[cfg(target_os = "macos")]
fn cursor_monitor() -> Option<Monitor> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};
    let screens = NSScreen::screens(MainThreadMarker::new()?);
    let pointer = NSEvent::mouseLocation();
    let primary_top = screens.firstObject()?.frame().size.height;
    for screen in screens.iter() {
        let frame = screen.frame();
        if pointer.x >= frame.origin.x
            && pointer.x < frame.origin.x + frame.size.width
            && pointer.y >= frame.origin.y
            && pointer.y < frame.origin.y + frame.size.height
        {
            return Some(Monitor {
                key: screen.localizedName().to_string(),
                origin: [frame.origin.x as f32, (primary_top - frame.origin.y - frame.size.height) as f32],
                size: [frame.size.width as f32, frame.size.height as f32],
            });
        }
    }
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn cursor_monitor() -> Option<Monitor> {
    None
}

/// Used by the software overlay to choose the same monitor as native windows.
pub(crate) fn cursor_monitor_name() -> Option<String> {
    cursor_monitor().map(|monitor| monitor.key)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_restores_inside_a_negative_origin_monitor_after_resolution_changes() {
        let monitor = Monitor { key: "left".into(), origin: [-1280.0, -200.0], size: [1280.0, 800.0] };
        let stored = Geometry { position: [-2500.0, 900.0], size: [1900.0, 1200.0] }.on_monitor(&monitor);
        assert_eq!(stored.size, [1280.0, 800.0]);
        assert_eq!(stored.position, [-1280.0, -200.0]);
        let invalid = Geometry { position: [f32::NAN; 2], size: [f32::NAN; 2] }.on_monitor(&monitor);
        assert_eq!(invalid.size, [720.0, 520.0]);
        assert_eq!(invalid.position, monitor.origin);
    }
}
