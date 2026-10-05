//! The system content picker (ZK-129, from the P3 prototype): the person picks a window or a
//! display in macOS's own window, and Znimok captures just that.
//!
//! With the Screen Recording permission the pick goes through `SCScreenshotManager`. Without it
//! macOS refuses that even for the picker's filter ("The user declined TCCs…", P3 live test
//! 28.09) and exempts only streams, so one frame of a short `SCStream` is taken — macOS draws its
//! purple "sharing" badge on it. That is why this is the fallback, not the main way.
//!
//! ⚠ The picker's callback arrives on the main thread, and ScreenCaptureKit calls back on the
//! main queue too: capturing inside the callback deadlocks, so the capture runs on its own thread.
//! Only single-window and single-display modes are offered: with the application mode macOS adds
//! a "Share this window / all application windows" step.

use std::sync::Mutex;
use std::time::Duration;

use screencapturekit::content_sharing_picker::{
    SCContentSharingPicker, SCContentSharingPickerConfiguration, SCContentSharingPickerMode,
    SCPickerOutcome,
};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// What the picker gave: straight RGBA, and whether it came without the permission (then the
/// sharing badge may be on it).
pub struct Picked {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub without_permission: bool,
}

/// Opens the picker; `done` gets the capture, `Ok(None)` when the person cancelled, or an error.
/// `done` runs on a worker thread.
pub fn pick_and_capture(done: impl FnOnce(Result<Option<Picked>, String>) + Send + 'static) {
    let mut cfg = match SCContentSharingPickerConfiguration::new() {
        Ok(c) => c,
        Err(e) => return done(Err(format!("picker config: {e}"))),
    };
    cfg.set_allowed_picker_modes(&[
        SCContentSharingPickerMode::SingleWindow,
        SCContentSharingPickerMode::SingleDisplay,
    ]);
    let done = Mutex::new(Some(done));
    SCContentSharingPicker::show(&cfg, move |outcome| {
        let Some(done) = done.lock().ok().and_then(|mut d| d.take()) else {
            return;
        };
        match outcome {
            SCPickerOutcome::Picked(result) => {
                let filter = result.filter();
                let (pw, ph) = result.pixel_size();
                std::thread::spawn(move || {
                    // SAFETY: a plain CoreGraphics query.
                    let granted = unsafe { CGPreflightScreenCaptureAccess() };
                    let r = if granted {
                        let cfg = SCStreamConfiguration::new()
                            .with_width(pw.max(1))
                            .with_height(ph.max(1))
                            .with_shows_cursor(false)
                            .with_scales_to_fit(true);
                        // The full pixel resolution: macOS 27 defaults to 1× (ZK-292).
                        let cfg = cfg
                            .clone()
                            .with_capture_resolution_type(screencapturekit::stream::configuration::SCCaptureResolutionType::Best)
                            .unwrap_or(cfg);
                        SCScreenshotManager::capture_image(&filter, &cfg)
                            .map_err(|e| format!("capture_image: {e}"))
                            .and_then(|img| {
                                let (width, height) = (img.width() as u32, img.height() as u32);
                                img.rgba_data()
                                    .map_err(|e| format!("CGImage: {e}"))
                                    .map(|rgba| Picked {
                                        width,
                                        height,
                                        rgba,
                                        without_permission: false,
                                    })
                            })
                    } else {
                        one_frame(&filter, pw, ph).map(|(width, height, rgba)| Picked {
                            width,
                            height,
                            rgba,
                            without_permission: true,
                        })
                    };
                    done(r.map(Some));
                });
            }
            _ => done(Ok(None)),
        }
    });
}

/// One frame of a short-lived `SCStream` (BGRA → tight RGBA).
fn one_frame(filter: &SCContentFilter, pw: u32, ph: u32) -> Result<(u32, u32, Vec<u8>), String> {
    use screencapturekit::cv::CVPixelBufferLockFlags;
    use std::sync::mpsc;

    type Frame = (u32, u32, Vec<u8>);
    struct First(Mutex<Option<mpsc::Sender<Frame>>>);
    impl SCStreamOutputTrait for First {
        fn did_output_sample_buffer(&self, sample: CMSampleBuffer, kind: SCStreamOutputType) {
            if !matches!(kind, SCStreamOutputType::Screen) {
                return;
            }
            let Some(pb) = sample.pixel_buffer() else {
                return;
            };
            let Ok(guard) = pb.lock(CVPixelBufferLockFlags::READ_ONLY) else {
                return;
            };
            let (w, h, stride) = (guard.width(), guard.height(), guard.bytes_per_row());
            // SAFETY: the buffer stays locked for as long as `guard` lives.
            let Some(bytes) = (unsafe { guard.as_slice() }) else {
                return;
            };
            let mut rgba = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                for px in bytes[y * stride..y * stride + w * 4].as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
            if let Ok(mut tx) = self.0.lock()
                && let Some(tx) = tx.take()
            {
                let _ = tx.send((w as u32, h as u32, rgba));
            }
        }
    }

    let cfg = SCStreamConfiguration::new()
        .with_width(pw.max(1))
        .with_height(ph.max(1))
        .with_pixel_format(PixelFormat::BGRA)
        .with_shows_cursor(false);
    let (tx, rx) = mpsc::channel();
    let mut stream = SCStream::new(filter, &cfg).map_err(|e| format!("SCStream: {e}"))?;
    stream
        .add_output_handler(First(Mutex::new(Some(tx))), SCStreamOutputType::Screen)
        .map_err(|e| format!("output: {e}"))?;
    stream.start_capture().map_err(|e| format!("start: {e}"))?;
    let frame = rx.recv_timeout(Duration::from_secs(5));
    let _ = stream.stop_capture();
    frame.map_err(|_| "no frame within 5 s".to_string())
}
