//! `.znimok` files open in Znimok on macOS (ZK-75), through Launch Services (`NSWorkspace`,
//! macOS 12+). The type itself is declared by the app bundle — `CFBundleDocumentTypes` and
//! `UTExportedTypeDeclarations` in Info.plist (fragment: `crates/znimok-mac/assoc/document-types.plist`).
//!
//! macOS has no «unregister»: the system forgets the handler when the app is deleted, so
//! [`MacFileAssoc::unregister`] does nothing. Called through the Objective-C runtime (a few
//! messages; the typed UTType crate would be a new dependency).

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSError, NSString, NSURL};
use znimok_platform::{AssocState, FileAssoc, PlatformError, Result};

#[link(name = "UniformTypeIdentifiers", kind = "framework")]
unsafe extern "C" {}

/// The bundle identifier of the app (Info.plist).
pub const BUNDLE_ID: &str = "ua.plum.znimok.app";
/// The exported document type.
pub const DOCUMENT_UTI: &str = "ua.plum.znimok.document";

pub struct MacFileAssoc {
    bundle_id: String,
}

impl Default for MacFileAssoc {
    fn default() -> Self {
        Self {
            bundle_id: BUNDLE_ID.into(),
        }
    }
}

fn content_type(ext: &str) -> Option<Retained<AnyObject>> {
    let ext = NSString::from_str(ext.trim_start_matches('.'));
    // SAFETY: `+[UTType typeWithFilenameExtension:]`, returns nil or a type.
    unsafe { msg_send![class!(UTType), typeWithFilenameExtension: &*ext] }
}

fn workspace() -> Retained<AnyObject> {
    // SAFETY: the shared workspace always exists.
    unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
}

impl MacFileAssoc {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bundle id of the app that opens `ext` now.
    fn handler(&self, ext: &str) -> Option<String> {
        let ty = content_type(ext)?;
        // SAFETY: documented selectors; nil results are handled.
        unsafe {
            let url: Option<Retained<NSURL>> =
                msg_send![&workspace(), URLForApplicationToOpenContentType: &*ty];
            let url = url?;
            let bundle: Option<Retained<AnyObject>> =
                msg_send![class!(NSBundle), bundleWithURL: &*url];
            let id: Option<Retained<NSString>> = msg_send![&bundle?, bundleIdentifier];
            id.map(|s| s.to_string())
        }
    }
}

impl FileAssoc for MacFileAssoc {
    fn state(&self, ext: &str) -> Result<AssocState> {
        Ok(match self.handler(ext) {
            Some(id) if id == self.bundle_id => AssocState::Ours,
            Some(id) => AssocState::Other(id),
            None => AssocState::None,
        })
    }

    fn register(&self, ext: &str) -> Result<()> {
        // SAFETY: documented selectors.
        let app: Retained<NSURL> = unsafe {
            let main: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
            msg_send![&main, bundleURL]
        };
        if !app.path().is_some_and(|p| p.to_string().ends_with(".app")) {
            return Err(PlatformError::Unsupported(
                "file types can be claimed only by Znimok.app",
            ));
        }
        let ty = content_type(ext).ok_or_else(|| PlatformError::NotFound(ext.into()))?;
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let done = RcBlock::new(move |err: *mut NSError| {
            // SAFETY: valid or null for the call.
            let msg = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
            let _ = tx.send(msg);
        });
        // SAFETY: documented selector; the block outlives the call (the system copies it).
        unsafe {
            let _: () = msg_send![
                &workspace(),
                setDefaultApplicationAtURL: &*app,
                toOpenContentType: &*ty,
                completionHandler: &*done
            ];
        }
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => Ok(()),
            Ok(Some(m)) => Err(PlatformError::Other(m)),
            Err(_) => Err(PlatformError::Other(
                "Launch Services did not answer".into(),
            )),
        }
    }

    fn unregister(&self, _ext: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only against the real Launch Services: PNG has a handler (Preview or similar), a
    /// test binary cannot claim anything.
    #[test]
    fn state_and_refusal_outside_a_bundle() {
        let a = MacFileAssoc::new();
        assert!(
            matches!(a.state("png").unwrap(), AssocState::Other(_)),
            "{:?}",
            a.state("png")
        );
        let _ = a.state("znimok").unwrap();
        assert!(matches!(
            a.register("znimok"),
            Err(PlatformError::Unsupported(_))
        ));
        a.unregister("znimok").unwrap();
    }
}
