//! Test helper for `scripts/e2e-linux.sh`: a virtual pointer (wlr protocol)
//! for headless compositors, driven by stdin lines:
//! `move X Y` (absolute output pixels), `down`, `up`, `sleep MS`.
//!
//!     cargo run --example e2e-pointer -- 1280 800

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::{wl_pointer::ButtonState, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 as Manager;
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as Pointer;

    const BTN_LEFT: u32 = 0x110;

    struct State;

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
    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }
    impl Dispatch<Manager, ()> for State {
        fn event(_: &mut Self, _: &Manager, _: <Manager as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }
    impl Dispatch<Pointer, ()> for State {
        fn event(_: &mut Self, _: &Pointer, _: <Pointer as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }

    pub fn main() {
        let mut args = std::env::args().skip(1).map(|a| a.parse::<u32>().expect("width height"));
        let (w, h) = (args.next().unwrap_or(1280), args.next().unwrap_or(800));
        let conn = Connection::connect_to_env().expect("WAYLAND_DISPLAY");
        let (globals, mut queue) = registry_queue_init::<State>(&conn).expect("registry");
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).expect("wl_seat");
        let manager: Manager = globals.bind(&qh, 1..=2, ()).expect("zwlr_virtual_pointer_manager_v1");
        let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
        let start = Instant::now();
        let ms = || start.elapsed().as_millis() as u32;
        for line in std::io::stdin().lock().lines() {
            let line = line.expect("stdin");
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["move", x, y] => {
                    pointer.motion_absolute(ms(), x.parse().expect("x"), y.parse().expect("y"), w, h);
                    pointer.frame();
                }
                ["down"] => {
                    pointer.button(ms(), BTN_LEFT, ButtonState::Pressed);
                    pointer.frame();
                }
                ["up"] => {
                    pointer.button(ms(), BTN_LEFT, ButtonState::Released);
                    pointer.frame();
                }
                ["sleep", n] => std::thread::sleep(Duration::from_millis(n.parse().expect("ms"))),
                [] => {}
                _ => eprintln!("e2e-pointer: ignored {line:?}"),
            }
            let _ = queue.roundtrip(&mut State);
        }
    }
}
