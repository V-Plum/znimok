//! The COM side: class factory, the provider, DLL exports and per-user registration.

use super::{CLSID_STR, HEAD_LIMIT, ICON_CLSID_STR, thumbnail_rgba};
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, ERROR_SUCCESS, HINSTANCE,
    S_FALSE, S_OK,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IStream, STREAM_SEEK_SET};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RegDeleteTreeW, RegSetKeyValueW,
};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify,
    WTS_ALPHATYPE, WTSAT_ARGB,
};
use windows::core::{BOOL, GUID, HRESULT, HSTRING, Interface, Ref, Result, implement};

/// The shell's thumbnail handler slot: `.znimok\ShellEx\{this}` → our CLSID.
const THUMBNAIL_HANDLER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

/// The document type `.znimok` points to (znimok-win's `WinFileAssoc`, the MSI).
const PROG_ID: &str = "Znimok.Document";

fn clsid() -> GUID {
    GUID::from_u128(0x787777d8_e076_4fdc_8065_f7282e5d3f86)
}

static MODULE: AtomicIsize = AtomicIsize::new(0);

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0 as isize, Ordering::SeqCst);
    }
    true.into()
}

#[implement(IInitializeWithStream, IThumbnailProvider)]
struct Provider {
    head: Mutex<Option<Vec<u8>>>,
}

/// Reads the stream from its start until the stored thumbnail is in the buffer (or the limit).
fn read_head(s: &IStream) -> Result<Vec<u8>> {
    // SAFETY: plain IStream calls on a valid stream; the buffer outlives each call.
    unsafe {
        s.Seek(0, STREAM_SEEK_SET, None)?;
        let mut out = Vec::new();
        let mut chunk = vec![0u8; 1 << 20];
        loop {
            let mut got = 0u32;
            let hr = s.Read(
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                Some(&mut got),
            );
            hr.ok()?;
            out.extend_from_slice(&chunk[..got as usize]);
            if got == 0 || out.len() >= HEAD_LIMIT || znimok_format::peek(&out).is_ok() {
                return Ok(out);
            }
        }
    }
}

impl IInitializeWithStream_Impl for Provider_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> Result<()> {
        let s = stream.ok()?;
        *self.head.lock().unwrap_or_else(|p| p.into_inner()) = Some(read_head(s)?);
        Ok(())
    }
}

impl IThumbnailProvider_Impl for Provider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> Result<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(E_POINTER.into());
        }
        let head = self.head.lock().unwrap_or_else(|p| p.into_inner());
        let (w, h, rgba) = head
            .as_deref()
            .and_then(|b| thumbnail_rgba(b, cx.max(1)))
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        // SAFETY: a DIB section of w×h×4 bytes; `bits` is written right after, within its size.
        unsafe {
            let bmp = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
            // Premultiplied BGRA, as WTSAT_ARGB expects.
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(rgba.as_chunks::<4>().0)
            {
                let a = s[3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                *d = [pm(s[2]), pm(s[1]), pm(s[0]), s[3]];
            }
            *phbmp = bmp;
            *pdwalpha = WTSAT_ARGB;
        }
        Ok(())
    }
}

/// Which class a factory makes: the thumbnail provider or the icon handler (ZK-150).
#[implement(IClassFactory)]
struct Factory {
    icons: bool,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<windows::core::IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let obj: windows::core::IUnknown = if self.icons {
            super::icon::IconHandler::new().into()
        } else {
            Provider {
                head: Mutex::new(None),
            }
            .into()
        };
        // SAFETY: standard QueryInterface into the caller's out pointer.
        unsafe { obj.query(riid, ppv).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// # Safety
/// COM contract: valid pointers from the caller.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    // SAFETY: checked non-null above.
    unsafe {
        *ppv = std::ptr::null_mut();
        let icons = *rclsid == super::icon::clsid();
        if *rclsid != clsid() && !icons {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let f: IClassFactory = Factory { icons }.into();
        f.query(riid, ppv)
    }
}

/// Explorer may keep the DLL: thumbnails come in bursts.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

pub(crate) fn module_path() -> Option<String> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: the buffer and its length.
    let n = unsafe {
        GetModuleFileNameW(
            Some(HINSTANCE(MODULE.load(Ordering::SeqCst) as *mut _).into()),
            &mut buf,
        )
    };
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
}

/// Where the registration goes: the user's classes (normal), or the machine's — only for an
/// elevated process, which COM does not let read per-user registrations (a test on CI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    Machine,
}

impl Scope {
    fn root(self) -> HKEY {
        match self {
            Scope::User => HKEY_CURRENT_USER,
            Scope::Machine => HKEY_LOCAL_MACHINE,
        }
    }
}

fn set(root: HKEY, key: &str, name: &str, value: &str) -> Result<()> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    // SAFETY: NUL-terminated UTF-16 data of the given size.
    let r = unsafe {
        RegSetKeyValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(r.to_hresult().into())
    }
}

/// Registers the handler for the current user: the CLSID with this DLL (apartment-threaded) and
/// the `.znimok` thumbnail slot. `classes` is `Software\Classes` (tests pass a scratch key).
pub fn register(dll: &str, classes: &str) -> Result<()> {
    register_in(Scope::User, dll, classes)
}

/// [`register`] into a chosen hive; `Scope::Machine` needs admin rights.
pub fn register_in(scope: Scope, dll: &str, classes: &str) -> Result<()> {
    let root = scope.root();
    let set = |k: &str, n: &str, v: &str| set(root, k, n, v);
    let clsid = format!(r"{classes}\CLSID\{CLSID_STR}");
    set(&clsid, "", "Znimok thumbnail")?;
    set(&format!(r"{clsid}\InprocServer32"), "", dll)?;
    set(
        &format!(r"{clsid}\InprocServer32"),
        "ThreadingModel",
        "Apartment",
    )?;
    set(
        &format!(r"{classes}\.znimok\ShellEx\{THUMBNAIL_HANDLER}"),
        "",
        CLSID_STR,
    )?;
    // The icon handler (ZK-150): its class, and the document type asking it per file
    // (`DefaultIcon` = "%1" tells Explorer to ask the handler).
    let icon = format!(r"{classes}\CLSID\{ICON_CLSID_STR}");
    set(&icon, "", "Znimok file icon")?;
    set(&format!(r"{icon}\InprocServer32"), "", dll)?;
    set(
        &format!(r"{icon}\InprocServer32"),
        "ThreadingModel",
        "Apartment",
    )?;
    set(
        &format!(r"{classes}\{PROG_ID}\shellex\IconHandler"),
        "",
        ICON_CLSID_STR,
    )?;
    set(&format!(r"{classes}\{PROG_ID}\DefaultIcon"), "", "%1")?;
    Ok(())
}

pub fn unregister(classes: &str) -> Result<()> {
    unregister_in(Scope::User, classes)
}

/// A string value, if present.
fn get(root: HKEY, key: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: a buffer of `len` bytes.
    let r = unsafe {
        windows::Win32::System::Registry::RegGetValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from(name),
            windows::Win32::System::Registry::RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    (r == ERROR_SUCCESS && len >= 2).then(|| String::from_utf16_lossy(&buf[..len as usize / 2 - 1]))
}

pub fn unregister_in(scope: Scope, classes: &str) -> Result<()> {
    // `DefaultIcon` = "%1" only means something with the icon handler: without it, the type's
    // own icon (the app's association) should show again.
    let default_icon = format!(r"{classes}\{PROG_ID}\DefaultIcon");
    if get(scope.root(), &default_icon, "").as_deref() == Some("%1") {
        // SAFETY: plain call.
        let _ = unsafe { RegDeleteTreeW(scope.root(), &HSTRING::from(default_icon.as_str())) };
    }
    for k in [
        format!(r"{classes}\CLSID\{CLSID_STR}"),
        format!(r"{classes}\.znimok\ShellEx\{THUMBNAIL_HANDLER}"),
        format!(r"{classes}\CLSID\{ICON_CLSID_STR}"),
        format!(r"{classes}\{PROG_ID}\shellex\IconHandler"),
    ] {
        // SAFETY: plain call; a missing key is fine.
        let _ = unsafe { RegDeleteTreeW(scope.root(), &HSTRING::from(k)) };
    }
    Ok(())
}

fn notify() {
    // SAFETY: documented broadcast, no items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// `regsvr32 znimok_thumbnail.dll` — per user, no admin rights.
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    let Some(dll) = module_path() else {
        return E_FAIL;
    };
    match register(&dll, r"Software\Classes") {
        Ok(()) => {
            notify();
            S_OK
        }
        Err(e) => e.code(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    let _ = unregister(r"Software\Classes");
    notify();
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::SHCreateMemStream;

    /// The whole COM path in this process: factory → provider → stream → HBITMAP.
    #[test]
    fn com_path_gives_a_bitmap_of_the_stored_thumbnail() {
        // SAFETY: COM on this test thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let doc = crate::tests::znimok_format_doc();
        let bytes = znimok_format::write(
            &doc,
            &znimok_format::WriteOptions {
                thumbnail: Some(crate::tests::solid(320, 200, [200, 40, 30, 255])),
                ..Default::default()
            },
        );
        // SAFETY: the COM contract of this crate's own exports.
        unsafe {
            let mut f: *mut c_void = std::ptr::null_mut();
            DllGetClassObject(&clsid(), &IClassFactory::IID, &mut f)
                .ok()
                .unwrap();
            let factory = IClassFactory::from_raw(f);
            let init: IInitializeWithStream = factory.CreateInstance(None).unwrap();
            let stream = SHCreateMemStream(Some(&bytes)).unwrap();
            init.Initialize(&stream, 0).unwrap();
            let tp: IThumbnailProvider = init.cast().unwrap();
            let mut bmp = HBITMAP::default();
            let mut alpha = WTS_ALPHATYPE::default();
            tp.GetThumbnail(128, &mut bmp, &mut alpha).unwrap();
            assert_eq!(alpha, WTSAT_ARGB);
            let mut info = BITMAP::default();
            GetObjectW(
                bmp.into(),
                size_of::<BITMAP>() as i32,
                Some((&mut info as *mut BITMAP).cast()),
            );
            assert_eq!((info.bmWidth, info.bmHeight), (128, 80));
            let px = std::slice::from_raw_parts(info.bmBits.cast::<u8>(), 4);
            assert_eq!(px, [30, 40, 200, 255], "BGRA of the stored thumbnail");
            let _ = DeleteObject(bmp.into());

            // Not a Znimok file: a clean failure, not a crash.
            let other: IInitializeWithStream = factory.CreateInstance(None).unwrap();
            other
                .Initialize(&SHCreateMemStream(Some(b"hello")).unwrap(), 0)
                .unwrap();
            let tp: IThumbnailProvider = other.cast().unwrap();
            assert!(tp.GetThumbnail(64, &mut bmp, &mut alpha).is_err());

            let mut none: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                DllGetClassObject(&GUID::zeroed(), &IClassFactory::IID, &mut none),
                CLASS_E_CLASSNOTAVAILABLE
            );
        }
    }

    #[test]
    fn per_user_registration_in_a_scratch_key() {
        let classes = format!(r"Software\ZnimokTest-thumb-{}\Classes", std::process::id());
        register(r"C:\Program Files\Znimok\znimok_thumbnail.dll", &classes).unwrap();
        let read = |k: &str, n: &str| {
            let mut buf = vec![0u16; 512];
            let mut len = (buf.len() * 2) as u32;
            // SAFETY: buffer of `len` bytes.
            let r = unsafe {
                windows::Win32::System::Registry::RegGetValueW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(k),
                    &HSTRING::from(n),
                    windows::Win32::System::Registry::RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut len),
                )
            };
            assert_eq!(r, ERROR_SUCCESS, "{k}");
            String::from_utf16_lossy(&buf[..len as usize / 2 - 1])
        };
        assert_eq!(
            read(
                &format!(r"{classes}\.znimok\ShellEx\{THUMBNAIL_HANDLER}"),
                ""
            ),
            CLSID_STR
        );
        // The icon handler (ZK-150): its class and the document type asking it per file.
        assert_eq!(
            read(&format!(r"{classes}\{PROG_ID}\shellex\IconHandler"), ""),
            ICON_CLSID_STR
        );
        assert_eq!(read(&format!(r"{classes}\{PROG_ID}\DefaultIcon"), ""), "%1");
        assert_eq!(
            read(
                &format!(r"{classes}\CLSID\{ICON_CLSID_STR}\InprocServer32"),
                "ThreadingModel"
            ),
            "Apartment"
        );
        assert_eq!(
            read(
                &format!(r"{classes}\CLSID\{CLSID_STR}\InprocServer32"),
                "ThreadingModel"
            ),
            "Apartment"
        );
        unregister(&classes).unwrap();
        // Nothing of the handlers left, not even the "%1" icon (it means nothing without them).
        for k in [
            format!(r"{classes}\{PROG_ID}\DefaultIcon"),
            format!(r"{classes}\{PROG_ID}\shellex\IconHandler"),
            format!(r"{classes}\CLSID\{ICON_CLSID_STR}"),
        ] {
            assert_eq!(get(HKEY_CURRENT_USER, &k, ""), None, "{k}");
        }
        // SAFETY: deletes only the scratch key.
        let _ = unsafe {
            RegDeleteTreeW(
                HKEY_CURRENT_USER,
                &HSTRING::from(format!(r"Software\ZnimokTest-thumb-{}", std::process::id())),
            )
        };
    }
}

#[cfg(test)]
#[test]
fn clsid_constant_matches_the_string() {
    assert_eq!(format!("{{{:?}}}", clsid()), CLSID_STR);
}
