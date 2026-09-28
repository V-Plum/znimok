//! System «Share» on macOS (ZK-63): `NSSharingServicePicker` next to the anchor (the button that
//! was clicked), over the app's view. Files go as file URLs — Messages, Mail, AirDrop, Notes take
//! them; the picker lists what the system offers for that file type.
//!
//! AppKit UI: [`MacShare::share`] must run on the main thread (the app's event loop); from another
//! thread it returns an error rather than touching AppKit.

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSScreen, NSSharingServicePicker, NSView};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};
use znimok_platform::{PlatformError, Rect, Result, Share};

pub struct MacShare {
    /// The `NSView` the picker is shown over (the app window's content view), as a raw pointer.
    view: usize,
}

// SAFETY: the pointer is only dereferenced on the main thread (checked with MainThreadMarker).
unsafe impl Send for MacShare {}
// SAFETY: as above.
unsafe impl Sync for MacShare {}

impl MacShare {
    /// # Safety
    /// `ns_view` must be a live `NSView*` for as long as this value is used (the app's window
    /// content view — winit/Slint give it as the raw window handle).
    pub unsafe fn new(ns_view: *mut std::ffi::c_void) -> Self {
        Self {
            view: ns_view as usize,
        }
    }
}

/// Global desktop units (origin top-left of the primary display, y down) → Cocoa screen
/// coordinates (origin bottom-left, y up).
fn to_cocoa(r: Rect, primary_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(r.x as f64, primary_height - (r.y as f64 + r.height as f64)),
        NSSize::new(r.width as f64, r.height as f64),
    )
}

impl Share for MacShare {
    fn available(&self) -> bool {
        true
    }

    fn share(&self, files: &[PathBuf], anchor: Option<Rect>) -> Result<()> {
        let mtm = MainThreadMarker::new().ok_or(PlatformError::Other(
            "Share must be called on the main thread".into(),
        ))?;
        if files.is_empty() {
            return Err(PlatformError::Other("nothing to share".into()));
        }
        for f in files {
            if !f.exists() {
                return Err(PlatformError::NotFound(f.display().to_string()));
            }
        }
        // SAFETY: the caller of `new` guarantees a live view; we are on the main thread.
        let view: &NSView = unsafe { &*(self.view as *const NSView) };
        let urls: Vec<Retained<AnyObject>> = files
            .iter()
            .map(|f| {
                let url = NSURL::fileURLWithPath(&NSString::from_str(&f.to_string_lossy()));
                Retained::into_super(Retained::into_super(url))
            })
            .collect();
        let items = NSArray::from_retained_slice(&urls);
        // SAFETY: an array of file URLs, as documented.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items)
        };
        let rect = match (anchor, view.window()) {
            (Some(a), Some(win)) => {
                let primary_h = NSScreen::screens(mtm)
                    .firstObject()
                    .map(|s| s.frame().size.height)
                    .unwrap_or(0.0);
                let in_window = win.convertRectFromScreen(to_cocoa(a, primary_h));
                view.convertRect_fromView(in_window, None)
            }
            _ => view.bounds(),
        };
        picker.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_units_to_cocoa() {
        let r = to_cocoa(
            Rect {
                x: 100,
                y: 50,
                width: 30,
                height: 20,
            },
            1080.0,
        );
        assert_eq!((r.origin.x, r.origin.y), (100.0, 1010.0));
        assert_eq!((r.size.width, r.size.height), (30.0, 20.0));
    }

    /// Off the main thread (test threads are not) it refuses instead of touching AppKit.
    #[test]
    fn refuses_off_the_main_thread() {
        // SAFETY: never dereferenced — the main-thread check comes first.
        let s = unsafe { MacShare::new(std::ptr::null_mut()) };
        assert!(s.share(&[PathBuf::from("/tmp")], None).is_err());
    }
}
