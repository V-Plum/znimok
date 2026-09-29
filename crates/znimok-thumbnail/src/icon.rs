//! The icon handler (ZK-150): `IPersistFile` + `IExtractIconW` for `Znimok.Document`, so a
//! screenshot, a video and a video with a DevTools log each show their own icon in Explorer's
//! list, details and small-icon views (large views show the thumbnail anyway).
//!
//! Explorer hands us the path; we read the head of the file (the kind lives in `INFO`/`VINF`,
//! before the pixels), and answer with the matching `.ico` installed next to this DLL
//! (`icons\doc-*.ico`). Explorer loads and caches the icon itself: no drawing here.

use super::{DocIcon, HEAD_LIMIT, doc_icon};
use std::io::Read;
use std::sync::Mutex;

use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, S_FALSE};
use windows::Win32::System::Com::{IPersist_Impl, IPersistFile, IPersistFile_Impl, STGM};
use windows::Win32::UI::Shell::{GIL_PERINSTANCE, IExtractIconW, IExtractIconW_Impl};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::core::{BOOL, GUID, PCWSTR, PWSTR, Result, implement};

pub fn clsid() -> GUID {
    GUID::from_u128(0x0f5c6e12_2a39_4aac_9c34_3e2c9305e05a)
}

#[implement(IPersistFile, IExtractIconW)]
pub struct IconHandler {
    kind: Mutex<Option<DocIcon>>,
}

impl IconHandler {
    pub fn new() -> Self {
        Self {
            kind: Mutex::new(None),
        }
    }
}

/// The start of the file: enough for the kind (the head blocks precede the stored thumbnail and
/// the pixels); read further only while `peek` still needs more, up to [`HEAD_LIMIT`].
fn kind_of(path: &str) -> DocIcon {
    let Ok(f) = std::fs::File::open(path) else {
        return DocIcon::Image;
    };
    let mut head = Vec::new();
    let mut want = 64 << 10;
    loop {
        let before = head.len();
        if (&f)
            .take((want - before) as u64)
            .read_to_end(&mut head)
            .is_err()
        {
            return DocIcon::Image;
        }
        if znimok_format::peek(&head).is_ok() || head.len() == before || want >= HEAD_LIMIT {
            return doc_icon(&head);
        }
        want = (want * 4).min(HEAD_LIMIT);
    }
}

impl IPersist_Impl for IconHandler_Impl {
    fn GetClassID(&self) -> Result<GUID> {
        Ok(clsid())
    }
}

impl IPersistFile_Impl for IconHandler_Impl {
    fn IsDirty(&self) -> windows::core::HRESULT {
        S_FALSE
    }

    fn Load(&self, file: &PCWSTR, _mode: STGM) -> Result<()> {
        // SAFETY: Explorer's NUL-terminated path, valid for this call.
        let path = unsafe { file.to_string() }.map_err(|_| windows::core::Error::from(E_FAIL))?;
        *self.kind.lock().unwrap_or_else(|e| e.into_inner()) = Some(kind_of(&path));
        Ok(())
    }

    fn Save(&self, _file: &PCWSTR, _remember: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn SaveCompleted(&self, _file: &PCWSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetCurFile(&self) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
}

impl IExtractIconW_Impl for IconHandler_Impl {
    fn GetIconLocation(
        &self,
        _flags: u32,
        icon_file: PWSTR,
        cch: u32,
        index: *mut i32,
        out_flags: *mut u32,
    ) -> Result<()> {
        let kind = self
            .kind
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or(DocIcon::Image);
        let dll = super::win::module_path().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let dir = std::path::Path::new(&dll)
            .parent()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let ico = dir.join("icons").join(kind.file_name());
        let wide: Vec<u16> = ico.as_os_str().to_string_lossy().encode_utf16().collect();
        if icon_file.is_null()
            || index.is_null()
            || out_flags.is_null()
            || wide.len() + 1 > cch as usize
        {
            return Err(E_FAIL.into());
        }
        // SAFETY: Explorer's buffer of `cch` characters and its out pointers, checked above.
        unsafe {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), icon_file.0, wide.len());
            *icon_file.0.add(wide.len()) = 0;
            *index = 0;
            // Per file (the kind differs between files of one type); Explorer loads the icon
            // from the returned file itself.
            *out_flags = GIL_PERINSTANCE;
        }
        Ok(())
    }

    fn Extract(
        &self,
        _file: &PCWSTR,
        _index: u32,
        _large: *mut HICON,
        _small: *mut HICON,
        _sizes: u32,
    ) -> Result<()> {
        // S_FALSE: «extract it yourself from the location above».
        Err(S_FALSE.into())
    }
}
