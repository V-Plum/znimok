//! Dragging a file out of the window on Windows: OLE `DoDragDrop` with a `CF_HDROP` data
//! object. winit/Slint have no API for starting an external drag, so this talks to OLE
//! directly; the call blocks in a modal loop until the user drops or cancels.

use std::path::PathBuf;

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, DV_E_TYMED,
    E_NOTIMPL, HGLOBAL, POINT, S_OK,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource,
    IDropSource_Impl, OleInitialize,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{DROPFILES, SHCreateStdEnumFmtEtc};
use windows::core::{BOOL, HRESULT, Ref, Result, implement};

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn wants_hdrop(f: *const FORMATETC) -> HRESULT {
    let Some(f) = (unsafe { f.as_ref() }) else {
        return DV_E_FORMATETC;
    };
    if f.cfFormat != CF_HDROP.0 || f.dwAspect != DVASPECT_CONTENT.0 {
        return DV_E_FORMATETC;
    }
    if f.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
        return DV_E_TYMED;
    }
    S_OK
}

/// `DROPFILES` header followed by UTF-16 paths, each NUL-terminated, then an extra NUL.
fn build_hdrop(paths: &[PathBuf]) -> Result<HGLOBAL> {
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);
    let header = std::mem::size_of::<DROPFILES>();
    let bytes = header + wide.len() * 2;
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes)?;
        let base = GlobalLock(h) as *mut u8;
        let df = base as *mut DROPFILES;
        df.write(DROPFILES {
            pFiles: header as u32,
            pt: POINT::default(),
            fNC: BOOL(0),
            fWide: BOOL(1),
        });
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, base.add(header), wide.len() * 2);
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

#[implement(IDataObject)]
struct FileData {
    paths: Vec<PathBuf>,
}

impl IDataObject_Impl for FileData_Impl {
    fn GetData(&self, f: *const FORMATETC) -> Result<STGMEDIUM> {
        wants_hdrop(f).ok()?;
        let h = build_hdrop(&self.paths)?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: h },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _f: *const FORMATETC, _m: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
        wants_hdrop(f)
    }

    fn GetCanonicalFormatEtc(&self, _fin: *const FORMATETC, fout: *mut FORMATETC) -> HRESULT {
        if let Some(out) = unsafe { fout.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        E_NOTIMPL
    }

    fn SetData(&self, _f: *const FORMATETC, _m: *const STGMEDIUM, _release: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&[hdrop_format()]) }
    }

    fn DAdvise(
        &self,
        _f: *const FORMATETC,
        _advf: u32,
        _sink: Ref<'_, IAdviseSink>,
    ) -> Result<u32> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _c: u32) -> Result<()> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys & MK_LBUTTON == MODIFIERKEYS_FLAGS(0) {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Starts an OLE drag of `paths` as a file copy. Blocks until the drag ends; returns `true`
/// when the files were dropped somewhere.
pub fn drag_files(paths: Vec<PathBuf>) -> Result<bool> {
    // S_FALSE ("already initialised") comes back as Ok; a different apartment mode is an Err
    // we surface, because DoDragDrop would fail anyway.
    unsafe { OleInitialize(None)? };
    let data: IDataObject = FileData { paths }.into();
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT_NONE;
    let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    if hr == DRAGDROP_S_DROP {
        Ok(effect != DROPEFFECT_NONE)
    } else if hr == DRAGDROP_S_CANCEL {
        Ok(false)
    } else {
        hr.ok().map(|_| false)
    }
}
