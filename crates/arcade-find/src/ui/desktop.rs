//! winit + softbuffer backend: X11, Windows, macOS, and Wayland desktops
//! without layer shell (GNOME). A borderless always-on-top window, centered
//! on the monitor and re-centered as it grows. Hiding it hands focus back to
//! the previous app (on macOS by hiding the app).

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use super::model::{metrics, Effect, Mode, Mods, Overlay};
use super::render::Renderer;
use crate::service::{Outcome, Service, UiMsg};
use crate::theme::Palette;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

struct App {
    svc: Arc<Service>,
    overlay: Overlay,
    renderer: Renderer,
    window: Option<Rc<Window>>,
    surface: Option<Surface>,
    mods: Mods,
    cursor_y: f64,
    last_click: Option<(Instant, usize)>,
    /// The window had focus since it was shown (so losing it means "hide").
    focused: bool,
    /// The screen center the window stays centered on while shown.
    center: Option<PhysicalPosition<i32>>,
    on_start: Option<Box<dyn FnOnce()>>,
    initial: Option<UiMsg>,
}

/// Runs the overlay on this (main) thread until quit. `on_start` runs once
/// the event loop is up (tray icons on Windows and macOS need that).
pub fn run(svc: Arc<Service>, initial: Option<UiMsg>, on_start: Option<Box<dyn FnOnce()>>) -> Result<(), String> {
    let mut builder = EventLoop::<UiMsg>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().map_err(|e| e.to_string())?;
    let proxy: Mutex<EventLoopProxy<UiMsg>> = Mutex::new(event_loop.create_proxy());
    svc.attach_ui(Arc::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(m);
        }
    }));
    let mut overlay = Overlay::default();
    svc.configure(&mut overlay);
    let mut renderer = Renderer::new();
    renderer.radius = if cfg!(windows) { 8.0 } else { 0.0 };
    let mut app = App {
        svc,
        overlay,
        renderer,
        window: None,
        surface: None,
        mods: Mods::default(),
        cursor_y: 0.0,
        last_click: None,
        focused: false,
        center: None,
        on_start,
        initial,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())
}

fn attributes() -> WindowAttributes {
    #[allow(unused_mut)]
    let mut a = Window::default_attributes()
        .with_title("Arcade Find")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_inner_size(LogicalSize::new(metrics::WIDTH as f64, metrics::BAR as f64));
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::{WindowAttributesExtX11, WindowType};
        a = a.with_name("arcade-find", "Arcade Find").with_x11_window_type(vec![WindowType::Utility]);
    }
    #[cfg(windows)]
    {
        use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
        a = a.with_skip_taskbar(true).with_corner_preference(CornerPreference::Round);
    }
    a
}

impl App {
    fn window(&self) -> Option<&Rc<Window>> {
        self.window.as_ref()
    }

    fn apply(&mut self, out: Outcome) {
        if out.hide {
            self.hide();
            return;
        }
        if out.show {
            self.show();
        }
        if out.redraw || out.show {
            self.resize_and_redraw();
        }
    }

    fn logical_height(&self) -> f64 {
        self.overlay.height() as f64
    }

    fn show(&mut self) {
        let Some(w) = self.window().cloned() else { return };
        self.overlay.visible = true;
        self.svc.set_visible(true);
        let monitor = w.current_monitor().or_else(|| w.primary_monitor());
        if let Some(m) = monitor {
            let (pos, size) = (m.position(), m.size());
            self.center = Some(PhysicalPosition::new(pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2));
        }
        self.place(&w);
        self.focused = false;
        w.set_visible(true);
        w.focus_window();
    }

    fn hide(&mut self) {
        if self.overlay.visible {
            self.overlay.hide();
        }
        self.svc.set_visible(false);
        if let Some(w) = self.window() {
            w.set_visible(false);
        }
        self.focused = false;
        return_focus();
    }

    /// Sizes the window for the overlay and keeps it centered.
    fn place(&self, w: &Window) {
        let scale = w.scale_factor();
        let _ = w.request_inner_size(LogicalSize::new(metrics::WIDTH as f64, self.logical_height()));
        if let Some(c) = self.center {
            let pw = (metrics::WIDTH as f64 * scale).round() as i32;
            let ph = (self.logical_height() * scale).round() as i32;
            w.set_outer_position(PhysicalPosition::new(c.x - pw / 2, c.y - ph / 2));
        }
    }

    fn resize_and_redraw(&mut self) {
        let Some(w) = self.window().cloned() else { return };
        if !self.overlay.visible {
            return;
        }
        let want = (metrics::WIDTH as f64 * w.scale_factor()).round() as u32;
        let want_h = (self.logical_height() * w.scale_factor()).round() as u32;
        let size = w.inner_size();
        if size.width != want || size.height != want_h {
            self.place(&w);
        }
        w.request_redraw();
    }

    fn draw(&mut self) {
        let Some(w) = self.window().cloned() else { return };
        let Some(surface) = self.surface.as_mut() else { return };
        let scale = w.scale_factor() as f32;
        let palette = Palette::for_theme(self.svc.settings().theme, false);
        let Some(pm) = self.renderer.render(&self.overlay, &palette, scale, find_core::now_secs()) else { return };
        let size = w.inner_size();
        let (Some(bw), Some(bh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(bw, bh).is_err() {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        // The window is opaque: composite the panel over its own color.
        let bg = palette.panel;
        let (pw, ph) = (pm.width() as usize, pm.height() as usize);
        let data = pm.data();
        let (bw, bh) = (bw.get() as usize, bh.get() as usize);
        for y in 0..bh {
            for x in 0..bw {
                let px = if x < pw && y < ph {
                    let o = (y * pw + x) * 4;
                    let a = data[o + 3] as u32;
                    let inv = 255 - a;
                    let r = data[o] as u32 + bg.r as u32 * inv / 255;
                    let g = data[o + 1] as u32 + bg.g as u32 * inv / 255;
                    let b = data[o + 2] as u32 + bg.b as u32 * inv / 255;
                    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
                } else {
                    ((bg.r as u32) << 16) | ((bg.g as u32) << 8) | bg.b as u32
                };
                buf[y * bw + x] = px;
            }
        }
        let _ = buf.present();
    }

    fn effects(&mut self, effects: Vec<Effect>) {
        let svc = self.svc.clone();
        let mut out = Outcome { redraw: true, ..Default::default() };
        for e in effects {
            let o = svc.effect(&mut self.overlay, e);
            out.hide |= o.hide;
        }
        self.apply(out);
    }

    fn row_at(&self, y: f64) -> Option<usize> {
        let top = (metrics::BAR + metrics::LIST_PAD) as f64;
        if y < top || !self.overlay.shows_list() {
            return None;
        }
        let vi = ((y - top) / metrics::ROW as f64) as usize;
        if vi >= self.overlay.visible_rows() {
            return None;
        }
        let scroll = match &self.overlay.mode {
            Mode::Actions { scroll, .. } => *scroll,
            _ => self.overlay.scroll,
        };
        Some(scroll + vi)
    }
}

impl ApplicationHandler<UiMsg> for App {
    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.overlay.tick(Instant::now());
            if let Some(w) = self.window() {
                w.request_redraw();
            }
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Ok(w) = el.create_window(attributes()) else {
            el.exit();
            return;
        };
        let w = Rc::new(w);
        let Ok(ctx) = softbuffer::Context::new(w.clone()) else {
            el.exit();
            return;
        };
        self.surface = softbuffer::Surface::new(&ctx, w.clone()).ok();
        self.window = Some(w);
        if let Some(f) = self.on_start.take() {
            f();
        }
        if let Some(m) = self.initial.take() {
            self.user_event(el, m);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, m: UiMsg) {
        let svc = self.svc.clone();
        let out = svc.on_msg(&mut self.overlay, m);
        if out.quit {
            self.hide();
            el.exit();
            return;
        }
        self.apply(out);
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::CloseRequested => self.hide(),
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) => {
                if self.focused && self.overlay.visible {
                    self.hide();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => self.resize_and_redraw(),
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.mods = Mods { ctrl: s.control_key(), shift: s.shift_key(), alt: s.alt_key(), logo: s.super_key() };
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.overlay.visible => {
                let ev = super::keys::from_winit(&event.logical_key, event.text.as_deref(), self.mods);
                let svc = self.svc.clone();
                let effects = self.overlay.key(&ev, &*svc);
                self.effects(effects);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window().map_or(1.0, |w| w.scale_factor());
                self.cursor_y = position.y / scale;
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let Some(i) = self.row_at(self.cursor_y) else { return };
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, j)| j == i && now - t < DOUBLE_CLICK);
                self.last_click = if double { None } else { Some((now, i)) };
                let svc = self.svc.clone();
                let effects = self.overlay.click(i, double, &*svc);
                self.effects(effects);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -(y.round() as isize),
                    MouseScrollDelta::PixelDelta(p) => -((p.y / metrics::ROW as f64).round() as isize),
                };
                if lines != 0 {
                    self.overlay.scroll_by(lines);
                    if let Some(w) = self.window() {
                        w.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.overlay.toast.as_ref().map(|t| t.until) {
            Some(until) => el.set_control_flow(ControlFlow::WaitUntil(until)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Gives focus back to the app that had it before the overlay.
fn return_focus() {
    #[cfg(target_os = "macos")]
    {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            app.hide(None);
        }
    }
}
