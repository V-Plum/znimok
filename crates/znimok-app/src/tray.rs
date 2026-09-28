//! The tray / menu bar icon's picture and the single-instance rule.
//!
//! Closing the window leaves Znimok running in the tray (the hotkey keeps working), so starting
//! the app again must not create a second hidden process: the new process leaves a "wake" file
//! next to the lock and exits; the running one sees it on its timer and shows its window.
//! The lock is an OS file lock — it goes away with the process, even after a crash.

use std::fs::File;
use std::path::{Path, PathBuf};

use znimok_render::vello_cpu;

/// The "ZK" mark from P3: blue Z, yellow K on a dark rounded square.
pub fn icon(size: u16) -> slint::Image {
    use vello_cpu::color::{AlphaColor, Srgb};
    use vello_cpu::kurbo::{Affine, BezPath, Cap, Join, Point, RoundedRect, Shape, Stroke};
    let mut ctx = vello_cpu::RenderContext::new(size, size);
    ctx.set_transform(Affine::scale(size as f64 / 24.0));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0x1E, 0x22, 0x29, 255));
    ctx.fill_path(&RoundedRect::new(0.5, 0.5, 23.5, 23.5, 5.5).to_path(0.05));
    ctx.set_stroke(
        Stroke::new(2.4)
            .with_caps(Cap::Round)
            .with_join(Join::Round),
    );
    let mut z = BezPath::new();
    z.move_to(Point::new(3.5, 13.5));
    z.line_to(Point::new(11.5, 13.5));
    z.line_to(Point::new(3.5, 21.0));
    z.line_to(Point::new(11.5, 21.0));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0x3D, 0x7B, 0xF5, 255));
    ctx.stroke_path(&z);
    let mut k = BezPath::new();
    k.move_to(Point::new(14.5, 3.0));
    k.line_to(Point::new(14.5, 21.0));
    k.move_to(Point::new(21.0, 4.5));
    k.line_to(Point::new(14.5, 12.0));
    k.line_to(Point::new(21.0, 20.0));
    ctx.set_paint(AlphaColor::<Srgb>::from_rgba8(0xFF, 0xD2, 0x3F, 255));
    ctx.stroke_path(&k);
    ctx.flush();
    let mut pix = vello_cpu::Pixmap::new(size, size);
    let mut res = vello_cpu::Resources::default();
    ctx.render(pix.as_mut(), &mut res);
    let rgba = znimok_render::pixmap_to_rgba(&pix);
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &rgba,
        size as u32,
        size as u32,
    );
    slint::Image::from_rgba8(buf)
}

pub struct Instance {
    _lock: Option<File>,
    wake: PathBuf,
}

pub enum Start {
    /// This is the only instance; keep the value alive for the whole run.
    First(Instance),
    /// Another instance runs and was asked to show its window.
    Woke,
}

/// The lock lives next to the library, so a test run with its own `ZNIMOK_LIBRARY` is separate.
pub fn start(dir: &Path) -> Start {
    let _ = std::fs::create_dir_all(dir);
    let wake = dir.join(".znimok-wake");
    let lock = match File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".znimok-instance.lock"))
    {
        Ok(f) => f,
        // No lock possible (read-only folder): run anyway rather than refuse to start.
        Err(_) => return Start::First(Instance { _lock: None, wake }),
    };
    if lock.try_lock().is_ok() {
        let _ = std::fs::remove_file(&wake);
        Start::First(Instance {
            _lock: Some(lock),
            wake,
        })
    } else {
        let _ = std::fs::write(&wake, std::process::id().to_string());
        Start::Woke
    }
}

impl Instance {
    /// True once per wake request from another start.
    pub fn take_wake(&self) -> bool {
        std::fs::remove_file(&self.wake).is_ok()
    }
}
