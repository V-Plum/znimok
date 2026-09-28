//! Secrets in the OS store, never in `settings.json`.
//!
//! Windows: Credential Manager, a generic credential `<service>:<name>` persisted on this machine
//! only (not roamed). macOS: a generic password in the login Keychain, service `<service>`,
//! account `<name>`. Elsewhere: [`SecretError::Unsupported`].
//!
//! The IPC session token is not here on purpose: the CLI must read it without a Keychain prompt,
//! and on macOS an item made by the app asks before another binary reads it — see znimok-ipc.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::fmt;

/// What Znimok keeps secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Secret {
    /// The user's Anthropic API key for the assistant and cloud features (BYOK).
    AnthropicApiKey,
}

impl Secret {
    pub const ALL: &'static [Secret] = &[Secret::AnthropicApiKey];

    /// Name inside the store.
    pub const fn name(self) -> &'static str {
        match self {
            Self::AnthropicApiKey => "anthropic-api-key",
        }
    }
}

#[derive(Debug)]
pub enum SecretError {
    /// No OS store on this system.
    Unsupported,
    /// Longer than the store takes (Windows: 2560 bytes).
    TooLong,
    /// The value in the store is not UTF-8 (not written by Znimok).
    NotText,
    /// The store refused (locked Keychain, denied access…): the OS error code and message.
    Os(i32, String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "no secret store on this system"),
            Self::TooLong => write!(f, "the secret is too long for the system store"),
            Self::NotText => write!(f, "the stored secret is not text"),
            Self::Os(code, m) => write!(f, "system secret store: {m} ({code})"),
        }
    }
}

impl std::error::Error for SecretError {}

pub type Result<T> = std::result::Result<T, SecretError>;

/// The store under one service name. [`Vault::default`] is Znimok's; tests use their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vault {
    service: String,
}

impl Default for Vault {
    fn default() -> Self {
        Self::new("Znimok")
    }
}

impl Vault {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.into(),
        }
    }

    pub fn get(&self, s: Secret) -> Result<Option<String>> {
        self.get_raw(s.name())
    }

    /// Replaces the value if there is one.
    pub fn set(&self, s: Secret, value: &str) -> Result<()> {
        self.set_raw(s.name(), value)
    }

    /// `true` if there was something to delete.
    pub fn delete(&self, s: Secret) -> Result<bool> {
        self.delete_raw(s.name())
    }

    pub fn has(&self, s: Secret) -> Result<bool> {
        Ok(self.get(s)?.is_some())
    }

    #[cfg(windows)]
    fn get_raw(&self, name: &str) -> Result<Option<String>> {
        win::get(&self.service, name)
    }
    #[cfg(windows)]
    fn set_raw(&self, name: &str, value: &str) -> Result<()> {
        win::set(&self.service, name, value)
    }
    #[cfg(windows)]
    fn delete_raw(&self, name: &str) -> Result<bool> {
        win::delete(&self.service, name)
    }

    #[cfg(target_os = "macos")]
    fn get_raw(&self, name: &str) -> Result<Option<String>> {
        mac::get(&self.service, name)
    }
    #[cfg(target_os = "macos")]
    fn set_raw(&self, name: &str, value: &str) -> Result<()> {
        mac::set(&self.service, name, value)
    }
    #[cfg(target_os = "macos")]
    fn delete_raw(&self, name: &str) -> Result<bool> {
        mac::delete(&self.service, name)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn get_raw(&self, _: &str) -> Result<Option<String>> {
        Err(SecretError::Unsupported)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    fn set_raw(&self, _: &str, _: &str) -> Result<()> {
        Err(SecretError::Unsupported)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    fn delete_raw(&self, _: &str) -> Result<bool> {
        Err(SecretError::Unsupported)
    }
}

/// Against the real store (Credential Manager / login Keychain) under a throw-away service name.
/// On macOS it needs an unlocked login Keychain — a desktop session or a CI runner, not plain ssh
/// (`ZNIMOK_SKIP_KEYCHAIN=1` skips it there).
#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn set_get_replace_delete() {
        if std::env::var_os("ZNIMOK_SKIP_KEYCHAIN").is_some() {
            return;
        }
        let v = Vault::new(&format!("ZnimokTest-{}", std::process::id()));
        let s = Secret::AnthropicApiKey;
        let _ = v.delete(s);
        assert_eq!(v.get(s).unwrap(), None);
        v.set(s, "sk-ant-перший").unwrap();
        assert_eq!(v.get(s).unwrap().as_deref(), Some("sk-ant-перший"));
        v.set(s, "sk-ant-second").unwrap();
        assert_eq!(v.get(s).unwrap().as_deref(), Some("sk-ant-second"));
        assert!(v.has(s).unwrap());
        assert!(v.delete(s).unwrap());
        assert!(!v.delete(s).unwrap());
        assert_eq!(v.get(s).unwrap(), None);
        // Another service does not see it.
        v.set(s, "x").unwrap();
        assert_eq!(Vault::new("ZnimokTest-other").get(s).unwrap(), None);
        v.delete(s).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn too_long_is_refused() {
        let v = Vault::new(&format!("ZnimokTest-long-{}", std::process::id()));
        assert!(matches!(
            v.set(Secret::AnthropicApiKey, &"x".repeat(3000)),
            Err(SecretError::TooLong)
        ));
    }
}
