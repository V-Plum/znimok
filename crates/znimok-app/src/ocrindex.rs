//! The text on the library's screenshots, for its search (ZK-186): read on the device by the
//! same engine as «text on the picture» (Znimok's Tesseract helper on Windows, Apple Vision on
//! macOS — no cloud, no tokens), one screenshot at a time on a background thread with a pause
//! between them, and kept in the library's local index (never in the `.znimok` files). The text
//! marks' own text goes in too. Turned off, the text is forgotten.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::library::{Index, stamp_of};

/// A run over the screenshots still to read.
pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicUsize>,
    pub total: usize,
}

impl Job {
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Relaxed) >= self.total
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The pause after each screenshot, so the reading never takes the machine.
const PAUSE: Duration = Duration::from_millis(250);

/// Reads `paths` in turn; `step` is called after each (from the worker thread), with `true` the
/// last time.
pub fn start(index: Arc<Index>, paths: Vec<PathBuf>, step: impl Fn(bool) + Send + 'static) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicUsize::new(0));
    let job = Job {
        cancel: cancel.clone(),
        done: done.clone(),
        total: paths.len(),
    };
    std::thread::Builder::new()
        .name("znimok-text-index".into())
        .spawn(move || {
            lower_priority();
            for p in paths {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                // The stamp before reading: a file changed meanwhile is read again next time.
                if let Some(st) = stamp_of(&p) {
                    let text = read(&p).unwrap_or_default();
                    if !cancel.load(Ordering::Relaxed) {
                        index.put_text(&p, st, &text);
                    }
                }
                done.fetch_add(1, Ordering::Relaxed);
                step(false);
                std::thread::sleep(PAUSE);
            }
            done.store(usize::MAX / 2, Ordering::Relaxed);
            step(true);
        })
        .ok();
    job
}

/// A screenshot's text: its text marks, then what the engine reads on its picture.
fn read(path: &std::path::Path) -> Option<String> {
    let (doc, video) = znimok_format::open_parts(path).ok()?;
    if video.is_some() {
        return Some(String::new());
    }
    let mut out: Vec<String> = doc
        .objects
        .iter()
        .filter_map(|o| match &o.data {
            znimok_core::Data::Text { text, .. } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })
        .collect();
    let src = doc.source();
    if let Ok((lines, _)) = crate::text::recognize(src.width, src.height, src.rgba.clone(), (0, 0))
    {
        out.push(crate::text::joined(&lines));
    }
    Some(out.join("\n"))
}

#[cfg(windows)]
fn lower_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: the pseudo-handle of this very thread.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
fn lower_priority() {}
