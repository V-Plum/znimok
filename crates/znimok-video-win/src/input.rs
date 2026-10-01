//! The cursor and the clicks in a recording (ZK-90), as Little Helpers draws them
//! (`inventory_video.md` §4):
//!
//! - clicks come from a low-level mouse hook (`WH_MOUSE_LL`) on a thread of its own, stamped
//!   with QPC — the recording clock — and go into the recording's [`EventQueue`] in video
//!   pixels (the `MOUS` log) and into a short list of recent presses for the rings; a press on
//!   one of Znimok's own windows (the recording bar) is left out;
//! - the cursor is read at every slot (`GetCursorInfo`), its picture made again only when the
//!   `HCURSOR` changes, as "multiplier + addition" per pixel so an I-beam inverts what is under
//!   it (monochrome AND/XOR, colour with alpha, colour with a mask);
//! - a press is a ring that spreads for 350 ms (radius `(6 + 20·p)·scale`, opacity `1 − p`), a
//!   right press a double ring, the left button held longer than 350 ms a steady ring of
//!   `14·scale` at the cursor.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MonitorFromPoint, ReleaseDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor,
    GetCursorInfo, GetIconInfo, GetMessageW, HCURSOR, HHOOK, HICON, ICONINFO, MSG, MSLLHOOKSTRUCT,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WindowFromPoint,
};
use znimok_video::events::{Button, EventQueue, InputEvent};

use crate::clock::{QpcClock, qpc_now};
use crate::shader::{CursorImage, DesktopMap, Overlay, Ring};
use crate::source::{OverlayFn, f16_bits};
use znimok_video::clock::Clock;

/// How long a press spreads, and after how long a held left button becomes a steady ring.
const RING_MS: i64 = 350;
/// The largest cursor picture taken (LH: 512 × 512).
const CURSOR_MAX: u32 = 512;

/// What a recording shows of the mouse.
#[derive(Clone, Copy, Debug)]
pub struct MouseOpts {
    pub cursor: bool,
    pub clicks: bool,
    /// The rings' colour, sRGB 0..1.
    pub color: [f32; 3],
}

/// A press seen by the hook, on the desktop.
#[derive(Clone, Copy, Debug)]
struct Press {
    ticks: i64,
    x: i32,
    y: i32,
    button: Button,
    down: bool,
}

#[derive(Default)]
struct Shared {
    queue: EventQueue,
    recent: Mutex<VecDeque<Press>>,
    /// Where the desktop is in the video now (the last slot's), for the `MOUS` coordinates.
    map: Mutex<Option<DesktopMap>>,
    /// Top-level windows whose clicks are not recorded (the recording bar).
    exclude: Mutex<Vec<isize>>,
}

/// The one hook of the process (a low-level hook procedure has no context of its own).
fn current() -> &'static Mutex<Option<Arc<Shared>>> {
    static CURRENT: OnceLock<Mutex<Option<Arc<Shared>>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(None))
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let what = match wp.0 as u32 {
            WM_LBUTTONDOWN => Some((Button::Left, true)),
            WM_LBUTTONUP => Some((Button::Left, false)),
            WM_RBUTTONDOWN => Some((Button::Right, true)),
            WM_RBUTTONUP => Some((Button::Right, false)),
            WM_MBUTTONDOWN => Some((Button::Middle, true)),
            WM_MBUTTONUP => Some((Button::Middle, false)),
            _ => None,
        };
        let shared = current().lock().ok().and_then(|c| c.clone());
        if let (Some((button, down)), Some(s)) = (what, shared) {
            // SAFETY: for WH_MOUSE_LL, lParam points at an MSLLHOOKSTRUCT for this call.
            let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
            record(&s, info.pt, button, down);
        }
    }
    // SAFETY: passing the event on, as every hook must.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

fn record(s: &Shared, pt: POINT, button: Button, down: bool) {
    // SAFETY: plain window queries; a stale handle only fails the comparison.
    let root = unsafe { GetAncestor(WindowFromPoint(pt), GA_ROOT) }.0 as isize;
    if s.exclude.lock().is_ok_and(|e| e.contains(&root)) {
        return;
    }
    let ticks = qpc_now();
    if let Ok(mut r) = s.recent.lock() {
        r.push_back(Press {
            ticks,
            x: pt.x,
            y: pt.y,
            button,
            down,
        });
        while r.len() > 64 {
            r.pop_front();
        }
    }
    let (x, y) = match s.map.lock().ok().and_then(|m| *m) {
        Some(m) => {
            let (x, y) = m.to_video(pt.x, pt.y);
            (x.round() as i32, y.round() as i32)
        }
        None => (pt.x, pt.y),
    };
    s.queue.push(InputEvent {
        ticks,
        x,
        y,
        button,
        down,
    });
}

/// The mouse of one recording: the hook thread and what the frames draw.
pub struct MouseInput {
    shared: Arc<Shared>,
    thread: Option<(u32, JoinHandle<()>)>,
}

impl MouseInput {
    /// Starts the hook (it lives until [`MouseInput::stop`] or drop).
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        if let Ok(mut c) = current().lock() {
            *c = Some(shared.clone());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("znimok-mouse".into())
            .spawn(move || {
                // SAFETY: the hook and the message loop belong to this thread; the loop ends on
                // WM_QUIT and the hook is removed before the thread returns.
                unsafe {
                    let mut msg = MSG::default();
                    // Make the thread's message queue before anyone posts to it.
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let module = GetModuleHandleW(None).ok().map(|m| m.into());
                    let h: Option<HHOOK> =
                        SetWindowsHookExW(WH_MOUSE_LL, Some(hook), module, 0).ok();
                    let _ = tx.send(GetCurrentThreadId());
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    if let Some(h) = h {
                        let _ = UnhookWindowsHookEx(h);
                    }
                }
            })
            .ok();
        let thread = thread.and_then(|t| rx.recv().ok().map(|id| (id, t)));
        Self { shared, thread }
    }

    /// The queue the recording logs from (`RecordRequest::events`).
    pub fn queue(&self) -> EventQueue {
        self.shared.queue.clone()
    }

    /// Clicks on this top-level window are not recorded (the recording bar).
    pub fn exclude(&self, hwnd: isize) {
        if let Ok(mut e) = self.shared.exclude.lock() {
            e.push(hwnd);
        }
    }

    /// A press at a desktop point as if the hook saw it — the self-test's clicks (the real
    /// mouse is not touched); later the browser extension's.
    pub fn note(&self, x: i32, y: i32, button: Button, down: bool) {
        record(&self.shared, POINT { x, y }, button, down);
    }

    /// What each frame draws of the mouse (`RecordRequest::overlay`).
    pub fn overlay(&self, opts: MouseOpts) -> OverlayFn {
        let shared = self.shared.clone();
        let mut cursor = CursorCache::default();
        let clock = QpcClock::new();
        let mut scale: Option<f32> = None;
        Box::new(move |_slot, map: &DesktopMap| {
            if let Ok(mut m) = shared.map.lock() {
                *m = Some(*map);
            }
            let s = *scale.get_or_insert_with(|| dpi_scale(map.origin));
            let now = clock.ticks();
            let f = clock.frequency();
            let pointer = cursor_pos();
            let rings = if opts.clicks {
                let recent: Vec<Press> = shared
                    .recent
                    .lock()
                    .map(|r| r.iter().copied().collect())
                    .unwrap_or_default();
                rings_at(&recent, now, f, s, pointer, map)
            } else {
                Vec::new()
            };
            let cursor = opts.cursor.then(|| cursor.now(map)).flatten();
            Overlay {
                rings,
                cursor,
                scale: s,
                ring_color: opts.color,
            }
        })
    }

    pub fn stop(&mut self) {
        if let Some((id, t)) = self.thread.take() {
            // SAFETY: posting WM_QUIT to the hook thread's queue ends its loop.
            let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = t.join();
        }
        if let Ok(mut c) = current().lock()
            && c.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.shared))
        {
            *c = None;
        }
    }
}

impl Drop for MouseInput {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Rings at `now` from the recent presses (LH `VidOverlay::Frame`): the newest 8.
fn rings_at(
    recent: &[Press],
    now: i64,
    f: i64,
    s: f32,
    pointer: Option<(i32, i32)>,
    map: &DesktopMap,
) -> Vec<Ring> {
    let ms = |t: i64| (now - t) * 1000 / f;
    let mut rings = Vec::new();
    for p in recent.iter().filter(|p| p.down) {
        let age = ms(p.ticks);
        if !(0..RING_MS).contains(&age) {
            continue;
        }
        let k = age as f32 / RING_MS as f32;
        let (x, y) = map.to_video(p.x, p.y);
        rings.push(Ring {
            x,
            y,
            radius: (6.0 + 20.0 * k) * s,
            alpha: 1.0 - k,
            kind: u32::from(p.button == Button::Right),
        });
    }
    // The left button still held after 350 ms: a steady ring under the cursor.
    let last_left = recent.iter().rev().find(|p| p.button == Button::Left);
    if let Some(p) = last_left
        && p.down
        && ms(p.ticks) >= RING_MS
    {
        let (px, py) = pointer.unwrap_or((p.x, p.y));
        let (x, y) = map.to_video(px, py);
        rings.push(Ring {
            x,
            y,
            radius: 14.0 * s,
            alpha: 0.85,
            kind: 2,
        });
    }
    let keep = rings.len().saturating_sub(Overlay::MAX_RINGS);
    rings.drain(..keep);
    rings
}

/// The display's scale at the recorded content (96 DPI = 1).
fn dpi_scale(origin: (i32, i32)) -> f32 {
    // SAFETY: plain monitor queries; failure leaves 96 DPI.
    unsafe {
        let m = MonitorFromPoint(
            POINT {
                x: origin.0,
                y: origin.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x.max(96) as f32 / 96.0
    }
}

fn cursor_info() -> Option<CURSORINFO> {
    let mut ci = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: a sized out-structure.
    unsafe { GetCursorInfo(&mut ci) }.ok()?;
    Some(ci)
}

fn cursor_pos() -> Option<(i32, i32)> {
    cursor_info().map(|c| (c.ptScreenPos.x, c.ptScreenPos.y))
}

/// A cursor picture: the texture's bytes, width, height and hot spot.
type Picture = (Arc<Vec<u8>>, u32, u32, (i32, i32));

/// The cursor's picture, made again only when the `HCURSOR` changes.
#[derive(Default)]
struct CursorCache {
    handle: isize,
    picture: Option<Picture>,
    generation: u64,
}

impl CursorCache {
    fn now(&mut self, map: &DesktopMap) -> Option<CursorImage> {
        let ci = cursor_info()?;
        if ci.flags.0 & CURSOR_SHOWING.0 == 0 || ci.hCursor.is_invalid() {
            return None;
        }
        let h = ci.hCursor.0 as isize;
        if h != self.handle {
            self.handle = h;
            self.picture = cursor_picture(ci.hCursor);
            self.generation += 1;
        }
        let (data, w, hgt, hot) = self.picture.clone()?;
        let (x, y) = map.to_video(ci.ptScreenPos.x - hot.0, ci.ptScreenPos.y - hot.1);
        Some(CursorImage {
            x: x.round() as i32,
            y: y.round() as i32,
            width: w,
            height: hgt,
            data,
            generation: self.generation,
        })
    }
}

/// A bitmap's pixels as 32-bit BGRA, top-down: `(width, height, pixels)`.
fn bitmap_pixels(bm: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    // SAFETY: GDI reads of a bitmap owned by the caller; the buffer is sized for the rows asked.
    unsafe {
        let mut info = BITMAP::default();
        let n = GetObjectW(
            HGDIOBJ(bm.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut info as *mut _ as *mut _),
        );
        if n == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
            return None;
        }
        let (w, h) = (info.bmWidth as u32, info.bmHeight as u32);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let rows = GetDIBits(
            dc,
            bm,
            0,
            h,
            Some(px.as_mut_ptr().cast()),
            &mut bi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        (rows > 0).then_some((w, h, px))
    }
}

/// A cursor as "multiplier + addition" (RGBA16F: rgb added, a multiplies what is under it),
/// with its hot spot: `(data, w, h, hot)` (LH `VidCursorBuild`).
fn cursor_picture(c: HCURSOR) -> Option<Picture> {
    let mut ii = ICONINFO::default();
    // SAFETY: GetIconInfo makes copies of the cursor's bitmaps, deleted below.
    unsafe { GetIconInfo(HICON(c.0), &mut ii) }.ok()?;
    let hot = (ii.xHotspot as i32, ii.yHotspot as i32);
    let mask = bitmap_pixels(ii.hbmMask);
    let color = (!ii.hbmColor.is_invalid())
        .then(|| bitmap_pixels(ii.hbmColor))
        .flatten();
    // SAFETY: the copies GetIconInfo made.
    unsafe {
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
    }
    let (mw, mh, mask) = mask?;
    // Per pixel: (addition r, g, b in 0..1, multiplier).
    let mut px: Vec<[f32; 4]> = Vec::new();
    let (w, h) = match &color {
        Some((cw, ch, col)) => {
            let has_alpha = col.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
            for (i, p) in col.as_chunks::<4>().0.iter().enumerate() {
                let (r, g, b) = (
                    p[2] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[0] as f32 / 255.0,
                );
                px.push(if has_alpha {
                    let a = p[3] as f32 / 255.0;
                    [r * a, g * a, b * a, 1.0 - a]
                } else {
                    let m = mask.get(i * 4).copied().unwrap_or(0) != 0;
                    let black = p[0] == 0 && p[1] == 0 && p[2] == 0;
                    match (m, black) {
                        (false, _) => [r, g, b, 0.0],
                        (true, true) => [0.0, 0.0, 0.0, 1.0],
                        (true, false) => [1.0, 1.0, 1.0, -1.0],
                    }
                });
            }
            (*cw, *ch)
        }
        None => {
            // Monochrome: the mask is AND over XOR, twice the height.
            let h = mh / 2;
            for i in 0..(mw * h) as usize {
                let and = mask[i * 4] != 0;
                let xor = mask[(i + (mw * h) as usize) * 4] != 0;
                px.push(match (and, xor) {
                    (false, x) => {
                        let v = f32::from(u8::from(x));
                        [v, v, v, 0.0]
                    }
                    (true, false) => [0.0, 0.0, 0.0, 1.0],
                    (true, true) => [1.0, 1.0, 1.0, -1.0],
                });
            }
            (mw, h)
        }
    };
    let (cw, ch) = (w.min(CURSOR_MAX), h.min(CURSOR_MAX));
    let mut data = Vec::with_capacity((cw * ch * 8) as usize);
    for y in 0..ch {
        for x in 0..cw {
            let p = px[(y * w + x) as usize];
            for v in p {
                data.extend_from_slice(&f16_bits(v).to_le_bytes());
            }
        }
    }
    Some((Arc::new(data), cw, ch, hot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::FrameGeometry;

    fn map() -> DesktopMap {
        DesktopMap {
            origin: (100, 50),
            geometry: FrameGeometry {
                mode: 0,
                white: 80.0,
                crop: (10, 20, 800, 600),
                out: (800, 600),
            },
        }
    }

    fn press(ms: i64, button: Button, down: bool) -> Press {
        Press {
            ticks: ms * 1000,
            x: 510,
            y: 370,
            button,
            down,
        }
    }

    /// LH §4: a press spreads for 350 ms from radius 6 to 26 (× scale) and fades; a right press
    /// is the double ring; after 350 ms nothing — unless the left button is still held.
    #[test]
    fn rings_spread_and_fade() {
        let f = 1_000_000; // ticks per second: ms × 1000
        let m = map();
        let at =
            |now_ms: i64, presses: &[Press]| rings_at(presses, now_ms * 1000, f, 2.0, None, &m);
        let left = [
            press(0, Button::Left, true),
            press(100, Button::Left, false),
        ];
        let r = at(0, &left);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].x, r[0].y), (400.0, 300.0), "desktop → video");
        assert!((r[0].radius - 12.0).abs() < 1e-3 && (r[0].alpha - 1.0).abs() < 1e-3);
        let r = at(175, &left);
        assert!((r[0].radius - 32.0).abs() < 1e-3 && (r[0].alpha - 0.5).abs() < 1e-3);
        assert!(at(400, &left).is_empty(), "gone after 350 ms");
        let right = [press(0, Button::Right, true)];
        assert_eq!(at(10, &right)[0].kind, 1);
        let held = [press(0, Button::Left, true)];
        let r = at(500, &held);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].kind, r[0].radius, r[0].alpha), (2, 28.0, 0.85));
    }

    /// The hook's queue gets presses in video pixels, and clicks on an excluded window are not
    /// recorded.
    #[test]
    fn presses_go_to_the_queue_in_video_pixels() {
        let s = Shared::default();
        *s.map.lock().unwrap() = Some(map());
        record(&s, POINT { x: 510, y: 370 }, Button::Left, true);
        assert_eq!(s.queue.len(), 1);
        let root =
            unsafe { GetAncestor(WindowFromPoint(POINT { x: 510, y: 370 }), GA_ROOT) }.0 as isize;
        s.exclude.lock().unwrap().push(root);
        record(&s, POINT { x: 510, y: 370 }, Button::Left, false);
        assert_eq!(
            s.queue.len(),
            1,
            "a click on an excluded window is left out"
        );
    }

    /// The arrow the system shows is made into a picture with an opaque body.
    #[test]
    fn the_arrow_cursor_has_a_picture() {
        use windows::Win32::UI::WindowsAndMessaging::{IDC_ARROW, LoadCursorW};
        let c = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap();
        let (data, w, h, _hot) = cursor_picture(c).expect("a picture");
        assert!(w >= 16 && h >= 16 && data.len() == (w * h * 8) as usize);
        // Some pixel replaces what is under it (multiplier 0).
        let opaque = data
            .as_chunks::<8>()
            .0
            .iter()
            .any(|p| p[6] == 0 && p[7] == 0);
        assert!(opaque);
    }
}
