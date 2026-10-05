//! What ScreenCaptureKit really hands over for a display (ZK-295): for each configuration the
//! size of the buffer, the frame's own account of where the content is (`contentRect`,
//! `contentScale`, `scaleFactor`) and where the picture really is (the box of the pixels that
//! are not black), and whether it is sharp or a 1× picture stretched.
//!
//! Needs «Screen Recording» for the process that runs it (Terminal, when started from one):
//! `cargo run -p znimok-video-mac --example sck_probe -- <report.txt>`.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    probe::run();
}

#[cfg(target_os = "macos")]
mod probe {
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use objc2_core_video::{
        CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
        CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use screencapturekit::cg::{CGPoint, CGRect, CGSize};
    use screencapturekit::cm::SCFrameStatus;
    use screencapturekit::prelude::*;
    use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
    use screencapturekit::shareable_content::SCShareableContentInfo;
    use screencapturekit::stream::configuration::SCCaptureResolutionType;
    use znimok_video_mac::{PixelBuf, RecordRequest, Recording, Target};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsWide(id: u32) -> usize;
        fn CGDisplayPixelsHigh(id: u32) -> usize;
        fn CGDisplayCopyDisplayMode(id: u32) -> *mut std::ffi::c_void;
        fn CGDisplayModeGetWidth(mode: *mut std::ffi::c_void) -> usize;
        fn CGDisplayModeGetHeight(mode: *mut std::ffi::c_void) -> usize;
        fn CGDisplayModeGetPixelWidth(mode: *mut std::ffi::c_void) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: *mut std::ffi::c_void) -> usize;
        fn CGDisplayModeRelease(mode: *mut std::ffi::c_void);
    }

    /// Where the picture is in a BGRA / RGBA image and how sharp it is.
    #[derive(Debug, Default, Clone)]
    struct Seen {
        w: usize,
        h: usize,
        /// The box of the pixels that are not black: x0, y0, x1, y1 (inclusive).
        bbox: Option<(usize, usize, usize, usize)>,
        /// The share of pixels that are not black in each quarter: TL, TR, BL, BR.
        quarters: [f64; 4],
        /// Mean difference of neighbours in pairs (2k, 2k+1) and (2k+1, 2k+2) inside the box:
        /// a 1× picture stretched to 2× has the first far below the second.
        pair_even: f64,
        pair_odd: f64,
    }

    fn look(w: usize, h: usize, stride: usize, px: &[u8]) -> Seen {
        let lit = |x: usize, y: usize| {
            let p = &px[y * stride + x * 4..y * stride + x * 4 + 3];
            p[0] > 10 || p[1] > 10 || p[2] > 10
        };
        let mut s = Seen {
            w,
            h,
            ..Default::default()
        };
        let (mut n, mut total) = ([0u64; 4], [0u64; 4]);
        let step = 3;
        for y in (0..h).step_by(step) {
            for x in (0..w).step_by(step) {
                let q = usize::from(x >= w / 2) + 2 * usize::from(y >= h / 2);
                total[q] += 1;
                if lit(x, y) {
                    n[q] += 1;
                    s.bbox = Some(match s.bbox {
                        None => (x, y, x, y),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                    });
                }
            }
        }
        for q in 0..4 {
            s.quarters[q] = n[q] as f64 / total[q].max(1) as f64;
        }
        if let Some((x0, y0, x1, y1)) = s.bbox {
            let lum = |x: usize, y: usize| {
                let p = &px[y * stride + x * 4..];
                i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2])
            };
            let (mut e, mut o, mut ne, mut no) = (0u64, 0u64, 0u64, 0u64);
            for y in (y0..=y1).step_by(7) {
                let mut x = x0 & !1;
                while x + 2 <= x1 {
                    e += u64::from((lum(x, y) - lum(x + 1, y)).unsigned_abs());
                    o += u64::from((lum(x + 1, y) - lum(x + 2, y)).unsigned_abs());
                    ne += 1;
                    no += 1;
                    x += 2;
                }
            }
            s.pair_even = e as f64 / ne.max(1) as f64;
            s.pair_odd = o as f64 / no.max(1) as f64;
        }
        s
    }

    fn look_buffer(pb: &CVPixelBuffer) -> Seen {
        // SAFETY: a live BGRA pixel buffer, locked for reading while its bytes are read.
        unsafe {
            CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
            let (w, h) = (CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb));
            let stride = CVPixelBufferGetBytesPerRow(pb);
            let base = CVPixelBufferGetBaseAddress(pb).cast::<u8>();
            let s = if base.is_null() {
                Seen {
                    w,
                    h,
                    ..Default::default()
                }
            } else {
                look(w, h, stride, std::slice::from_raw_parts(base, stride * h))
            };
            CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
            s
        }
    }

    fn say_seen(out: &mut String, s: &Seen) {
        let _ = writeln!(out, "    picture: {} x {}", s.w, s.h);
        match s.bbox {
            Some((a, b, c, d)) => {
                let _ = writeln!(
                    out,
                    "    not black: x {a}..{c}, y {b}..{d}  = {:.3} x {:.3} of the frame",
                    (c + 1) as f64 / s.w.max(1) as f64,
                    (d + 1) as f64 / s.h.max(1) as f64
                );
            }
            None => {
                let _ = writeln!(out, "    not black: nothing (all black)");
            }
        }
        let _ = writeln!(
            out,
            "    lit share TL {:.2} TR {:.2} BL {:.2} BR {:.2}; neighbours even {:.2} odd {:.2}",
            s.quarters[0], s.quarters[1], s.quarters[2], s.quarters[3], s.pair_even, s.pair_odd
        );
    }

    struct Got {
        seen: Seen,
        status: Option<SCFrameStatus>,
        content_rect: Option<CGRect>,
        content_scale: Option<f64>,
        scale_factor: Option<f64>,
        frames: usize,
    }

    /// One stream with `cfg`: the first complete frame (and how many came in the wait).
    fn stream_case(
        out: &mut String,
        name: &str,
        filter: &SCContentFilter,
        cfg: SCStreamConfiguration,
    ) {
        let _ = writeln!(out, "\n[{name}]");
        let _ = writeln!(
            out,
            "    asked: {} x {}, scalesToFit {}, resolution {:?}",
            cfg.width(),
            cfg.height(),
            cfg.scales_to_fit(),
            cfg.capture_resolution_type().ok()
        );
        let (tx, rx) = mpsc::channel::<Got>();
        let count = Arc::new(Mutex::new(0usize));
        let stream = match SCStream::new(filter, &cfg) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(out, "    SCStream: {e}");
                return;
            }
        };
        let mut stream = stream;
        let c2 = count.clone();
        let sent = Arc::new(Mutex::new(false));
        let handler = move |sample: CMSampleBuffer, _t: SCStreamOutputType| {
            let status = sample.frame_status();
            let ptr = sample.image_buffer_ptr_borrowed();
            // SAFETY: a CVPixelBufferRef borrowed from the live sample; retained while read.
            let Some(pb) = (unsafe { PixelBuf::retain_raw(ptr) }) else {
                return;
            };
            let n = {
                let mut c = c2.lock().unwrap();
                *c += 1;
                *c
            };
            // The third picture: the first ones may come before the screen is drawn whole.
            if n < 3 || std::mem::replace(&mut *sent.lock().unwrap(), true) {
                return;
            }
            let _ = tx.send(Got {
                seen: look_buffer(&pb.0),
                status,
                content_rect: sample.content_rect(),
                content_scale: sample.content_scale(),
                scale_factor: sample.scale_factor(),
                frames: n,
            });
        };
        if let Err(e) = stream.add_output_handler(handler, SCStreamOutputType::Screen) {
            let _ = writeln!(out, "    add_output_handler: {e}");
            return;
        }
        if let Err(e) = stream.start_capture() {
            let _ = writeln!(out, "    start_capture: {e}");
            return;
        }
        let got = rx.recv_timeout(Duration::from_secs(5));
        let _ = stream.stop_capture();
        match got {
            Ok(g) => {
                let rect = |r: Option<CGRect>| {
                    r.map(|r| {
                        format!(
                            "{:.1},{:.1} {:.1}x{:.1}",
                            r.origin.x, r.origin.y, r.size.width, r.size.height
                        )
                    })
                };
                let _ = writeln!(
                    out,
                    "    frame info: status {:?}, contentRect {:?}, contentScale {:?}, scaleFactor {:?} (frame #{})",
                    g.status,
                    rect(g.content_rect),
                    g.content_scale,
                    g.scale_factor,
                    g.frames
                );
                say_seen(out, &g.seen);
            }
            Err(_) => {
                let _ = writeln!(
                    out,
                    "    no picture in 5 s (frames with a buffer: {})",
                    *count.lock().unwrap()
                );
            }
        }
    }

    fn shot_case(
        out: &mut String,
        name: &str,
        filter: &SCContentFilter,
        cfg: SCStreamConfiguration,
    ) {
        let _ = writeln!(out, "\n[{name}]");
        let _ = writeln!(
            out,
            "    asked: {} x {}, scalesToFit {}",
            cfg.width(),
            cfg.height(),
            cfg.scales_to_fit()
        );
        match SCScreenshotManager::capture_image(filter, &cfg) {
            Ok(img) => match img.rgba_data() {
                Ok(d) => {
                    let (w, h) = (img.width(), img.height());
                    say_seen(out, &look(w, h, w * 4, &d));
                }
                Err(e) => {
                    let _ = writeln!(out, "    rgba_data: {e}");
                }
            },
            Err(e) => {
                let _ = writeln!(out, "    capture_image: {e}");
            }
        }
    }

    fn even(v: f64) -> u32 {
        ((v / 2.0).round() as u32 * 2).max(64)
    }

    pub fn run() {
        let path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "sck-probe.txt".into());
        let mut out = String::new();
        let _ = writeln!(out, "Znimok sck_probe (ZK-295)");
        let content = match SCShareableContent::get() {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(out, "SCShareableContent: {e}");
                let _ = writeln!(
                    out,
                    "(no «Screen Recording» permission for the program that started this?)"
                );
                finish(&path, &out);
                return;
            }
        };
        // SAFETY: plain CoreGraphics calls.
        let main_id = unsafe { CGMainDisplayID() };
        for d in content.displays() {
            let id = d.display_id();
            let f = d.frame();
            // SAFETY: plain CoreGraphics calls; the mode is released.
            let (pw, ph, mw, mh, mpw, mph) = unsafe {
                let m = CGDisplayCopyDisplayMode(id);
                let r = if m.is_null() {
                    (0, 0, 0, 0)
                } else {
                    let r = (
                        CGDisplayModeGetWidth(m),
                        CGDisplayModeGetHeight(m),
                        CGDisplayModeGetPixelWidth(m),
                        CGDisplayModeGetPixelHeight(m),
                    );
                    CGDisplayModeRelease(m);
                    r
                };
                (
                    CGDisplayPixelsWide(id),
                    CGDisplayPixelsHigh(id),
                    r.0,
                    r.1,
                    r.2,
                    r.3,
                )
            };
            let _ = writeln!(
                out,
                "\n=== display {id}{}: SCDisplay frame {:.0},{:.0} {:.0}x{:.0} (width {} height {}); CGDisplayPixels {pw}x{ph}; mode {mw}x{mh} points, {mpw}x{mph} pixels",
                if id == main_id { " (main)" } else { "" },
                f.origin.x,
                f.origin.y,
                f.size.width,
                f.size.height,
                d.width(),
                d.height()
            );
            let Ok(filter) = SCContentFilter::create().with_display(&d).build() else {
                let _ = writeln!(out, "    SCContentFilter failed");
                continue;
            };
            let info = SCShareableContentInfo::for_filter(&filter);
            let (nw, nh, scale) = match &info {
                Some(i) => {
                    let (w, h) = i.pixel_size();
                    let r = i.content_rect();
                    let _ = writeln!(
                        out,
                        "    filter: pixel_size {w}x{h}, pointPixelScale {}, contentRect {:.1},{:.1} {:.1}x{:.1}",
                        i.point_pixel_scale(),
                        r.origin.x,
                        r.origin.y,
                        r.size.width,
                        r.size.height
                    );
                    (f64::from(w), f64::from(h), f64::from(i.point_pixel_scale()))
                }
                None => {
                    let _ = writeln!(out, "    filter: no SCShareableContentInfo");
                    (f.size.width * 2.0, f.size.height * 2.0, 2.0)
                }
            };
            let (pts_w, pts_h) = (f.size.width, f.size.height);
            let exact = (even(nw), even(nh));
            // Smaller than the native size, as the recording asks when the display is over the
            // encoder's limit (and here always, to see it on any display).
            let less = (even(nw * 0.9), even(nh * 0.9));
            let whole = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: pts_w,
                    height: pts_h,
                },
            };
            let base = |w: u32, h: u32| {
                SCStreamConfiguration::new()
                    .with_width(w)
                    .with_height(h)
                    .with_pixel_format(PixelFormat::BGRA)
                    .with_fps(30)
                    .with_queue_depth(5)
                    .with_shows_cursor(true)
            };
            let best = |c: SCStreamConfiguration| {
                c.clone()
                    .with_capture_resolution_type(SCCaptureResolutionType::Best)
                    .unwrap_or(c)
            };
            let kind = |c: SCStreamConfiguration, k: SCCaptureResolutionType| {
                c.clone().with_capture_resolution_type(k).unwrap_or(c)
            };
            let px = |w: u32, h: u32| CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: f64::from(w),
                    height: f64::from(h),
                },
            };

            stream_case(
                &mut out,
                "A exact, defaults",
                &filter,
                base(exact.0, exact.1),
            );
            stream_case(
                &mut out,
                "B exact, scalesToFit + Best",
                &filter,
                best(base(exact.0, exact.1).with_scales_to_fit(true)),
            );
            stream_case(
                &mut out,
                "C exact, sourceRect whole",
                &filter,
                base(exact.0, exact.1).with_source_rect(whole),
            );
            stream_case(&mut out, "D less, defaults", &filter, base(less.0, less.1));
            stream_case(
                &mut out,
                "E less, scalesToFit",
                &filter,
                base(less.0, less.1).with_scales_to_fit(true),
            );
            stream_case(
                &mut out,
                "F less, scalesToFit + Best (as 0.0.21 records)",
                &filter,
                best(base(less.0, less.1).with_scales_to_fit(true)),
            );
            stream_case(
                &mut out,
                "G less, scalesToFit + Best + sourceRect whole",
                &filter,
                best(base(less.0, less.1).with_scales_to_fit(true)).with_source_rect(whole),
            );
            stream_case(
                &mut out,
                "H less, scalesToFit + Best + sourceRect + destinationRect in pixels",
                &filter,
                best(base(less.0, less.1).with_scales_to_fit(true))
                    .with_source_rect(whole)
                    .with_destination_rect(px(less.0, less.1)),
            );
            stream_case(
                &mut out,
                "I less, scalesToFit + Best + destinationRect in points",
                &filter,
                best(base(less.0, less.1).with_scales_to_fit(true)).with_destination_rect(px(
                    (f64::from(less.0) / scale) as u32,
                    (f64::from(less.1) / scale) as u32,
                )),
            );
            stream_case(
                &mut out,
                "J less, scalesToFit + Nominal",
                &filter,
                kind(
                    base(less.0, less.1).with_scales_to_fit(true),
                    SCCaptureResolutionType::Nominal,
                ),
            );
            stream_case(
                &mut out,
                "K points size (1x), scalesToFit",
                &filter,
                base(even(pts_w), even(pts_h)).with_scales_to_fit(true),
            );
            stream_case(
                &mut out,
                "L exact, Nominal",
                &filter,
                kind(base(exact.0, exact.1), SCCaptureResolutionType::Nominal),
            );
            // A region, as the overlay asks for one: 800 x 600 points from 100, 100.
            let region = CGRect {
                origin: CGPoint { x: 100.0, y: 100.0 },
                size: CGSize {
                    width: 800.0,
                    height: 600.0,
                },
            };
            stream_case(
                &mut out,
                "M region 800x600 pt, exact pixels, scalesToFit + Best",
                &filter,
                best(base(even(800.0 * scale), even(600.0 * scale)).with_scales_to_fit(true))
                    .with_source_rect(region),
            );
            stream_case(
                &mut out,
                "N region 800x600 pt, less pixels, scalesToFit + Best",
                &filter,
                best(base(even(720.0 * scale), even(540.0 * scale)).with_scales_to_fit(true))
                    .with_source_rect(region),
            );
            shot_case(
                &mut out,
                "S1 screenshot exact, scalesToFit + Best",
                &filter,
                best(
                    SCStreamConfiguration::new()
                        .with_width(exact.0)
                        .with_height(exact.1)
                        .with_scales_to_fit(true),
                ),
            );
            shot_case(
                &mut out,
                "S2 screenshot less, scalesToFit + Best",
                &filter,
                best(
                    SCStreamConfiguration::new()
                        .with_width(less.0)
                        .with_height(less.1)
                        .with_scales_to_fit(true),
                ),
            );

            // The recording itself, as the app makes it: two seconds, then its first frame.
            if id == main_id {
                let _ = writeln!(out, "\n[R the recording as the app makes it]");
                let mp4 = std::path::Path::new(&path).with_extension("mp4");
                let _ = std::fs::remove_file(&mp4);
                let req = RecordRequest::new(Target::Display { id, region: None }, mp4.clone());
                match Recording::start(req) {
                    Ok(rec) => {
                        let _ = writeln!(out, "    started: {:?}", rec.started());
                        std::thread::sleep(Duration::from_secs(2));
                        let fin = rec.stop();
                        let _ = writeln!(
                            out,
                            "    result: frames {:?}, error {:?}, file {:?}",
                            fin.result.frames,
                            fin.result.error.as_ref().map(|e| e.to_string()),
                            fin.path
                        );
                        match znimok_video_mac::poster::first_frame(&mp4) {
                            Ok((w, h, rgba)) => say_seen(
                                &mut out,
                                &look(w as usize, h as usize, w as usize * 4, &rgba),
                            ),
                            Err(e) => {
                                let _ = writeln!(out, "    first_frame: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(out, "    start: {e}");
                    }
                }
            }
        }
        finish(&path, &out);
    }

    fn finish(path: &str, out: &str) {
        print!("{out}");
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("{path}: {e}");
        }
    }
}
