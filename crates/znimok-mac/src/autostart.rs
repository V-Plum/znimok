//! Start at login through `SMAppService.mainAppService` (ZK-77; macOS 13+, carried over from
//! P3, where the owner checked it live). The system asks the user to allow a new login item —
//! until then the state is [`AutostartState::NeedsApproval`]. Only a program inside an `.app`
//! bundle can register ([`AutostartState::Unavailable`] otherwise, e.g. `cargo run`).
//!
//! The app is started without arguments at login; it tells a login launch by
//! `LaunchedAsLoginItem` / `keyAEPropData` if it needs to (Windows passes `--background`).

use objc2_service_management::{SMAppService, SMAppServiceStatus};
use znimok_platform::{Autostart, AutostartState, PlatformError, Result};

#[derive(Default)]
pub struct MacAutostart;

impl MacAutostart {
    pub fn new() -> Self {
        Self
    }

    /// System Settings → General → Login Items, where the user allows it.
    pub fn open_login_items() {
        // SAFETY: class method without arguments.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
}

fn state_of(s: SMAppServiceStatus) -> AutostartState {
    match s {
        SMAppServiceStatus::Enabled => AutostartState::On,
        SMAppServiceStatus::RequiresApproval => AutostartState::NeedsApproval,
        SMAppServiceStatus::NotFound => AutostartState::Unavailable,
        _ => AutostartState::Off,
    }
}

impl Autostart for MacAutostart {
    fn is_enabled(&self) -> Result<bool> {
        Ok(self.state()? == AutostartState::On)
    }

    fn set_enabled(&self, on: bool) -> Result<()> {
        // SAFETY: the shared service object of this app; the calls take no arguments.
        let svc = unsafe { SMAppService::mainAppService() };
        let status = unsafe { svc.status() };
        if on && status == SMAppServiceStatus::NotFound {
            return Err(PlatformError::Unsupported(
                "autostart needs Znimok inside an .app bundle",
            ));
        }
        let r = match (on, status) {
            (true, SMAppServiceStatus::Enabled) => return Ok(()),
            (false, SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound) => {
                return Ok(());
            }
            (true, _) => unsafe { svc.registerAndReturnError() },
            (false, _) => unsafe { svc.unregisterAndReturnError() },
        };
        r.map_err(|e| PlatformError::Os {
            code: e.code() as i64,
            message: e.localizedDescription().to_string(),
        })
    }

    fn state(&self) -> Result<AutostartState> {
        // SAFETY: as above.
        Ok(state_of(unsafe { SMAppService::mainAppService().status() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only: a test binary is not a bundle, so the system says «not found». Registering is
    /// not tried here — it would add a login item for the test binary.
    #[test]
    fn state_outside_a_bundle() {
        let a = MacAutostart::new();
        assert_eq!(a.state().unwrap(), AutostartState::Unavailable);
        assert!(!a.is_enabled().unwrap());
        assert!(matches!(
            a.set_enabled(true),
            Err(PlatformError::Unsupported(_))
        ));
        a.set_enabled(false).unwrap();
    }

    #[test]
    fn statuses_map() {
        assert_eq!(state_of(SMAppServiceStatus::Enabled), AutostartState::On);
        assert_eq!(
            state_of(SMAppServiceStatus::NotRegistered),
            AutostartState::Off
        );
        assert_eq!(
            state_of(SMAppServiceStatus::RequiresApproval),
            AutostartState::NeedsApproval
        );
    }
}
