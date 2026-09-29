//! Text recognition through `znimok-ocr.exe` (ZK-120): Tesseract with Ukrainian and English,
//! built by `tools/ocr-helper`, shipped next to the app. Windows has no Ukrainian of its own.
//!
//! The helper is a short-lived child: started on the first request, kept while requests come,
//! gone a few seconds after the last one (it exits by itself), so no memory is held between uses
//! (owner, 29.09: RAM matters more than disk). It runs in a job object with a memory cap and is
//! killed with it; a crash costs one request, not the app.
//!
//! Two readings: **text** (`ukr+eng`, what people see and copy) and **masking** (`eng+ukr`: Latin
//! reads better — e-mail, keys, cards; secrets are looked for in both).

use super::{Line, Ocr, OcrError, OcrResult, Rect, Word, is_russian, match_languages};
use crate::Rgba;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The languages the helper's models cover.
pub const LANGUAGES: [&str; 2] = ["uk", "en"];
/// Largest side the helper accepts (it refuses bigger pictures).
pub const MAX_SIDE: u32 = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Text = 0,
    Masking = 1,
}

/// A request: header + 8-bit grey pixels (luma of the RGBA picture).
pub fn encode_request(img: &Rgba, mode: Mode) -> Vec<u8> {
    let n = img.width as usize * img.height as usize;
    let mut out = Vec::with_capacity(20 + n);
    out.extend_from_slice(b"ZOCR");
    for v in [1u32, mode as u32, img.width, img.height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend(img.pixels.as_chunks::<4>().0.iter().map(|p| {
        let (r, g, b) = (p[0] as u32, p[1] as u32, p[2] as u32);
        ((r * 299 + g * 587 + b * 114 + 500) / 1000) as u8
    }));
    out
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    lines: Vec<ReplyLine>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ReplyLine {
    text: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    conf: f32,
    #[serde(default)]
    words: Vec<ReplyWord>,
}

#[derive(Deserialize)]
struct ReplyWord {
    text: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// The helper's JSON reply → lines with words (confidence 0–1).
pub fn parse_reply(json: &[u8], mode: Mode) -> Result<OcrResult, OcrError> {
    let r: Reply =
        serde_json::from_slice(json).map_err(|e| OcrError::Os(format!("helper reply: {e}")))?;
    if let Some(e) = r.error {
        return Err(OcrError::Os(format!("helper: {e}")));
    }
    let lines = r
        .lines
        .into_iter()
        .map(|l| Line {
            text: l.text,
            rect: Rect {
                x: l.x,
                y: l.y,
                w: l.w,
                h: l.h,
            },
            confidence: Some((l.conf / 100.0).clamp(0.0, 1.0)),
            words: l
                .words
                .into_iter()
                .map(|w| Word {
                    text: w.text,
                    rect: Rect {
                        x: w.x,
                        y: w.y,
                        w: w.w,
                        h: w.h,
                    },
                })
                .collect(),
        })
        .collect();
    let languages = match mode {
        Mode::Text => vec!["uk".into(), "en".into()],
        Mode::Masking => vec!["en".into(), "uk".into()],
    };
    Ok(OcrResult {
        lines,
        languages,
        missing: Vec::new(),
    })
}

/// `znimok-ocr.exe` and its `tessdata` next to the running app, or `ZNIMOK_OCR_HELPER`.
pub fn find() -> Option<PathBuf> {
    let exe = std::env::var_os("ZNIMOK_OCR_HELPER")
        .map(PathBuf::from)
        .or_else(|| {
            let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
            Some(dir.join(if cfg!(windows) {
                "znimok-ocr.exe"
            } else {
                "znimok-ocr"
            }))
        })?;
    usable(&exe).then_some(exe)
}

fn usable(exe: &Path) -> bool {
    let data = exe.with_file_name("tessdata");
    exe.is_file()
        && data.join("ukr.traineddata").is_file()
        && data.join("eng.traineddata").is_file()
}

/// Recogniser backed by the helper process.
pub struct TessHelper {
    exe: PathBuf,
    live: std::sync::Mutex<Option<proc::Live>>,
}

impl TessHelper {
    pub fn new(exe: PathBuf) -> Self {
        Self {
            exe,
            live: std::sync::Mutex::new(None),
        }
    }

    fn run(&self, img: &Rgba, mode: Mode) -> Result<OcrResult, OcrError> {
        if img.width > MAX_SIDE || img.height > MAX_SIDE {
            return Err(OcrError::Os(
                "picture too large for text recognition".into(),
            ));
        }
        let req = encode_request(img, mode);
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        // The helper leaves after a few idle seconds: a broken pipe means «start it again».
        for attempt in 0..2 {
            if live.is_none() {
                *live = Some(proc::Live::start(&self.exe)?);
            }
            match live.as_mut().map(|l| l.exchange(&req)) {
                Some(Ok(reply)) => return parse_reply(&reply, mode),
                Some(Err(e)) => {
                    *live = None;
                    if attempt == 1 {
                        return Err(e);
                    }
                }
                None => unreachable!(),
            }
        }
        unreachable!()
    }
}

impl Ocr for TessHelper {
    fn languages(&self) -> Vec<String> {
        LANGUAGES.iter().map(|s| s.to_string()).collect()
    }

    fn recognize(&self, img: &Rgba, languages: &[&str]) -> Result<OcrResult, OcrError> {
        let wanted: Vec<&str> = if languages.is_empty() {
            LANGUAGES.to_vec()
        } else {
            languages.to_vec()
        };
        let (usable, mut missing) = match_languages(&wanted, &self.languages());
        missing.retain(|m| !is_russian(m));
        if usable.is_empty() {
            return Err(OcrError::NoLanguage {
                available: self.languages(),
            });
        }
        let mut r = self.run(img, Mode::Text)?;
        r.missing = missing;
        Ok(r)
    }

    fn recognize_for_masking(
        &self,
        img: &Rgba,
        _languages: &[&str],
    ) -> Result<OcrResult, OcrError> {
        self.run(img, Mode::Masking)
    }
}

#[cfg(windows)]
mod proc {
    //! The child process in a job object: memory cap, killed with the job, no console window,
    //! below-normal priority.
    use super::OcrError;
    use std::io::{Read, Write};
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// Memory cap of the helper: measured peaks are 30–55 MB (a full 2560×1440 screen); a
    /// picture that would take far more is refused by the system instead of hurting the machine.
    const MEMORY_CAP: usize = 300 << 20;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

    fn os(e: impl std::fmt::Display) -> OcrError {
        OcrError::Os(format!("helper: {e}"))
    }

    /// The job's handle as a number: `HANDLE` is not `Send`, the recogniser must be.
    struct Job(isize);

    impl Job {
        fn handle(&self) -> HANDLE {
            HANDLE(self.0 as *mut _)
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: our own handle; closing it kills the helper (KILL_ON_JOB_CLOSE).
            let _ = unsafe { CloseHandle(self.handle()) };
        }
    }

    pub struct Live {
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
        _job: Job,
    }

    impl Live {
        pub fn start(exe: &Path) -> Result<Self, OcrError> {
            // SAFETY: plain creation with no name or security attributes.
            let job = Job(unsafe { CreateJobObjectW(None, None) }.map_err(os)?.0 as isize);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            info.ProcessMemoryLimit = MEMORY_CAP;
            // SAFETY: the struct and its size match the information class.
            unsafe {
                SetInformationJobObject(
                    job.handle(),
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                )
            }
            .map_err(os)?;
            let mut child = Command::new(exe)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
                .spawn()
                .map_err(os)?;
            // SAFETY: a live child's process handle.
            let assigned = unsafe {
                AssignProcessToJobObject(job.handle(), HANDLE(child.as_raw_handle() as *mut _))
            };
            if let Err(e) = assigned {
                let _ = child.kill();
                return Err(os(e));
            }
            let stdin = child.stdin.take().ok_or_else(|| os("no stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| os("no stdout"))?;
            Ok(Self {
                child,
                stdin,
                stdout,
                _job: job,
            })
        }

        pub fn exchange(&mut self, req: &[u8]) -> Result<Vec<u8>, OcrError> {
            self.stdin.write_all(req).map_err(os)?;
            self.stdin.flush().map_err(os)?;
            let mut len = [0u8; 4];
            self.stdout.read_exact(&mut len).map_err(os)?;
            let n = u32::from_le_bytes(len) as usize;
            if n > 64 << 20 {
                return Err(os("reply too large"));
            }
            let mut buf = vec![0u8; n];
            self.stdout.read_exact(&mut buf).map_err(os)?;
            Ok(buf)
        }
    }

    impl Drop for Live {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(not(windows))]
mod proc {
    use super::OcrError;
    use std::path::Path;

    pub struct Live;

    impl Live {
        pub fn start(_exe: &Path) -> Result<Self, OcrError> {
            Err(OcrError::Unsupported)
        }
        pub fn exchange(&mut self, _req: &[u8]) -> Result<Vec<u8>, OcrError> {
            Err(OcrError::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_header_and_luma() {
        let img = Rgba::new(2, 1, vec![255, 255, 255, 255, 0, 0, 0, 255]).unwrap();
        let r = encode_request(&img, Mode::Masking);
        assert_eq!(&r[..4], b"ZOCR");
        assert_eq!(u32::from_le_bytes(r[4..8].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(r[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(r[12..16].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(r[16..20].try_into().unwrap()), 1);
        assert_eq!(&r[20..], &[255, 0]);
    }

    #[test]
    fn reply_becomes_lines_and_words() {
        let json = br#"{"lines":[{"x":10,"y":20,"w":100,"h":14,"conf":91.5,"text":"v.plum@example.com o 14:35",
            "words":[{"x":10,"y":20,"w":60,"h":14,"conf":90,"text":"v.plum@example.com"}]}]}"#;
        let r = parse_reply(json, Mode::Text).unwrap();
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].words[0].text, "v.plum@example.com");
        assert_eq!(
            r.lines[0].rect,
            Rect {
                x: 10.0,
                y: 20.0,
                w: 100.0,
                h: 14.0
            }
        );
        assert!((r.lines[0].confidence.unwrap() - 0.915).abs() < 1e-6);
        assert_eq!(r.languages, vec!["uk", "en"]);
        assert!(matches!(
            parse_reply(br#"{"error":"bad"}"#, Mode::Text),
            Err(OcrError::Os(_))
        ));
        assert!(parse_reply(b"not json", Mode::Text).is_err());
    }
}
