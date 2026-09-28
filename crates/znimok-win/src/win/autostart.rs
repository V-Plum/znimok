//! Start at login without elevation: a value under `HKCU\…\CurrentVersion\Run` (ZK-77).
//!
//! Not a scheduled task: a task «at logon» for the current user is fine without admin rights too,
//! but the Run key is what Task Manager → Startup apps shows and lets the user switch off, and it
//! needs no schtasks.exe (which antivirus heuristics dislike — see the LH experience).
//!
//! Task Manager's switch lives in `…\Explorer\StartupApproved\Run`: a 12-byte REG_BINARY whose
//! first byte is even when allowed and odd when switched off. We only read it, except when the
//! user turns autostart on in Znimok: then the "off" mark is removed (their explicit choice).

use std::path::{Path, PathBuf};

use super::reg::{delete_value, not_found, os, read_sz, write_sz};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RegGetValueW};
use windows::core::HSTRING;
use znimok_platform::{Autostart, AutostartState, PlatformError, Result};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The argument the app gets when started at login: go to the tray, open no window.
pub const BACKGROUND_ARG: &str = "--background";

pub struct RunKeyAutostart {
    run_key: String,
    approved_key: String,
    name: String,
    command: String,
}

impl RunKeyAutostart {
    /// Starts `exe --background` at login under the value name `Znimok`.
    pub fn new(exe: &Path) -> Self {
        Self::with_keys(RUN, APPROVED, "Znimok", exe)
    }

    /// For the running program (the app itself, not the CLI).
    pub fn for_current_exe() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|e| PlatformError::Other(e.to_string()))?;
        Ok(Self::new(&exe))
    }

    /// Other keys (tests use a throw-away key instead of the real Run key).
    pub fn with_keys(run_key: &str, approved_key: &str, name: &str, exe: &Path) -> Self {
        Self {
            run_key: run_key.into(),
            approved_key: approved_key.into(),
            name: name.into(),
            command: format!("\"{}\" {BACKGROUND_ARG}", exe.display()),
        }
    }

    /// What we write: `"<exe>" --background`.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// What is registered now, if anything.
    pub fn registered_command(&self) -> Result<Option<String>> {
        read_sz(&self.run_key, &self.name)
    }

    /// The program moved (a new install folder, a portable copy): point the entry at this copy.
    /// Only when autostart is registered; `true` if it was rewritten.
    pub fn repair(&self) -> Result<bool> {
        match self.registered_command()? {
            Some(c) if !same_command(&c, &self.command) => {
                write_sz(&self.run_key, &self.name, &self.command)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// The exe path of the registered command (for a hint "starts another copy").
    pub fn registered_exe(&self) -> Result<Option<PathBuf>> {
        Ok(self.registered_command()?.map(|c| exe_of(&c)))
    }

    fn switched_off_in_system(&self) -> Result<bool> {
        let mut buf = [0u8; 12];
        let mut len = buf.len() as u32;
        // SAFETY: buffer and its size are valid for the call.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(&self.approved_key),
                &HSTRING::from(&self.name),
                RRF_RT_REG_BINARY,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        match r {
            ERROR_SUCCESS | ERROR_MORE_DATA => Ok(buf[0] & 1 == 1),
            e if not_found(e) => Ok(false),
            e => Err(os(e)),
        }
    }
}

impl Autostart for RunKeyAutostart {
    fn is_enabled(&self) -> Result<bool> {
        Ok(self.state()? == AutostartState::On)
    }

    fn set_enabled(&self, on: bool) -> Result<()> {
        if on {
            write_sz(&self.run_key, &self.name, &self.command)?;
            delete_value(&self.approved_key, &self.name)
        } else {
            // Task Manager's mark goes too: a later «on» should not come back switched off.
            delete_value(&self.run_key, &self.name)?;
            delete_value(&self.approved_key, &self.name)
        }
    }

    fn state(&self) -> Result<AutostartState> {
        if self.registered_command()?.is_none() {
            return Ok(AutostartState::Off);
        }
        Ok(if self.switched_off_in_system()? {
            AutostartState::DisabledInSystem
        } else {
            AutostartState::On
        })
    }
}

/// The exe of a Run command: quoted, or up to the first space.
fn exe_of(command: &str) -> PathBuf {
    let c = command.trim();
    let exe = match c.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest),
        None => c.split(' ').next().unwrap_or(c),
    };
    PathBuf::from(exe)
}

/// Paths on Windows compare without case.
fn same_command(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{RegDeleteTreeW, RegSetKeyValueW};

    /// A private key instead of the real Run key: nothing starts at login on the test machine.
    struct Scratch(String);
    impl Scratch {
        fn new(tag: &str) -> Self {
            Self(format!(r"Software\ZnimokTest-{tag}-{}", std::process::id()))
        }
        fn autostart(&self, exe: &str) -> RunKeyAutostart {
            RunKeyAutostart::with_keys(
                &format!(r"{}\Run", self.0),
                &format!(r"{}\StartupApproved\Run", self.0),
                "Znimok",
                Path::new(exe),
            )
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: deletes only our own test key.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&self.0)) };
        }
    }

    #[test]
    fn on_off_and_the_command() {
        let s = Scratch::new("onoff");
        let a = s.autostart(r"C:\Program Files\Znimok\znimok-app.exe");
        assert_eq!(a.state().unwrap(), AutostartState::Off);
        a.set_enabled(true).unwrap();
        assert_eq!(a.state().unwrap(), AutostartState::On);
        assert!(a.is_enabled().unwrap());
        assert_eq!(
            a.registered_command().unwrap().as_deref(),
            Some(r#""C:\Program Files\Znimok\znimok-app.exe" --background"#)
        );
        assert_eq!(
            a.registered_exe().unwrap().unwrap(),
            Path::new(r"C:\Program Files\Znimok\znimok-app.exe")
        );
        a.set_enabled(true).unwrap(); // twice is fine
        a.set_enabled(false).unwrap();
        assert_eq!(a.state().unwrap(), AutostartState::Off);
        a.set_enabled(false).unwrap(); // and off twice
    }

    #[test]
    fn task_manager_switch_is_read_and_cleared_by_our_on() {
        let s = Scratch::new("approved");
        let a = s.autostart(r"C:\Z\znimok-app.exe");
        a.set_enabled(true).unwrap();
        let mut mark = [0u8; 12];
        mark[0] = 3; // what Task Manager writes when the user switches it off
        // SAFETY: valid buffer.
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(&a.approved_key),
                &HSTRING::from("Znimok"),
                windows::Win32::System::Registry::REG_BINARY.0,
                Some(mark.as_ptr().cast()),
                12,
            )
        };
        assert_eq!(r, ERROR_SUCCESS);
        assert_eq!(a.state().unwrap(), AutostartState::DisabledInSystem);
        assert!(!a.is_enabled().unwrap());
        a.set_enabled(true).unwrap();
        assert_eq!(a.state().unwrap(), AutostartState::On);
    }

    #[test]
    fn repair_points_at_this_copy_only_when_registered() {
        let s = Scratch::new("repair");
        let old = s.autostart(r"D:\old\znimok-app.exe");
        let new = s.autostart(r"C:\Users\u\AppData\Local\Programs\Znimok\znimok-app.exe");
        assert!(
            !new.repair().unwrap(),
            "nothing registered — nothing to repair"
        );
        assert_eq!(new.state().unwrap(), AutostartState::Off);
        old.set_enabled(true).unwrap();
        assert!(new.repair().unwrap());
        assert_eq!(
            new.registered_command().unwrap().as_deref(),
            Some(new.command())
        );
        assert!(!new.repair().unwrap(), "already right");
    }

    #[test]
    fn exe_of_command() {
        assert_eq!(
            exe_of(r#""C:\a b\z.exe" --background"#),
            Path::new(r"C:\a b\z.exe")
        );
        assert_eq!(exe_of(r"C:\z.exe --x"), Path::new(r"C:\z.exe"));
    }
}
