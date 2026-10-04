//! What an agent may do (ZK-69). Every tool belongs to a [`Scope`]; the first use of a scope by a
//! client asks the person — «цей раз / ця сесія / завжди» — through the app; the answer is kept
//! for that long. «Завжди» lives in `<data>/agents.json`; «ця сесія» only in memory (one MCP
//! process = one session); «цей раз» is not kept at all.
//!
//! A client is known by the name it reports (`clientInfo.name`). That name is self-reported by the
//! program that started us — the dialog shows it as such; it is not proof of identity.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a tool touches.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Screenshots and the list of windows (titles tell what the person is doing).
    Capture,
    /// Reading library documents, OCR of them, export copies.
    LibraryRead,
    /// Changing library documents (marks, masking) and adding new ones.
    LibraryWrite,
    /// Reading or changing Znimok's settings.
    Settings,
    /// Recording the screen as video (ZK-237) — also unattended, for as long as it is allowed.
    Record,
    /// The computer's sound and the microphone in a recording: asked for on its own.
    RecordAudio,
    /// Sending documents out to the person's connected services (ZK-274): asked for on its own.
    Share,
}

impl Scope {
    pub const ALL: [Scope; 7] = [
        Scope::Capture,
        Scope::LibraryRead,
        Scope::LibraryWrite,
        Scope::Settings,
        Scope::Record,
        Scope::RecordAudio,
        Scope::Share,
    ];

    /// Never given by «this session» / «always» for everything (ZK-251): what leaves the
    /// computer or listens is its own question.
    pub fn asked_alone(self) -> bool {
        matches!(self, Scope::RecordAudio | Scope::Share)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::LibraryRead => "library_read",
            Self::LibraryWrite => "library_write",
            Self::Settings => "settings",
            Self::Record => "record",
            Self::RecordAudio => "record_audio",
            Self::Share => "share",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.name() == s)
    }
}

/// How long an answer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    Once,
    Session,
    Always,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    /// Client name → scopes allowed «always».
    always: BTreeMap<String, BTreeSet<Scope>>,
}

/// Allowed now, or the person has to be asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed(Grant),
    Ask,
}

pub struct Permissions {
    path: PathBuf,
    session: Mutex<HashSet<(String, Scope)>>,
}

impl Permissions {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            session: Mutex::new(HashSet::new()),
        }
    }

    /// `<data>/agents.json` of the standard folders.
    pub fn open_default() -> Option<Self> {
        znimok_settings::Dirs::system().map(|d| Self::new(d.data.join("agents.json")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Stored {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn store(&self, s: &Stored) -> std::io::Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn check(&self, client: &str, scope: Scope) -> Decision {
        if self
            .load()
            .always
            .get(client)
            .is_some_and(|s| s.contains(&scope))
        {
            return Decision::Allowed(Grant::Always);
        }
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if session.contains(&(client.to_string(), scope)) {
            return Decision::Allowed(Grant::Session);
        }
        Decision::Ask
    }

    /// Keeps the person's answer for as long as it holds.
    pub fn grant(&self, client: &str, scope: Scope, g: Grant) -> std::io::Result<()> {
        match g {
            Grant::Once => Ok(()),
            Grant::Session => {
                self.session
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert((client.to_string(), scope));
                Ok(())
            }
            Grant::Always => {
                let mut s = self.load();
                s.always
                    .entry(client.to_string())
                    .or_default()
                    .insert(scope);
                self.store(&s)
            }
        }
    }

    /// Takes back everything a client has («відкликати»).
    pub fn revoke(&self, client: &str) -> std::io::Result<()> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|(c, _)| c != client);
        let mut s = self.load();
        if s.always.remove(client).is_some() {
            self.store(&s)?;
        }
        Ok(())
    }

    /// «Відкликати всіх».
    pub fn revoke_all(&self) -> std::io::Result<()> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.store(&Stored::default())
    }

    /// Clients with lasting permissions, for the «Агенти» page.
    pub fn clients(&self) -> BTreeMap<String, BTreeSet<Scope>> {
        self.load().always
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("znimok-perm-{tag}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn once_session_always_and_revoke() {
        let p = temp("grants");
        let perms = Permissions::new(&p);
        assert_eq!(perms.check("Claude Code", Scope::Capture), Decision::Ask);
        perms
            .grant("Claude Code", Scope::Capture, Grant::Once)
            .unwrap();
        assert_eq!(
            perms.check("Claude Code", Scope::Capture),
            Decision::Ask,
            "once is not kept"
        );
        perms
            .grant("Claude Code", Scope::Capture, Grant::Session)
            .unwrap();
        assert_eq!(
            perms.check("Claude Code", Scope::Capture),
            Decision::Allowed(Grant::Session)
        );
        // A new process (new session) forgets it.
        assert_eq!(
            Permissions::new(&p).check("Claude Code", Scope::Capture),
            Decision::Ask
        );
        perms
            .grant("Claude Code", Scope::LibraryRead, Grant::Always)
            .unwrap();
        assert_eq!(
            Permissions::new(&p).check("Claude Code", Scope::LibraryRead),
            Decision::Allowed(Grant::Always)
        );
        // Another client has nothing.
        assert_eq!(perms.check("Other", Scope::LibraryRead), Decision::Ask);
        perms.revoke("Claude Code").unwrap();
        assert_eq!(perms.check("Claude Code", Scope::Capture), Decision::Ask);
        assert_eq!(
            perms.check("Claude Code", Scope::LibraryRead),
            Decision::Ask
        );
        perms.grant("A", Scope::Settings, Grant::Always).unwrap();
        perms.revoke_all().unwrap();
        assert!(perms.clients().is_empty());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn scope_names_round_trip() {
        for s in Scope::ALL {
            assert_eq!(Scope::parse(s.name()), Some(s));
        }
        assert_eq!(Scope::parse("root"), None);
    }
}
