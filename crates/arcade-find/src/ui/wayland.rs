//! Wayland backend: a wlr-layer-shell surface on the overlay layer, with no
//! anchors so the compositor centers it (it grows up and down around the
//! screen center). Exclusive keyboard focus while shown; destroying the
//! surface on hide hands focus back to the previous window. Software
//! rendering into shared memory; fractional scaling through
//! `wp_fractional_scale_v1` + `wp_viewporter` when the compositor has them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::channel::{channel, Event as ChannelEvent};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent as WlKeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use super::model::{metrics, Mode, Mods, Overlay};
use super::render::Renderer;
use crate::service::{Outcome, Service, UiMsg};
use crate::theme::Palette;

const BTN_LEFT: u32 = 0x110;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Whether this session can show a layer-shell overlay.
pub fn available() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return false;
    }
    let Ok(conn) = Connection::connect_to_env() else { return false };
    let Ok((globals, _)) = registry_queue_init::<Probe>(&conn) else { return false };
    globals.contents().with_list(|l| l.iter().any(|g| g.interface == "zwlr_layer_shell_v1"))
}

struct Probe;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

struct Shown {
    layer: LayerSurface,
    viewport: Option<wp_viewport::WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    configured: bool,
    /// The logical size last requested.
    size: (u32, u32),
}

struct Wl {
    svc: Arc<Service>,
    overlay: Overlay,
    renderer: Renderer,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    fractional_mgr: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    shown: Option<Shown>,
    /// Integer buffer scale (without fractional scaling).
    int_scale: i32,
    /// Fractional scale (120ths), when the compositor sends one.
    frac_scale: Option<u32>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    mods: Mods,
    has_focus: bool,
    pointer_pos: (f64, f64),
    last_click: Option<(Instant, usize)>,
    qh: QueueHandle<Wl>,
    handle: LoopHandle<'static, Wl>,
    toast_timer: Option<RegistrationToken>,
    exit: bool,
}

/// Runs the overlay on this thread until quit. `Err` if layer shell is missing.
pub fn run(svc: Arc<Service>, initial: Option<UiMsg>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, queue) = registry_queue_init::<Wl>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, Wl> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).map_err(|e| e.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("layer shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let pool = SlotPool::new((metrics::WIDTH as usize) * 600 * 4, &shm).map_err(|e| e.to_string())?;
    let fractional_mgr = globals.bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();
    let viewporter = globals.bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ()).ok();

    let (tx, rx) = channel::<UiMsg>();
    let tx = std::sync::Mutex::new(tx);
    svc.attach_ui(Arc::new(move |m| {
        if let Ok(t) = tx.lock() {
            let _ = t.send(m);
        }
    }));
    handle
        .insert_source(rx, |ev, _, st: &mut Wl| {
            if let ChannelEvent::Msg(m) = ev {
                st.message(m);
            }
        })
        .map_err(|e| e.to_string())?;

    let mut overlay = Overlay::default();
    svc.configure(&mut overlay);
    let mut st = Wl {
        svc: svc.clone(),
        overlay,
        renderer: Renderer::new(),
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        shm,
        pool,
        fractional_mgr,
        viewporter,
        shown: None,
        int_scale: 1,
        frac_scale: None,
        keyboard: None,
        pointer: None,
        mods: Mods::default(),
        has_focus: false,
        pointer_pos: (0.0, 0.0),
        last_click: None,
        qh: qh.clone(),
        handle: handle.clone(),
        toast_timer: None,
        exit: false,
    };
    if let Some(m) = initial {
        st.message(m);
    }
    while !st.exit {
        event_loop.dispatch(None, &mut st).map_err(|e| e.to_string())?;
    }
    st.hide();
    let _ = conn.flush();
    Ok(())
}

impl Wl {
    fn message(&mut self, m: UiMsg) {
        let svc = self.svc.clone();
        let out = svc.on_msg(&mut self.overlay, m);
        self.apply(out);
    }

    fn apply(&mut self, out: Outcome) {
        if out.quit {
            self.exit = true;
            return;
        }
        if out.hide {
            self.hide();
            return;
        }
        if out.show && self.shown.is_none() {
            self.show();
            return;
        }
        if out.redraw {
            self.update();
        }
    }

    fn scale(&self) -> f32 {
        match self.frac_scale {
            Some(s) if self.viewporter.is_some() => s as f32 / 120.0,
            _ => self.int_scale as f32,
        }
    }

    fn logical_size(&self) -> (u32, u32) {
        (metrics::WIDTH as u32, self.overlay.height().round() as u32)
    }

    fn show(&mut self) {
        if self.shown.is_some() {
            return;
        }
        self.overlay.visible = true;
        self.svc.set_visible(true);
        let surface = self.compositor.create_surface(&self.qh);
        let fractional = self.fractional_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, &self.qh, ()));
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, &self.qh, ()));
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("arcade-find"), None);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_exclusive_zone(-1);
        let size = self.logical_size();
        layer.set_size(size.0, size.1);
        layer.commit();
        self.shown = Some(Shown { layer, viewport, fractional, configured: false, size });
    }

    fn hide(&mut self) {
        if let Some(s) = self.shown.take() {
            if let Some(v) = s.viewport {
                v.destroy();
            }
            if let Some(f) = s.fractional {
                f.destroy();
            }
            drop(s.layer);
        }
        if self.overlay.visible {
            self.overlay.hide();
        }
        self.svc.set_visible(false);
        self.has_focus = false;
        self.frac_scale = None;
    }

    /// Resizes (and waits for the configure) or redraws.
    fn update(&mut self) {
        let size = self.logical_size();
        let Some(s) = self.shown.as_mut() else { return };
        if s.size != size {
            s.size = size;
            s.layer.set_size(size.0, size.1);
            s.layer.commit();
            return;
        }
        if s.configured {
            self.draw();
        }
        self.schedule_toast();
    }

    fn schedule_toast(&mut self) {
        if let Some(t) = self.toast_timer.take() {
            self.handle.remove(t);
        }
        let Some(until) = self.overlay.toast.as_ref().map(|t| t.until) else { return };
        let timer = Timer::from_deadline(until);
        self.toast_timer = self
            .handle
            .insert_source(timer, |_, _, st: &mut Wl| {
                st.toast_timer = None;
                st.overlay.tick(Instant::now());
                st.update();
                TimeoutAction::Drop
            })
            .ok();
    }

    fn draw(&mut self) {
        let scale = self.scale();
        let settings = self.svc.settings();
        let palette = Palette::for_theme(settings.theme, true);
        let Some(pm) = self.renderer.render(&self.overlay, &palette, scale, find_core::now_secs()) else { return };
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        let Some(s) = self.shown.as_ref() else { return };
        let Ok((buffer, canvas)) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        // tiny-skia: premultiplied RGBA; ARGB8888 little-endian: B, G, R, A.
        for (dst, src) in canvas.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        let surface = s.layer.wl_surface();
        match &s.viewport {
            Some(vp) if self.frac_scale.is_some() => {
                surface.set_buffer_scale(1);
                vp.set_destination(s.size.0 as i32, s.size.1 as i32);
            }
            _ => {
                if let Some(vp) = &s.viewport {
                    vp.set_destination(-1, -1);
                }
                surface.set_buffer_scale(self.int_scale);
            }
        }
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            s.layer.commit();
        }
    }

    fn effects(&mut self, effects: Vec<super::model::Effect>) {
        let svc = self.svc.clone();
        let mut out = Outcome { redraw: true, ..Default::default() };
        for e in effects {
            let o = svc.effect(&mut self.overlay, e);
            out.hide |= o.hide;
            out.quit |= o.quit;
        }
        self.apply(out);
    }

    fn key(&mut self, event: WlKeyEvent) {
        if self.shown.is_none() {
            return;
        }
        let ev = super::keys::from_keysym(event.keysym, event.utf8.as_deref(), self.mods);
        let svc = self.svc.clone();
        let effects = self.overlay.key(&ev, &*svc);
        self.effects(effects);
    }

    /// The list row under logical y, as an absolute index.
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

impl CompositorHandler for Wl {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        if self.int_scale != factor {
            self.int_scale = factor.max(1);
            self.update();
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Wl {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Wl {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        // The compositor removed the surface (output gone): just hide.
        self.hide();
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _cfg: LayerSurfaceConfigure, _serial: u32) {
        let Some(s) = self.shown.as_mut() else { return };
        if s.layer.wl_surface() != layer.wl_surface() {
            return;
        }
        s.configured = true;
        self.draw();
        self.schedule_toast();
    }
}

impl SeatHandler for Wl {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let handle = self.handle.clone();
            self.keyboard =
                self.seat_state.get_keyboard_with_repeat(qh, &seat, None, handle, Box::new(|st: &mut Wl, _, ev| st.key(ev))).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Wl {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.shown.as_ref().is_some_and(|s| s.layer.wl_surface() == surface) {
            self.has_focus = true;
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        // Focus moved elsewhere (another window or overlay took it): hide.
        if self.has_focus && self.shown.as_ref().is_some_and(|s| s.layer.wl_surface() == surface) {
            self.hide();
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: WlKeyEvent) {
        self.key(event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: WlKeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: u32) {
        self.mods = Mods { ctrl: m.ctrl, shift: m.shift, alt: m.alt, logo: m.logo };
    }
}

impl PointerHandler for Wl {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for ev in events {
            if !self.shown.as_ref().is_some_and(|s| s.layer.wl_surface() == &ev.surface) {
                continue;
            }
            self.pointer_pos = ev.position;
            match ev.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    let Some(i) = self.row_at(ev.position.1) else { continue };
                    let now = Instant::now();
                    let double = self.last_click.is_some_and(|(t, j)| j == i && now - t < DOUBLE_CLICK);
                    self.last_click = if double { None } else { Some((now, i)) };
                    let svc = self.svc.clone();
                    let effects = self.overlay.click(i, double, &*svc);
                    self.effects(effects);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let lines =
                        if vertical.discrete != 0 { vertical.discrete as isize } else { (vertical.absolute / 20.0).round() as isize };
                    if lines != 0 {
                        self.overlay.scroll_by(lines);
                        self.update();
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for Wl {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Wl {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, ()> for Wl {
    fn event(
        _: &mut Self,
        _: &wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for Wl {
    fn event(
        st: &mut Self,
        obj: &wp_fractional_scale_v1::WpFractionalScaleV1,
        ev: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev {
            let ours = st.shown.as_ref().and_then(|s| s.fractional.as_ref()).is_some_and(|f| f.id() == obj.id());
            if ours && st.frac_scale != Some(scale) {
                st.frac_scale = Some(scale);
                if st.shown.as_ref().is_some_and(|s| s.configured) {
                    st.draw();
                }
            }
        }
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for Wl {
    fn event(_: &mut Self, _: &wp_viewporter::WpViewporter, _: wp_viewporter::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wp_viewport::WpViewport, ()> for Wl {
    fn event(_: &mut Self, _: &wp_viewport::WpViewport, _: wp_viewport::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_compositor!(Wl);
delegate_output!(Wl);
delegate_shm!(Wl);
delegate_seat!(Wl);
delegate_keyboard!(Wl);
delegate_pointer!(Wl);
delegate_layer!(Wl);
delegate_registry!(Wl);
