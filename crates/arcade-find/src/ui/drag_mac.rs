//! Dragging results out on macOS: an AppKit dragging session from the
//! overlay's view, one pasteboard item (`public.file-url`) per file with its
//! Finder icon as the image. Copy or link only: the files never move.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSPasteboardItem,
    NSPasteboardTypeFileURL, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class keeps
    // no state.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArcadeFindDragSource"]
    struct DragSource;

    unsafe impl NSObjectProtocol for DragSource {}

    unsafe impl NSDraggingSource for DragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operations(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy | NSDragOperation::Link
        }
    }
);

impl DragSource {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The source of the drag in flight (AppKit doesn't keep it alive).
    static SOURCE: RefCell<Option<Retained<DragSource>>> = const { RefCell::new(None) };
}

/// Starts a dragging session for `paths` from `ns_view` during the current
/// mouse-dragged event. The session runs on AppKit's event loop.
pub fn drag(ns_view: *mut c_void, paths: &[PathBuf]) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    // SAFETY: winit's AppKit handle is the window's live content NSView.
    let view: &NSView = unsafe { &*(ns_view as *const NSView) };
    let event = NSApplication::sharedApplication(mtm).currentEvent().ok_or("no current event")?;
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let workspace = NSWorkspace::sharedWorkspace();
    let mut items: Vec<Retained<NSDraggingItem>> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let path = NSString::from_str(&path.to_string_lossy());
        let Some(url) = NSURL::fileURLWithPath(&path).absoluteString() else { continue };
        let item = NSPasteboardItem::new();
        // SAFETY: NSPasteboardTypeFileURL is an AppKit constant.
        if !item.setString_forType(&url, unsafe { NSPasteboardTypeFileURL }) {
            continue;
        }
        let dragging = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*item));
        let icon = workspace.iconForFile(&path);
        let step = (i.min(4) as f64) * 4.0;
        let frame = NSRect::new(NSPoint::new(at.x - 16.0 + step, at.y - 16.0 - step), NSSize::new(32.0, 32.0));
        // SAFETY: an NSImage is valid dragging contents.
        unsafe { dragging.setDraggingFrame_contents(frame, Some(&icon)) };
        items.push(dragging);
    }
    if items.is_empty() {
        return Err("none of the files could be dragged".into());
    }
    let source = DragSource::new(mtm);
    let _session =
        view.beginDraggingSessionWithItems_event_source(&NSArray::from_retained_slice(&items), &event, ProtocolObject::from_ref(&*source));
    SOURCE.with(|s| *s.borrow_mut() = Some(source));
    Ok(())
}
