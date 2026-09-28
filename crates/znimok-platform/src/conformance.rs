//! Checks every implementation must pass. The OS crates call these from their tests against the
//! real system (CI runners on both OSes), the synthetic OS runs them too. Each returns what broke,
//! in words, so a failing CI log says which rule.
//!
//! They only assert what is true on any desktop: e.g. frame sizes and formats, not pixel values
//! (a real screen changes between two captures).

use crate::PlatformError;
use crate::frame::PixelFormat;
use crate::keys::KeyCombo;
use crate::traits::*;

type Check = Result<(), String>;

fn ensure(ok: bool, msg: impl FnOnce() -> String) -> Check {
    if ok { Ok(()) } else { Err(msg()) }
}

/// Displays are sane and a full capture of each one has the display's pixel size;
/// a small region inside the primary display is exactly that region.
pub fn capture(c: &dyn Capture) -> Check {
    let ds = c.displays().map_err(|e| format!("displays: {e}"))?;
    ensure(!ds.is_empty(), || "жодного дисплея".into())?;
    ensure(ds.iter().filter(|d| d.primary).count() == 1, || {
        "має бути рівно один основний дисплей".into()
    })?;
    ensure(ds[0].primary, || "основний дисплей має бути першим".into())?;
    for (i, a) in ds.iter().enumerate() {
        ensure(!a.bounds.is_empty(), || format!("{:?}: порожні межі", a.id))?;
        ensure(
            a.work_area.intersect(&a.bounds) == Some(a.work_area),
            || format!("{:?}: робоча область поза межами", a.id),
        )?;
        ensure(a.scale_factor >= 1.0 && a.pixels_per_unit >= 1.0, || {
            format!("{:?}: масштаб < 1", a.id)
        })?;
        ensure(a.color.sdr_white_nits > 0.0, || {
            format!("{:?}: біле SDR ≤ 0", a.id)
        })?;
        for b in &ds[i + 1..] {
            ensure(a.id != b.id, || format!("повторений id {:?}", a.id))?;
            ensure(a.bounds.intersect(&b.bounds).is_none(), || {
                format!("{:?} і {:?} перекриваються", a.id, b.id)
            })?;
        }
    }
    for d in &ds {
        let f = c
            .capture(
                &CaptureTarget::Display { id: d.id.clone() },
                &CaptureOptions::default(),
            )
            .map_err(|e| format!("знімок {:?}: {e}", d.id))?;
        ensure((f.width, f.height) == d.pixel_size(), || {
            format!(
                "{:?}: кадр {}×{}, а дисплей {:?} пікселів",
                d.id,
                f.width,
                f.height,
                d.pixel_size()
            )
        })?;
        ensure(f.source == d.bounds, || {
            format!("{:?}: source {:?} ≠ межі {:?}", d.id, f.source, d.bounds)
        })?;
        ensure(
            !f.color.hdr || matches!(f.format, PixelFormat::Rgba16Float | PixelFormat::Rgb10A2),
            || format!("{:?}: HDR-кадр у 8-бітному форматі {:?}", d.id, f.format),
        )?;
        let sdr = c
            .capture(
                &CaptureTarget::Display { id: d.id.clone() },
                &CaptureOptions {
                    keep_hdr: false,
                    cursor: false,
                },
            )
            .map_err(|e| format!("SDR-знімок {:?}: {e}", d.id))?;
        ensure(sdr.to_rgba8().is_some() || d.color.hdr, || {
            format!("{:?}: SDR-дисплей дав не 8-бітний кадр", d.id)
        })?;
    }
    let p = &ds[0];
    let r = crate::geom::Rect::new(p.bounds.x + 10, p.bounds.y + 10, 64, 32);
    let f = c
        .capture(
            &CaptureTarget::Region { rect: r },
            &CaptureOptions::default(),
        )
        .map_err(|e| format!("ділянка: {e}"))?;
    let want = r.relative_to(p.bounds.origin()).scaled(p.pixels_per_unit);
    ensure((f.width, f.height) == (want.width, want.height), || {
        format!(
            "ділянка {r:?}: кадр {}×{}, очікувано {}×{}",
            f.width, f.height, want.width, want.height
        )
    })?;
    ensure(f.source == r, || format!("ділянка: source {:?}", f.source))
}

/// Window list: unique ids, non-empty bounds, `window_at` of the front window's centre is it.
pub fn windows(w: &dyn WindowList) -> Check {
    let list = w.windows().map_err(|e| format!("windows: {e}"))?;
    let mut ids: Vec<_> = list.iter().map(|x| x.id).collect();
    ids.sort();
    ids.dedup();
    ensure(ids.len() == list.len(), || "повторені id вікон".into())?;
    for x in list.iter().filter(|x| !x.minimized) {
        ensure(!x.bounds.is_empty(), || {
            format!("вікно {:?} «{}»: порожні межі", x.id, x.title)
        })?;
    }
    if let Some(front) = list.iter().find(|x| !x.minimized && !x.own) {
        let c = crate::geom::Point {
            x: front.bounds.x + front.bounds.width as i32 / 2,
            y: front.bounds.y + front.bounds.height as i32 / 2,
        };
        let hit = w.window_at(c).map_err(|e| format!("window_at: {e}"))?;
        ensure(hit.as_ref().map(|h| h.id) == Some(front.id), || {
            format!(
                "window_at(центр «{}») = {:?}",
                front.title,
                hit.map(|h| h.title)
            )
        })?;
    }
    Ok(())
}

/// `free` must be a combination nobody uses on the test machine (e.g. Ctrl+Alt+F13).
/// Register → second id refused → probe says taken → unregister → free again.
pub fn hotkeys(h: &dyn Hotkeys, free: KeyCombo) -> Check {
    let (a, b) = (HotkeyId(0xC0DE), HotkeyId(0xC0DF));
    ensure(
        matches!(h.probe(free), Availability::Free | Availability::Unknown),
        || format!("{free} не вільна до реєстрації"),
    )?;
    h.register(a, free)
        .map_err(|e| format!("register {free}: {e}"))?;
    let second = h.register(b, free);
    let probe = h.probe(free);
    h.unregister(a).map_err(|e| format!("unregister: {e}"))?;
    let _ = h.unregister(b);
    ensure(matches!(second, Err(PlatformError::Busy(_))), || {
        format!("повторна реєстрація {free}: {second:?}")
    })?;
    ensure(
        matches!(probe, Availability::Taken | Availability::Unknown),
        || format!("probe зайнятої {free}: {probe:?}"),
    )?;
    ensure(
        matches!(h.probe(free), Availability::Free | Availability::Unknown),
        || format!("{free} не звільнилась"),
    )
}

/// Text and image survive a write/read round trip (image pixels exactly, straight alpha).
pub fn clipboard(c: &dyn Clipboard) -> Check {
    c.write(&[ClipItem::Text("Znimok ✓ знімок".into())])
        .map_err(|e| format!("write text: {e}"))?;
    let got = c.read().map_err(|e| format!("read: {e}"))?;
    ensure(
        got.iter()
            .any(|i| matches!(i, ClipItem::Text(t) if t == "Znimok ✓ знімок")),
        || format!("текст не повернувся: {got:?}"),
    )?;
    let (w, h) = (7u32, 5u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i * 9) as u8, 200, (255 - i * 7) as u8, 255])
        .collect();
    c.write(&[ClipItem::Image(ClipImage {
        width: w,
        height: h,
        rgba: rgba.clone(),
        png: None,
    })])
    .map_err(|e| format!("write image: {e}"))?;
    let got = c.read().map_err(|e| format!("read: {e}"))?;
    let img = got.iter().find_map(|i| match i {
        ClipItem::Image(im) => Some(im),
        _ => None,
    });
    ensure(
        img.is_some_and(|im| im.width == w && im.height == h && im.rgba == rgba),
        || "зображення не повернулось без змін".into(),
    )
}
