//! Dragging a file out of the window on macOS (ZK-64): an `NSDraggingSession` started from the
//! window's content view with the file's URL on the pasteboard. Unlike OLE on Windows it does
//! not block — AppKit runs the drag and tells the source when it ends.

use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSImage, NSView, NSWorkspace,
};
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::AppWindow;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class adds no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZnimokDragSource"]
    struct DragSource;

    unsafe impl NSObjectProtocol for DragSource {}

    unsafe impl NSDraggingSource for DragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }
);

impl DragSource {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: plain NSObject init.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The session holds its source weakly: keep ours alive for the app's lifetime.
    static SOURCE: std::cell::OnceCell<Retained<DragSource>> = const { std::cell::OnceCell::new() };
}

/// Starts dragging `path` as a file from `view` under the current mouse event. `false` when
/// there is no event to start from (the drag must begin inside a mouse-dragged event).
pub fn drag_file(view: &NSView, path: &Path) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(event) = app.currentEvent() else {
        return false;
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // SAFETY: NSURL conforms to NSPasteboardWriting.
    let writer = ProtocolObject::from_retained(url);
    let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &writer);
    // The file's own icon (Finder shows the PNG preview) under the pointer.
    let icon: Retained<NSImage> =
        NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()));
    let side = 64.0;
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let frame = NSRect::new(
        NSPoint::new(at.x - side / 2.0, at.y - side / 2.0),
        NSSize::new(side, side),
    );
    // SAFETY: the image is a valid NSImage; contents may be any object.
    unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
    let source = SOURCE.with(|s| s.get_or_init(|| DragSource::new(mtm)).clone());
    let items = NSArray::from_retained_slice(&[item]);
    let session = view.beginDraggingSessionWithItems_event_source(
        &items,
        &event,
        ProtocolObject::from_ref(&*source),
    );
    session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    true
}

/// [`drag_file`] from the editor window's content view.
pub fn drag_from(ui: &AppWindow, path: &Path) -> bool {
    use slint::ComponentHandle;
    ui.window()
        .with_winit_window(|w| {
            let handle = w.window_handle().ok()?;
            let RawWindowHandle::AppKit(a) = handle.as_raw() else {
                return None;
            };
            // SAFETY: winit hands out the NSView of a live window; main thread.
            let view: &NSView = unsafe { a.ns_view.cast().as_ref() };
            Some(drag_file(view, path))
        })
        .flatten()
        .unwrap_or(false)
}
