//! The page area of a window (ZK-310): where a browser's page (or a document's content) stands
//! inside the window, without the title bar, the tabs, the address bar and the side panels —
//! so that `capture` with `content_only` gives the page alone. UI Automation names it: the
//! window's largest descendant of control type Document (a browser's web area; an editor's
//! text). Nothing of the kind — `None`: the caller captures the whole window.
//!
//! UI Automation is COM; the thread is joined to an apartment if it is not in one yet. A browser
//! turns its accessibility tree on when a client first asks — the first call takes a moment.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_ControlTypePropertyId,
    UIA_DocumentControlTypeId,
};
use znimok_platform::{Rect, WindowId};

use super::winlist::{dwm_bounds, hwnd};

/// The page area of window `id` in desktop units, or `None` when the window has none.
pub fn content_rect(id: WindowId) -> Option<Rect> {
    let h = hwnd(id);
    let frame = dwm_bounds(h)?;
    // A browser builds its accessibility tree when a client first asks: the first look may
    // find nothing yet — a few more, a moment apart.
    let mut found = None;
    for _ in 0..6 {
        found = largest_document(h);
        if found.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let found = found?;
    // Inside the window's visible frame: a page partly off-screen or under a side panel is
    // still the page, but never outside the window.
    let x0 = found.x.max(frame.x);
    let y0 = found.y.max(frame.y);
    let x1 = (found.x + found.width as i32).min(frame.x + frame.width as i32);
    let y1 = (found.y + found.height as i32).min(frame.y + frame.height as i32);
    if x1 - x0 < 16 || y1 - y0 < 16 {
        return None;
    }
    Some(Rect {
        x: x0,
        y: y0,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

/// The largest Document element under the window (a browser has several: the page, an
/// extension's panel, DevTools).
fn largest_document(h: HWND) -> Option<Rect> {
    // SAFETY: COM calls with the handles they return; the apartment join may say «already in
    // one» (S_FALSE) or «in another kind» (RPC_E_CHANGED_MODE) — both leave COM usable here.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let ui: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let root = ui.ElementFromHandle(h).ok()?;
        let mut v = VARIANT::default();
        {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = VT_I4;
            inner.Anonymous.lVal = UIA_DocumentControlTypeId.0;
        }
        let cond = ui
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &v)
            .ok()?;
        let all = root.FindAll(TreeScope_Descendants, &cond).ok()?;
        let n = all.Length().unwrap_or(0);
        if std::env::var_os("ZNIMOK_CONTENT_DEBUG").is_some() {
            eprintln!("content_rect: {n} document elements under the window");
        }
        let mut best: Option<Rect> = None;
        for i in 0..n {
            let Ok(e) = all.GetElement(i) else { continue };
            let Ok(r) = e.CurrentBoundingRectangle() else {
                continue;
            };
            let (w, hgt) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || hgt <= 0 {
                continue;
            }
            let area = w as i64 * hgt as i64;
            if best.is_none_or(|b| area > b.width as i64 * b.height as i64) {
                best = Some(Rect {
                    x: r.left,
                    y: r.top,
                    width: w as u32,
                    height: hgt as u32,
                });
            }
        }
        best
    }
}
