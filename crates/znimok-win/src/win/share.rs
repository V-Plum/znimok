//! System «Share» on Windows (ZK-63): `DataTransferManager` for a desktop window through
//! `IDataTransferManagerInterop` — no package identity, no admin rights.
//!
//! The files are turned into `StorageFile`s before the sheet opens (the `DataRequested` handler
//! must answer at once); a picture also goes as a bitmap, so apps that take images (Mail, Teams,
//! Phone Link) get pixels, and apps that take files get the file. The sheet opens over the window
//! given to [`WinShare::new`]; Windows places it itself (the anchor is a macOS thing).

use std::path::PathBuf;
use std::sync::Mutex;

use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Storage::{IStorageItem, StorageFile};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::core::{AgileReference, HSTRING, Interface};
use znimok_platform::{PlatformError, Rect, Result, Share};

pub struct WinShare {
    hwnd: isize,
    /// Token of the previous share's handler, removed before the next one. The manager itself is
    /// bound to the window's thread, so it is fetched again each time (the same object).
    registered: Mutex<Option<i64>>,
    title: String,
}

fn os(e: windows::core::Error) -> PlatformError {
    PlatformError::Os {
        code: e.code().0 as i64,
        message: e.message(),
    }
}

impl WinShare {
    /// `hwnd`: the app's top-level window the sheet belongs to; `title`: the sheet's heading
    /// («Znimok»).
    pub fn new(hwnd: isize, title: &str) -> Self {
        Self {
            hwnd,
            registered: Mutex::new(None),
            title: title.into(),
        }
    }

    fn manager(&self) -> Result<(IDataTransferManagerInterop, DataTransferManager)> {
        super::com_thread();
        let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()
            .map_err(os)?;
        // SAFETY: a window handle of this process, as documented for desktop apps.
        let dtm: DataTransferManager =
            unsafe { interop.GetForWindow(HWND(self.hwnd as *mut _)) }.map_err(os)?;
        Ok((interop, dtm))
    }
}

/// The files as storage items, in order; missing files are an error (the sheet would be empty).
fn storage_files(files: &[PathBuf]) -> Result<Vec<StorageFile>> {
    files
        .iter()
        .map(|p| {
            let full = std::path::absolute(p).map_err(|e| PlatformError::Other(e.to_string()))?;
            StorageFile::GetFileFromPathAsync(&HSTRING::from(full.as_os_str()))
                .and_then(|op| op.join())
                .map_err(|e| PlatformError::NotFound(format!("{}: {}", p.display(), e.message())))
        })
        .collect()
}

fn is_picture(p: &std::path::Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}

impl Share for WinShare {
    fn available(&self) -> bool {
        self.manager().is_ok()
    }

    fn share(&self, files: &[PathBuf], _anchor: Option<Rect>) -> Result<()> {
        if files.is_empty() {
            return Err(PlatformError::Other("nothing to share".into()));
        }
        let (interop, dtm) = self.manager()?;
        let items = storage_files(files)?;
        // Agile references: the handler runs on the window's thread.
        let bitmap = match (files.first(), items.first()) {
            (Some(p), Some(f)) if is_picture(p) => Some(
                AgileReference::new(&RandomAccessStreamReference::CreateFromFile(f).map_err(os)?)
                    .map_err(os)?,
            ),
            _ => None,
        };
        let storage: Vec<AgileReference<IStorageItem>> = items
            .iter()
            .map(|f| {
                f.cast::<IStorageItem>()
                    .and_then(|i| AgileReference::new(&i))
            })
            .collect::<windows::core::Result<_>>()
            .map_err(os)?;
        let title = HSTRING::from(&self.title);
        let names = files
            .iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(", ");
        let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(
            move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let data = args.Request()?.Data()?;
                let props = data.Properties()?;
                props.SetTitle(&title)?;
                props.SetDescription(&HSTRING::from(names.as_str()))?;
                let list: Vec<Option<IStorageItem>> = storage
                    .iter()
                    .map(|a| a.resolve().map(Some))
                    .collect::<windows::core::Result<_>>()?;
                data.SetStorageItemsReadOnly(
                    &windows_collections::IIterable::<IStorageItem>::from(list),
                )?;
                if let Some(b) = &bitmap {
                    data.SetBitmap(&b.resolve()?)?;
                }
                Ok(())
            },
        );
        let mut reg = self.registered.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(token) = reg.take() {
            let _ = dtm.RemoveDataRequested(token);
        }
        let token = dtm.DataRequested(&handler).map_err(os)?;
        *reg = Some(token);
        drop(reg);
        // SAFETY: the same window handle as above.
        unsafe { interop.ShowShareUIForWindow(HWND(self.hwnd as *mut _)) }.map_err(os)
    }
}

impl Drop for WinShare {
    fn drop(&mut self) {
        let token = self
            .registered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let (Some(token), Ok((_, dtm))) = (token, self.manager()) {
            let _ = dtm.RemoveDataRequested(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_become_storage_items_and_missing_ones_are_refused() {
        super::super::com_thread();
        let dir = std::env::temp_dir().join(format!("znimok-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("Знімок 1.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
        let items = storage_files(std::slice::from_ref(&png)).unwrap();
        assert_eq!(items[0].Name().unwrap().to_string(), "Знімок 1.png");
        assert!(is_picture(&png) && !is_picture(&dir.join("a.znimok")));
        assert!(matches!(
            storage_files(&[dir.join("немає.png")]),
            Err(PlatformError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }
}
