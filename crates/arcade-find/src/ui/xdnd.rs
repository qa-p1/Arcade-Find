//! XDND drag source for X11 (protocol version 5). winit can't start a drag,
//! so this runs its own connection: an unmapped helper window owns
//! `XdndSelection`, a short modal loop follows the pointer until the left
//! button is released, talks to the XdndAware window under it, and serves
//! the drop target's selection requests. Copy only: the files never move.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, ImageFormat,
    KeyButMask, PropMode, SelectionNotifyEvent, SelectionRequestEvent, StackMode, Window, WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use super::dnd;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        XdndAware,
        XdndSelection,
        XdndEnter,
        XdndPosition,
        XdndStatus,
        XdndLeave,
        XdndDrop,
        XdndFinished,
        XdndActionCopy,
        XdndTypeList,
        TARGETS,
        UTF8_STRING,
        URI_LIST: b"text/uri-list",
        TEXT_UTF8: b"text/plain;charset=utf-8",
        TEXT_PLAIN: b"text/plain",
    }
}

const VERSION: u32 = 5;
/// How long to wait for the target to fetch the data after the drop.
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);
/// Where the drag image sits relative to the pointer.
const IMAGE_GAP: i16 = 14;

/// The drag image: premultiplied RGBA, composited over `background`.
pub struct Image {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub background: [u8; 3],
}

struct Source {
    conn: RustConnection,
    root: Window,
    win: Window,
    atoms: Atoms,
    paths: Vec<PathBuf>,
    /// The target under the pointer and its protocol version.
    target: Option<(Window, u32)>,
    accepted: bool,
    awaiting_status: bool,
    finished: bool,
    /// The override-redirect window showing the drag image.
    image: Option<Window>,
}

/// Drags `paths` until the left button is released; true when a target
/// accepted the drop. `own` (the overlay window) is never a target.
pub fn drag(paths: Vec<PathBuf>, own: u32, image: Option<Image>) -> Result<bool, String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let (conn, screen) = x11rb::connect(None).map_err(|x| e(&x))?;
    let root = conn.setup().roots[screen].root;
    let atoms = Atoms::new(&conn).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
    let win = conn.generate_id().map_err(|x| e(&x))?;
    conn.create_window(0, win, root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map_err(|x| e(&x))?;
    let mut s =
        Source { conn, root, win, atoms, paths, target: None, accepted: false, awaiting_status: false, finished: false, image: None };
    if let Some(img) = image {
        // Cosmetic: a drag that can't show its image still works.
        s.image = s.image_window(screen, &img).ok();
    }
    let result = s.run(own).map_err(|x| e(&x));
    if let Some(w) = s.image {
        let _ = s.conn.destroy_window(w);
    }
    let _ = s.conn.destroy_window(s.win);
    let _ = s.conn.flush();
    result
}

impl Source {
    fn types(&self) -> [Atom; 4] {
        [self.atoms.URI_LIST, self.atoms.TEXT_UTF8, self.atoms.TEXT_PLAIN, self.atoms.UTF8_STRING]
    }

    fn mime(&self, atom: Atom) -> Option<&'static str> {
        let a = &self.atoms;
        match atom {
            x if x == a.URI_LIST => Some(dnd::URI_LIST),
            x if x == a.TEXT_UTF8 => Some(dnd::TEXT),
            x if x == a.TEXT_PLAIN => Some("text/plain"),
            x if x == a.UTF8_STRING => Some("UTF8_STRING"),
            _ => None,
        }
    }

    /// An unmanaged window holding the drag image as its background.
    fn image_window(&self, screen: usize, img: &Image) -> Result<Window, Box<dyn std::error::Error>> {
        let setup = self.conn.setup();
        let scr = &setup.roots[screen];
        let depth = scr.root_depth;
        let bpp = setup.pixmap_formats.iter().find(|f| f.depth == depth).map(|f| f.bits_per_pixel);
        if depth != 24 || bpp != Some(32) {
            return Err("unsupported visual".into());
        }
        let (w, h) = (img.width, img.height);
        let [br, bg, bb] = img.background.map(u32::from);
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        for px in img.rgba.as_chunks::<4>().0 {
            let inv = 255 - px[3] as u32;
            let c = |v: u8, b: u32| (v as u32 + b * inv / 255).min(255) as u8;
            data.extend_from_slice(&[c(px[2], bb), c(px[1], bg), c(px[0], br), 0]);
        }
        let pixmap = self.conn.generate_id()?;
        self.conn.create_pixmap(depth, pixmap, scr.root, w, h)?;
        let gc = self.conn.generate_id()?;
        self.conn.create_gc(gc, pixmap, &CreateGCAux::new())?;
        // In strips that fit the request size.
        let row = w as usize * 4;
        let rows_per = ((self.conn.maximum_request_bytes().saturating_sub(64)) / row).clamp(1, h as usize);
        for y in (0..h as usize).step_by(rows_per) {
            let n = rows_per.min(h as usize - y);
            self.conn.put_image(ImageFormat::Z_PIXMAP, pixmap, gc, w, n as u16, 0, y as i16, 0, depth, &data[y * row..(y + n) * row])?;
        }
        let win = self.conn.generate_id()?;
        let aux = CreateWindowAux::new().override_redirect(1).background_pixmap(pixmap).border_pixel(0);
        self.conn.create_window(depth, win, scr.root, -1000, -1000, w, h, 0, WindowClass::INPUT_OUTPUT, scr.root_visual, &aux)?;
        self.conn.free_gc(gc)?;
        self.conn.free_pixmap(pixmap)?;
        self.conn.map_window(win)?;
        Ok(win)
    }

    fn run(&mut self, own: u32) -> Result<bool, Box<dyn std::error::Error>> {
        let types = self.types();
        self.conn.change_property32(PropMode::REPLACE, self.win, self.atoms.XdndTypeList, AtomEnum::ATOM, &types)?;
        self.conn.set_selection_owner(self.win, self.atoms.XdndSelection, CURRENT_TIME)?;
        self.conn.flush()?;
        let mut last = (i16::MIN, i16::MIN);
        loop {
            self.pump()?;
            let p = self.conn.query_pointer(self.root)?.reply()?;
            if u16::from(p.mask) & u16::from(KeyButMask::BUTTON1) == 0 {
                break;
            }
            if let Some(img) = self.image {
                if (p.root_x, p.root_y) != last {
                    let at = ConfigureWindowAux::new()
                        .x(i32::from(p.root_x) + i32::from(IMAGE_GAP))
                        .y(i32::from(p.root_y) + i32::from(IMAGE_GAP))
                        .stack_mode(StackMode::ABOVE);
                    self.conn.configure_window(img, &at)?;
                }
            }
            let under = self.find_target(p.root_x, p.root_y, own)?;
            if under.map(|t| t.0) != self.target.map(|t| t.0) {
                if let Some((w, _)) = self.target {
                    self.send(w, self.atoms.XdndLeave, [self.win, 0, 0, 0, 0])?;
                }
                self.target = under;
                self.accepted = false;
                self.awaiting_status = false;
                last = (i16::MIN, i16::MIN);
                if let Some((w, v)) = self.target {
                    self.send(w, self.atoms.XdndEnter, [self.win, (v << 24) | 1, types[0], types[1], types[2]])?;
                }
            }
            if let Some((w, _)) = self.target {
                if !self.awaiting_status && (p.root_x, p.root_y) != last {
                    last = (p.root_x, p.root_y);
                    let pos = ((p.root_x as u16 as u32) << 16) | p.root_y as u16 as u32;
                    self.send(w, self.atoms.XdndPosition, [self.win, 0, pos, CURRENT_TIME, self.atoms.XdndActionCopy])?;
                    self.awaiting_status = true;
                }
            }
            self.conn.flush()?;
            std::thread::sleep(Duration::from_millis(12));
        }
        // Released: let a pending status arrive, then drop or leave.
        let settle = Instant::now() + Duration::from_millis(300);
        while self.awaiting_status && Instant::now() < settle {
            self.pump()?;
            std::thread::sleep(Duration::from_millis(5));
        }
        let Some((w, _)) = self.target else { return Ok(false) };
        if !self.accepted {
            self.send(w, self.atoms.XdndLeave, [self.win, 0, 0, 0, 0])?;
            self.conn.flush()?;
            return Ok(false);
        }
        self.send(w, self.atoms.XdndDrop, [self.win, 0, CURRENT_TIME, 0, 0])?;
        self.conn.flush()?;
        let deadline = Instant::now() + FINISH_TIMEOUT;
        while !self.finished && Instant::now() < deadline {
            self.pump()?;
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(self.finished)
    }

    /// The innermost XdndAware window under (x, y), skipping our own.
    fn find_target(&self, x: i16, y: i16, own: u32) -> Result<Option<(Window, u32)>, Box<dyn std::error::Error>> {
        let mut parent = self.root;
        loop {
            let child = self.conn.translate_coordinates(self.root, parent, x, y)?.reply()?.child;
            if child == NONE || child == own || Some(child) == self.image {
                return Ok(None);
            }
            let aware = self.conn.get_property(false, child, self.atoms.XdndAware, AtomEnum::ATOM, 0, 1)?.reply()?;
            if let Some(v) = aware.value32().and_then(|mut v| v.next()) {
                return Ok(Some((child, v.min(VERSION))));
            }
            parent = child;
        }
    }

    fn send(&self, to: Window, kind: Atom, data: [u32; 5]) -> Result<(), Box<dyn std::error::Error>> {
        let ev = ClientMessageEvent::new(32, to, kind, data);
        self.conn.send_event(false, to, EventMask::NO_EVENT, ev)?;
        Ok(())
    }

    fn pump(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(ev) = self.conn.poll_for_event()? {
            match ev {
                Event::ClientMessage(m) if m.type_ == self.atoms.XdndStatus => {
                    let d = m.data.as_data32();
                    if self.target.is_some_and(|t| t.0 == d[0]) {
                        self.accepted = d[1] & 1 != 0;
                        self.awaiting_status = false;
                    }
                }
                Event::ClientMessage(m) if m.type_ == self.atoms.XdndFinished => {
                    let d = m.data.as_data32();
                    if self.target.is_some_and(|t| t.0 == d[0]) {
                        self.finished = true;
                    }
                }
                Event::SelectionRequest(r) => self.answer(r)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Hands the drop target the data it asks for.
    fn answer(&self, r: SelectionRequestEvent) -> Result<(), Box<dyn std::error::Error>> {
        let property = if r.property == NONE { r.target } else { r.property };
        let mut reply = property;
        if r.selection != self.atoms.XdndSelection {
            reply = NONE;
        } else if r.target == self.atoms.TARGETS {
            let mut list = vec![self.atoms.TARGETS];
            list.extend(self.types());
            self.conn.change_property32(PropMode::REPLACE, r.requestor, property, AtomEnum::ATOM, &list)?;
        } else if let Some(bytes) = self.mime(r.target).and_then(|m| dnd::payload(&self.paths, m)) {
            self.conn.change_property8(PropMode::REPLACE, r.requestor, property, r.target, &bytes)?;
        } else {
            reply = NONE;
        }
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: r.time,
            requestor: r.requestor,
            selection: r.selection,
            target: r.target,
            property: reply,
        };
        self.conn.send_event(false, r.requestor, EventMask::NO_EVENT, notify)?;
        self.conn.flush()?;
        Ok(())
    }
}
