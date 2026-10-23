//! The page area of a window on macOS (ZK-310): where a browser's page stands inside its window,
//! without the title bar, the tabs, the address bar and the side panels — through the
//! Accessibility API: the window's largest descendant of role `AXWebArea`. Needs the
//! Accessibility permission (the system asks the first time; until it is given — a clear error).
//! A browser builds its accessibility tree when a client first asks: the first look may find
//! nothing yet, so a few looks a moment apart.
//!
//! Raw C calls (HIServices / CoreFoundation): a handful of functions, no binding crate; the
//! strings and arrays come back toll-free bridged to the Foundation classes objc2 gives us.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::c_void;
use znimok_platform::Rect;

type CFTypeRef = *const c_void;
type AXUIElementRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
}

const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;
/// Elements looked at under one window before giving up (a page can hold tens of thousands).
const NODE_LIMIT: usize = 6000;
const DEPTH_LIMIT: usize = 48;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

/// A retained CF value, released when dropped.
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a +1 reference this wrapper owns.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn attr(el: AXUIElementRef, name: &str) -> Option<Cf> {
    let key = NSString::from_str(name);
    let mut out: CFTypeRef = std::ptr::null();
    // SAFETY: the element and the key live for the call; `out` receives a +1 reference or stays
    // null on an error code.
    let err =
        unsafe { AXUIElementCopyAttributeValue(el, Retained::as_ptr(&key) as CFTypeRef, &mut out) };
    if err != 0 || out.is_null() {
        return None;
    }
    Some(Cf(out))
}

fn as_array(v: &Cf) -> Option<&NSArray<AnyObject>> {
    // SAFETY: a CFArray is toll-free bridged to NSArray; the type is checked first.
    unsafe {
        if CFGetTypeID(v.0) != CFArrayGetTypeID() {
            return None;
        }
        Some(&*(v.0 as *const NSArray<AnyObject>))
    }
}

fn as_string(v: &Cf) -> Option<String> {
    // SAFETY: a CFString is toll-free bridged to NSString; the type is checked first.
    unsafe {
        if CFGetTypeID(v.0) != CFStringGetTypeID() {
            return None;
        }
        Some((*(v.0 as *const NSString)).to_string())
    }
}

fn frame_of(el: AXUIElementRef) -> Option<(f64, f64, f64, f64)> {
    let p = attr(el, "AXPosition")?;
    let s = attr(el, "AXSize")?;
    let (mut pt, mut sz) = (CGPoint::default(), CGSize::default());
    // SAFETY: AXValue holders of the kinds asked for; the out pointers are the right structs.
    unsafe {
        if !AXValueGetValue(p.0, AX_VALUE_CGPOINT, &mut pt as *mut _ as *mut c_void)
            || !AXValueGetValue(s.0, AX_VALUE_CGSIZE, &mut sz as *mut _ as *mut c_void)
        {
            return None;
        }
    }
    Some((pt.x, pt.y, sz.width, sz.height))
}

/// Whether this process may use the Accessibility API; `prompt` shows the system's question
/// the first time.
pub fn accessibility_trusted(prompt: bool) -> bool {
    // SAFETY: the option key is a framework constant; the dictionary lives for the call.
    unsafe {
        let key = &*(kAXTrustedCheckOptionPrompt as *const NSString);
        let value = NSNumber::new_bool(prompt);
        let opts: Retained<NSDictionary<NSString, NSNumber>> =
            NSDictionary::from_slices(&[key], &[&*value]);
        AXIsProcessTrustedWithOptions(Retained::as_ptr(&opts) as CFTypeRef)
    }
}

/// The page area (points, desktop units) of the window of `pid` whose frame is `bounds`, or
/// `None` when it has none (not a browser). `Err` without the Accessibility permission.
pub fn content_rect(pid: u32, bounds: Rect) -> Result<Option<Rect>, String> {
    if !accessibility_trusted(true) {
        return Err(
            "Znimok needs the Accessibility permission for the page area of a window: \
                    System Settings → Privacy & Security → Accessibility (the system has just \
                    asked); then try again"
                .into(),
        );
    }
    let mut best = None;
    for _ in 0..4 {
        best = largest_web_area(pid, bounds);
        if best.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let Some((x, y, w, h)) = best else {
        return Ok(None);
    };
    let x0 = (x.round() as i32).max(bounds.x);
    let y0 = (y.round() as i32).max(bounds.y);
    let x1 = ((x + w).round() as i32).min(bounds.x + bounds.width as i32);
    let y1 = ((y + h).round() as i32).min(bounds.y + bounds.height as i32);
    if x1 - x0 < 16 || y1 - y0 < 16 {
        return Ok(None);
    }
    Ok(Some(Rect {
        x: x0,
        y: y0,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    }))
}

fn largest_web_area(pid: u32, bounds: Rect) -> Option<(f64, f64, f64, f64)> {
    // SAFETY: an application element for the pid; released by the wrapper.
    let app = Cf(unsafe { AXUIElementCreateApplication(pid as i32) });
    if app.0.is_null() {
        return None;
    }
    let windows = attr(app.0, "AXWindows")?;
    let list = as_array(&windows)?;
    // The window whose frame is the one asked for (the app may have several).
    let mut window: Option<AXUIElementRef> = None;
    let mut fallback: Option<AXUIElementRef> = None;
    for i in 0..list.len() {
        let w = Retained::as_ptr(&list.objectAtIndex(i)) as AXUIElementRef;
        fallback.get_or_insert(w);
        if let Some((x, y, wd, ht)) = frame_of(w)
            && (x - bounds.x as f64).abs() <= 2.0
            && (y - bounds.y as f64).abs() <= 2.0
            && (wd - bounds.width as f64).abs() <= 2.0
            && (ht - bounds.height as f64).abs() <= 2.0
        {
            window = Some(w);
            break;
        }
    }
    let window = window.or(if list.len() == 1 { fallback } else { None })?;
    // Depth-first over the children, the largest AXWebArea wins.
    let mut best: Option<(f64, f64, f64, f64)> = None;
    let mut seen = 0usize;
    let mut stack: Vec<(Cf, usize)> = Vec::new();
    if let Some(children) = attr(window, "AXChildren") {
        stack.push((children, 1));
    }
    while let Some((children, depth)) = stack.pop() {
        let Some(arr) = as_array(&children) else {
            continue;
        };
        for i in 0..arr.len() {
            seen += 1;
            if seen > NODE_LIMIT {
                return best;
            }
            let el = Retained::as_ptr(&arr.objectAtIndex(i)) as AXUIElementRef;
            let role = attr(el, "AXRole").and_then(|r| as_string(&r));
            if role.as_deref() == Some("AXWebArea") {
                if let Some(f) = frame_of(el)
                    && best.is_none_or(|b| f.2 * f.3 > b.2 * b.3)
                {
                    best = Some(f);
                }
                continue; // nothing of the kind nests inside a page
            }
            if depth < DEPTH_LIMIT
                && let Some(sub) = attr(el, "AXChildren")
            {
                stack.push((sub, depth + 1));
            }
        }
    }
    best
}
