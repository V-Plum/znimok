//! Mouse clicks while recording (ZK-302): what Windows hears through `WH_MOUSE_LL` (ZK-90), here
//! through a listen-only `CGEventTap` on a thread of its own. ScreenCaptureKit draws the clicks
//! into the frames by itself, but they never reached the recording's log (the MOUS block): no
//! dots on the timeline, none in the report.
//!
//! The tap pushes each press and release into the recording's [`EventQueue`] the way the Windows
//! hook does: stamped with the recording clock ([`MachClock`], the host clock of the frames) and
//! placed in video pixels of the recorded part ([`ClickMap`]). The recorder's gate then drops
//! what came before the first frame and during a pause, and turns ticks into video time.
//!
//! A listen-only tap of mouse events needs no permission of its own (keyboard events would ask
//! for «Input Monitoring»); when the system refuses the tap anyway, [`ClickTap::active`] says so
//! and the recording goes on without the log.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use znimok_video::events::{Button, EventQueue, InputEvent};

use crate::clock::MachClock;

/// A rectangle of the desktop in points: x, y, width, height.
pub type Area = (f64, f64, f64, f64);

/// Where a click lands in the video: the recorded part of the desktop (points) onto the video's
/// pixels; clicks on the recording bar are not the recording's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickMap {
    pub frame: Area,
    pub size: (u32, u32),
    pub skip: Option<Area>,
}

impl ClickMap {
    /// The click at desktop point (`x`, `y`) in video pixels, or `None` for the bar. A click
    /// outside the recorded part comes out outside the video and is left out when the recording
    /// is wrapped, as on Windows (ZK-252).
    pub fn to_video(&self, x: f64, y: f64) -> Option<(i32, i32)> {
        if let Some(s) = self.skip
            && inside(s, x, y)
        {
            return None;
        }
        let (fx, fy, fw, fh) = self.frame;
        if fw <= 0.0 || fh <= 0.0 {
            return None;
        }
        let vx = (x - fx) * f64::from(self.size.0) / fw;
        let vy = (y - fy) * f64::from(self.size.1) / fh;
        Some((vx.floor() as i32, vy.floor() as i32))
    }
}

fn inside((ax, ay, aw, ah): Area, x: f64, y: f64) -> bool {
    x >= ax && y >= ay && x < ax + aw && y < ay + ah
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

type TapCallback = extern "C" fn(
    proxy: *mut c_void,
    ty: u32,
    event: *mut c_void,
    user: *mut c_void,
) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFMachPortInvalidate(port: *mut c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRemoveSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: bool) -> i32;
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
    static kCFRunLoopDefaultMode: *const c_void;
}

// CGEventTapLocation, CGEventTapPlacement, CGEventTapOptions.
const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const LISTEN_ONLY: u32 = 1;
// CGEventType.
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

struct Ctx {
    queue: EventQueue,
    map: Arc<Mutex<ClickMap>>,
    clock: MachClock,
    port: *mut c_void,
}

extern "C" fn on_event(
    _proxy: *mut c_void,
    ty: u32,
    event: *mut c_void,
    user: *mut c_void,
) -> *mut c_void {
    // SAFETY: `user` is the `Ctx` the tap was made with; it is freed only after the run loop
    // ended and the port was invalidated, so it outlives every call.
    let ctx = unsafe { &*(user as *const Ctx) };
    let (button, down) = match ty {
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT => {
            // SAFETY: the tap's own port, alive while its thread runs.
            unsafe { CGEventTapEnable(ctx.port, true) };
            return event;
        }
        LEFT_DOWN => (Button::Left, true),
        LEFT_UP => (Button::Left, false),
        RIGHT_DOWN => (Button::Right, true),
        RIGHT_UP => (Button::Right, false),
        OTHER_DOWN => (Button::Middle, true),
        OTHER_UP => (Button::Middle, false),
        _ => return event,
    };
    let ticks = ctx.clock.now_ns();
    // SAFETY: a mouse event handed to the callback, valid for this call.
    let at = unsafe { CGEventGetLocation(event) };
    let map = ctx.map.lock().map(|m| *m).ok();
    if let Some((x, y)) = map.and_then(|m| m.to_video(at.x, at.y)) {
        ctx.queue.push(InputEvent {
            ticks,
            x,
            y,
            button,
            down,
        });
    }
    event
}

/// The system's clicks into a recording's queue for as long as the tap lives.
pub struct ClickTap {
    map: Arc<Mutex<ClickMap>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    active: bool,
}

impl ClickTap {
    /// The tap on a thread of its own; `queue` is what the recording reads
    /// ([`crate::RecordRequest::events`]).
    pub fn start(queue: EventQueue, map: ClickMap) -> Self {
        let map = Arc::new(Mutex::new(map));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let (m, s) = (map.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("znimok-clicks".into())
            .spawn(move || run(queue, m, s, tx))
            .ok();
        let active = rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false);
        Self {
            map,
            stop,
            thread,
            active,
        }
    }

    /// The system gave the tap (else no clicks are logged).
    pub fn active(&self) -> bool {
        self.active
    }

    /// The recorded part moved (a followed window) or the bar did.
    pub fn set_map(&self, f: impl FnOnce(&mut ClickMap)) {
        if let Ok(mut m) = self.map.lock() {
            f(&mut m);
        }
    }

    /// The tap goes; nothing after this is the recording's.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ClickTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn run(
    queue: EventQueue,
    map: Arc<Mutex<ClickMap>>,
    stop: Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<bool>,
) {
    let mask: u64 = [
        LEFT_DOWN, LEFT_UP, RIGHT_DOWN, RIGHT_UP, OTHER_DOWN, OTHER_UP,
    ]
    .iter()
    .fold(0, |m, t| m | (1u64 << t));
    let ctx = Box::into_raw(Box::new(Ctx {
        queue,
        map,
        clock: MachClock::new(),
        port: std::ptr::null_mut(),
    }));
    // SAFETY: CoreGraphics/CoreFoundation calls on this thread's own run loop; `ctx` is freed
    // only after the port is invalidated and released, so the callback never sees it dangling.
    unsafe {
        let port = CGEventTapCreate(
            SESSION_TAP,
            HEAD_INSERT,
            LISTEN_ONLY,
            mask,
            on_event,
            ctx.cast(),
        );
        if port.is_null() {
            let _ = ready.send(false);
            drop(Box::from_raw(ctx));
            return;
        }
        (*ctx).port = port;
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
        let rl = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
        CGEventTapEnable(port, true);
        let _ = ready.send(true);
        // Short turns, so a stop asked at any moment is seen.
        while !stop.load(Ordering::SeqCst) {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.2, false);
        }
        CGEventTapEnable(port, false);
        CFRunLoopRemoveSource(rl, source, kCFRunLoopCommonModes);
        CFMachPortInvalidate(port);
        CFRelease(source);
        CFRelease(port);
        drop(Box::from_raw(ctx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points of the recorded part → pixels of the video (a Retina display: two to the point).
    #[test]
    fn a_click_lands_in_video_pixels() {
        let m = ClickMap {
            frame: (100.0, 50.0, 800.0, 600.0),
            size: (1600, 1200),
            skip: None,
        };
        assert_eq!(m.to_video(150.0, 75.5), Some((100, 51)));
        assert_eq!(m.to_video(899.9, 649.0), Some((1599, 1198)));
        // Outside the recorded part: outside the video, left out when the recording is wrapped.
        assert_eq!(m.to_video(50.0, 10.0), Some((-100, -80)));
    }

    /// The bar's clicks (Pause, Stop) are not the recording's.
    #[test]
    fn the_bar_is_left_out() {
        let m = ClickMap {
            frame: (0.0, 0.0, 1800.0, 1169.0),
            size: (3548, 2304),
            skip: Some((764.0, 1050.0, 272.0, 52.0)),
        };
        assert_eq!(m.to_video(800.0, 1070.0), None);
        assert!(m.to_video(10.0, 10.0).is_some());
    }

    /// The tap starts and stops cleanly; a click pushed by hand reaches the queue as the tap's
    /// would (the system's own clicks cannot be made here).
    #[test]
    fn the_tap_starts_and_stops() {
        let q = EventQueue::new();
        let tap = ClickTap::start(
            q.clone(),
            ClickMap {
                frame: (0.0, 0.0, 100.0, 100.0),
                size: (100, 100),
                skip: None,
            },
        );
        tap.set_map(|m| m.frame = (10.0, 10.0, 100.0, 100.0));
        tap.stop();
        assert!(q.is_empty());
    }
}
