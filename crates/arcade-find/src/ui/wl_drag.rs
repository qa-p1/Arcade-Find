//! Dragging results out of the winit window on Wayland (GNOME and other
//! desktops without layer shell). winit has no drag source, so this runs a
//! second client on winit's own `wl_display`: its own event queue on its own
//! thread, its own `wl_pointer` (for the serial of the button press that
//! starts the drag) and a `wl_data_source` dragged from winit's surface.
//! Copy only: the files never move.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use smithay_client_toolkit::data_device_manager::{
    data_device::{DataDevice, DataDeviceHandler},
    data_offer::{DataOfferHandler, DragOffer},
    data_source::{DataSourceHandler, DragSource},
    DataDeviceManagerState, WritePipe,
};
use smithay_client_toolkit::reexports::calloop::channel::{channel, Event as ChannelEvent, Sender};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::shm::{slot::SlotPool, Shm, ShmHandler};
use smithay_client_toolkit::{delegate_data_device, delegate_shm};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::{wl_compositor, wl_data_device, wl_data_source, wl_pointer, wl_registry, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use super::dnd;

const BTN_LEFT: u32 = 0x110;

/// A drag image: premultiplied RGBA at `scale` buffer pixels per logical one.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub scale: i32,
}

struct Start {
    /// winit's `wl_surface`, the drag's origin.
    surface: usize,
    paths: Vec<PathBuf>,
    image: Option<Image>,
}

/// The drag source for one winit window, bound to winit's connection.
pub struct Dragger {
    tx: Sender<Start>,
    surface: usize,
    dragging: Arc<AtomicBool>,
}

impl Dragger {
    /// Starts the helper thread on winit's `display` for the window whose
    /// `wl_surface` is `surface`.
    ///
    /// # Safety
    /// `display` and `surface` must be winit's live `wl_display` and the
    /// window's `wl_surface`, and stay alive for as long as the process runs
    /// (the overlay window is created once and never destroyed).
    pub unsafe fn new(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) -> Result<Self, String> {
        // SAFETY: the caller guarantees a live wl_display.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let compositor = globals.bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=4, ()).map_err(|e| format!("wl_compositor: {e}"))?;
        let data_manager = DataDeviceManagerState::bind(&globals, &qh).map_err(|e| format!("wl_data_device_manager: {e}"))?;
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let pool = SlotPool::new(64 * 1024, &shm).map_err(|e| e.to_string())?;
        // The first seat: winit uses the same one for its pointer.
        globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
        let (tx, rx) = channel::<Start>();
        let dragging = Arc::new(AtomicBool::new(false));
        let flag = dragging.clone();
        std::thread::Builder::new()
            .name("wl-drag".into())
            .spawn(move || {
                let Ok(mut event_loop) = EventLoop::<State>::try_new() else { return };
                let handle = event_loop.handle();
                if WaylandSource::new(conn.clone(), queue).insert(handle.clone()).is_err() {
                    return;
                }
                let inserted = handle.insert_source(rx, |ev, _, st: &mut State| {
                    if let ChannelEvent::Msg(start) = ev {
                        st.start(start);
                    }
                });
                if inserted.is_err() {
                    return;
                }
                let mut st = State {
                    conn,
                    qh,
                    compositor,
                    data_manager,
                    shm,
                    pool,
                    pointer: None,
                    device: None,
                    press: None,
                    drag: None,
                    dragging: flag,
                };
                while event_loop.dispatch(None, &mut st).is_ok() {}
                crate::debug!("wl-drag: connection closed");
            })
            .map_err(|e| e.to_string())?;
        Ok(Dragger { tx, surface: surface as usize, dragging })
    }

    /// Drags `paths` from the window. The button must still be down: the
    /// compositor only starts a drag for the press it is grabbing.
    pub fn drag(&self, paths: Vec<PathBuf>, image: Option<Image>) -> Result<(), String> {
        self.tx.send(Start { surface: self.surface, paths, image }).map_err(|_| "drag thread stopped".to_string())
    }

    /// Whether a drag is in flight (the window keeps showing while it is).
    pub fn dragging(&self) -> bool {
        self.dragging.load(Ordering::Acquire)
    }
}

/// A drag in flight: the compositor asks the source for data.
struct Drag {
    source: DragSource,
    paths: Vec<PathBuf>,
    icon: Option<wl_surface::WlSurface>,
}

impl Drop for Drag {
    fn drop(&mut self) {
        if let Some(icon) = self.icon.take() {
            icon.destroy();
        }
    }
}

struct State {
    conn: Connection,
    qh: QueueHandle<State>,
    compositor: wl_compositor::WlCompositor,
    data_manager: DataDeviceManagerState,
    shm: Shm,
    pool: SlotPool,
    pointer: Option<wl_pointer::WlPointer>,
    device: Option<DataDevice>,
    /// Serial of the left press being held, if any.
    press: Option<u32>,
    drag: Option<Drag>,
    dragging: Arc<AtomicBool>,
}

impl State {
    fn start(&mut self, start: Start) {
        let (Some(serial), Some(device)) = (self.press, self.device.as_ref()) else {
            crate::debug!("wl-drag: no button held, drag not started");
            return;
        };
        // SAFETY: winit's live wl_surface (see Dragger::new).
        let id = unsafe { ObjectId::from_ptr(wl_surface::WlSurface::interface(), start.surface as *mut _) };
        let Ok(origin) = id.and_then(|id| wl_surface::WlSurface::from_id(&self.conn, id)) else {
            crate::debug!("wl-drag: window surface unavailable");
            return;
        };
        crate::debug!("wl-drag: drag of {} item(s)", start.paths.len());
        let source = self.data_manager.create_drag_and_drop_source(
            &self.qh,
            [dnd::URI_LIST, dnd::TEXT, "text/plain", "UTF8_STRING"],
            DndAction::Copy,
        );
        let icon = self.compositor.create_surface(&self.qh, ());
        source.start_drag(device, &origin, Some(&icon), serial);
        if let Some(image) = start.image {
            self.paint(&icon, &image);
        }
        self.dragging.store(true, Ordering::Release);
        self.drag = Some(Drag { source, paths: start.paths, icon: Some(icon) });
    }

    fn paint(&mut self, icon: &wl_surface::WlSurface, image: &Image) {
        let (w, h) = (image.width as i32, image.height as i32);
        let Ok((buffer, canvas)) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        for (dst, src) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(image.rgba.as_chunks::<4>().0) {
            *dst = [src[2], src[1], src[0], src[3]];
        }
        if icon.version() >= 3 {
            icon.set_buffer_scale(image.scale);
        }
        if icon.version() >= 4 {
            icon.damage_buffer(0, 0, w, h);
        } else {
            icon.damage(0, 0, i32::MAX, i32::MAX);
        }
        if buffer.attach_to(icon).is_ok() {
            icon.commit();
        }
    }

    fn end(&mut self, source: &wl_data_source::WlDataSource) {
        if self.drag.as_ref().is_some_and(|d| d.source.inner() == source) {
            self.drag = None;
            self.dragging.store(false, Ordering::Release);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
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

impl Dispatch<wl_compositor::WlCompositor, ()> for State {
    fn event(_: &mut Self, _: &wl_compositor::WlCompositor, _: wl_compositor::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for State {
    fn event(_: &mut Self, _: &wl_surface::WlSurface, _: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(st: &mut Self, seat: &wl_seat::WlSeat, ev: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = ev else { return };
        let has_pointer = caps.contains(wl_seat::Capability::Pointer);
        if has_pointer && st.pointer.is_none() {
            st.pointer = Some(seat.get_pointer(qh, ()));
            if st.device.is_none() {
                st.device = Some(st.data_manager.get_data_device(qh, seat));
            }
        } else if !has_pointer {
            if let Some(p) = st.pointer.take() {
                if p.version() >= 3 {
                    p.release();
                }
            }
            st.press = None;
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(st: &mut Self, _: &wl_pointer::WlPointer, ev: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            wl_pointer::Event::Button { serial, button: BTN_LEFT, state, .. } => {
                st.press = (state == WEnum::Value(wl_pointer::ButtonState::Pressed)).then_some(serial);
            }
            wl_pointer::Event::Leave { .. } if st.drag.is_none() => st.press = None,
            _ => {}
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl DataSourceHandler for State {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: Option<String>) {}

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        mime: String,
        mut fd: WritePipe,
    ) {
        let Some(drag) = self.drag.as_ref().filter(|d| d.source.inner() == source) else { return };
        if let Some(bytes) = dnd::payload(&drag.paths, &mime) {
            if let Err(e) = fd.write_all(&bytes) {
                crate::debug!("wl-drag: data ({mime}) not sent: {e}");
            }
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &wl_data_source::WlDataSource) {
        crate::debug!("wl-drag: cancelled");
        self.end(source);
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &wl_data_source::WlDataSource) {
        crate::debug!("wl-drag: finished");
        self.end(source);
    }

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: DndAction) {}
}

// Find is never a drop target; offers from other clients are ignored.
impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
}

impl DataOfferHandler for State {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

delegate_data_device!(State);
delegate_shm!(State);
