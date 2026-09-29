//! Recording parameters that do not depend on the OS (§1 frame rate and quality, §2.1 encoder,
//! §2.6 geometry, §6.3 export bitrate; §7 items 2, 3, 7, 15, 40).

use crate::traits::EncoderConfig;

/// Frame rate from the setting: 45 and above → 60, anything else → 30 (`VidLoadSettings`).
pub fn fps_from_setting(v: i32) -> u32 {
    if v >= 45 { 60 } else { 30 }
}

/// Quality setting: 0 smaller, 1 normal, 2 high.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Smaller,
    #[default]
    Normal,
    High,
}

impl Quality {
    /// Out-of-range values clamp (≤0 → smaller, ≥2 → high), as `VidBitsPerPixel` does.
    pub fn from_setting(q: i32) -> Self {
        match q {
            i32::MIN..=0 => Self::Smaller,
            1 => Self::Normal,
            _ => Self::High,
        }
    }

    /// Bits per pixel per frame: screen content is mostly still, so even "normal" keeps text
    /// sharp; "high" is for small fonts and motion.
    pub fn bits_per_pixel(self) -> f32 {
        match self {
            Self::Smaller => 0.06,
            Self::Normal => 0.10,
            Self::High => 0.16,
        }
    }
}

/// Bitrate bounds, bit/s: [1; 100] Mbit/s.
pub const MIN_BITRATE: f64 = 1e6;
pub const MAX_BITRATE: f64 = 1e8;
/// AAC-LC 160 kbit/s (`AVG_BYTES_PER_SECOND = 20000`).
pub const AUDIO_BITRATE: u32 = 160_000;

/// Recording bitrate: `w·h·fps·bpp`, clamped to [1; 100] Mbit/s.
pub fn record_bitrate(w: u32, h: u32, fps: u32, bpp: f32) -> u32 {
    (w as f64 * h as f64 * fps as f64 * bpp as f64).clamp(MIN_BITRATE, MAX_BITRATE) as u32
}

/// Export bitrate (§6.3, §7 item 15): the source's ×1.25 — a second compression without
/// headroom visibly blurs small text; unknown source bitrate → `w·h·fps·0.15`. Scaled by area
/// when the output is smaller, clamped to [1; 100] Mbit/s.
pub fn export_bitrate(
    source_bitrate: Option<u32>,
    src: (u32, u32),
    fps: f64,
    out: Option<(u32, u32)>,
) -> u32 {
    let (w, h) = (src.0 as f64, src.1 as f64);
    let mut b = match source_bitrate {
        Some(br) if br > 0 => br as f64 * 1.25,
        _ => w * h * fps * 0.15,
    };
    if let Some((ow, oh)) = out
        && w * h > 0.0
    {
        b *= ow as f64 * oh as f64 / (w * h);
    }
    b.clamp(MIN_BITRATE, MAX_BITRATE) as u32
}

/// Key frame interval as LH wrote it: one per second (`GOP = fps`, §7 item 3) — seeking and
/// trimming rely on it.
pub fn keyframe_interval_lh(fps: u32) -> u32 {
    fps
}

/// Key frame interval for Znimok recordings: every ~0.25 s (decision ZK-17 of 28.09.2026 — seek
/// on 4K with GOP = fps took up to 261 ms, with 15 at 60 fps up to 61 ms). The final value is
/// set by ZK-87 after measuring its bitrate cost.
pub fn keyframe_interval(fps: u32) -> u32 {
    (fps / 4).max(1)
}

/// Encoder settings for a recording.
pub fn encoder_config(
    w: u32,
    h: u32,
    fps: u32,
    quality: Quality,
    audio_tracks: usize,
) -> EncoderConfig {
    EncoderConfig {
        width: w,
        height: h,
        fps,
        bitrate: record_bitrate(w, h, fps, quality.bits_per_pixel()),
        keyframe_interval: keyframe_interval(fps),
        b_frames: 0,
        audio_tracks,
        audio_bitrate: AUDIO_BITRATE,
    }
}

/// Smallest side hardware encoders take.
pub const MIN_SIDE: i32 = 64;

/// A rectangle in physical pixels, `[left, right) × [top, bottom)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    fn intersect(&self, o: &Self) -> Self {
        let r = Self::new(
            self.left.max(o.left),
            self.top.max(o.top),
            self.right.min(o.right),
            self.bottom.min(o.bottom),
        );
        if r.right <= r.left || r.bottom <= r.top {
            Self::default()
        } else {
            r
        }
    }
}

/// Fit a recording region (`VidFitRect`, §2.6, §7 item 7): intersect with the monitor, grow to
/// at least 64 px around its centre (staying on the monitor), then make both sides EVEN (4:2:0)
/// by trimming the right/bottom edge. A multiple of 16 is NOT needed (1160 and 1234 px were
/// verified by decoding).
pub fn fit_region(sel: PixelRect, monitor: PixelRect) -> PixelRect {
    let mut r = sel.intersect(&monitor);
    fn grow(a: &mut i32, b: &mut i32, lo: i32, hi: i32, want: i32) {
        let want = want.min(hi - lo);
        if *b - *a >= want {
            return;
        }
        let c = (*a + *b) / 2;
        *a = c - want / 2;
        *b = *a + want;
        if *a < lo {
            *b += lo - *a;
            *a = lo;
        }
        if *b > hi {
            *a -= *b - hi;
            *b = hi;
        }
    }
    grow(
        &mut r.left,
        &mut r.right,
        monitor.left,
        monitor.right,
        MIN_SIDE,
    );
    grow(
        &mut r.top,
        &mut r.bottom,
        monitor.top,
        monitor.bottom,
        MIN_SIDE,
    );
    if r.width() & 1 != 0 {
        r.right -= 1;
    }
    if r.height() & 1 != 0 {
        r.bottom -= 1;
    }
    r
}

/// Plan a region recording: whether it is the whole monitor is decided BEFORE the even-size fit
/// (§7 item 40: a 3840×2089 monitor becomes a 3840×2088 video but is still "the whole screen" —
/// no frame indicator, no region border).
pub fn plan_region(sel: PixelRect, monitor: PixelRect) -> (PixelRect, bool) {
    let whole = sel.intersect(&monitor) == monitor;
    (fit_region(sel, monitor), whole)
}

/// Video size for a followed window: the window's size at start with the odd pixel dropped
/// (`tw & ~1 × th & ~1`, §2.6).
pub fn even_size(w: u32, h: u32) -> (u32, u32) {
    (w & !1, h & !1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1: 30 / 60 from the setting (≥45 → 60).
    #[test]
    fn fps_setting() {
        assert_eq!(fps_from_setting(15), 30);
        assert_eq!(fps_from_setting(44), 30);
        assert_eq!(fps_from_setting(45), 60);
        assert_eq!(fps_from_setting(60), 60);
    }

    /// §1: quality → 0.06 / 0.10 / 0.16 bpp; bitrate w·h·fps·bpp in [1; 100] Mbit/s.
    #[test]
    fn quality_and_bitrate() {
        assert_eq!(Quality::from_setting(-3), Quality::Smaller);
        assert_eq!(Quality::from_setting(1), Quality::Normal);
        assert_eq!(Quality::from_setting(7), Quality::High);
        assert_eq!(record_bitrate(1920, 1080, 30, 0.10), 6_220_800);
        assert_eq!(
            record_bitrate(64, 64, 30, 0.06),
            1_000_000,
            "floor 1 Mbit/s"
        );
        assert_eq!(record_bitrate(7680, 4320, 60, 0.16), 100_000_000, "ceiling");
    }

    /// §7 item 15: export ×1.25 of the source, area-scaled when smaller, fallback 0.15 bpp.
    #[test]
    fn export_bitrate_headroom() {
        assert_eq!(
            export_bitrate(Some(8_000_000), (1920, 1080), 30.0, None),
            10_000_000
        );
        assert_eq!(
            export_bitrate(Some(8_000_000), (1920, 1080), 30.0, Some((960, 540))),
            2_500_000
        );
        assert_eq!(export_bitrate(None, (1920, 1080), 30.0, None), 9_331_200);
        assert_eq!(
            export_bitrate(Some(1000), (100, 100), 30.0, None),
            1_000_000
        );
    }

    /// §7 items 2 and 3: no B-frames ever; key frames — LH one per second, Znimok ~0.25 s
    /// (decision ZK-17).
    #[test]
    fn encoder_defaults() {
        let c = encoder_config(1280, 720, 60, Quality::Normal, 2);
        assert_eq!(c.b_frames, 0);
        assert_eq!(c.keyframe_interval, 15);
        assert_eq!(keyframe_interval(30), 7);
        assert_eq!(keyframe_interval_lh(30), 30);
        assert_eq!(c.audio_tracks, 2);
        assert_eq!(c.audio_bitrate, 160_000);
    }

    /// §7 item 7: even sides, at least 64, no multiple of 16; grows around the centre inside the
    /// monitor.
    #[test]
    fn region_fit() {
        let mon = PixelRect::new(0, 0, 1920, 1080);
        assert_eq!(
            fit_region(PixelRect::new(100, 100, 1261, 1335), mon),
            PixelRect::new(100, 100, 1260, 1080)
        );
        assert_eq!(
            fit_region(PixelRect::new(500, 500, 510, 505), mon),
            PixelRect::new(473, 470, 537, 534)
        );
        assert_eq!(
            fit_region(PixelRect::new(0, 0, 10, 10), mon),
            PixelRect::new(0, 0, 64, 64),
            "pushed back onto the monitor"
        );
        assert_eq!(
            fit_region(PixelRect::new(1915, 1075, 1925, 1085), mon),
            PixelRect::new(1856, 1016, 1920, 1080)
        );
        let r = fit_region(PixelRect::new(0, 0, 1160, 1234 / 2 * 2 - 100), mon);
        assert_eq!(r.width(), 1160, "1160 is not a multiple of 16 and stays");
    }

    /// §7 item 40: "whole screen" is decided before the even-size fit.
    #[test]
    fn whole_screen_before_parity() {
        let mon = PixelRect::new(0, 0, 3840, 2089);
        let (r, whole) = plan_region(mon, mon);
        assert!(whole);
        assert_eq!((r.width(), r.height()), (3840, 2088));
        let (_, whole) = plan_region(PixelRect::new(0, 0, 3840, 2088), mon);
        assert!(!whole);
    }

    /// §2.6: followed window — size at start with the odd pixel dropped.
    #[test]
    fn window_even_size() {
        assert_eq!(even_size(1919, 1051), (1918, 1050));
    }
}
