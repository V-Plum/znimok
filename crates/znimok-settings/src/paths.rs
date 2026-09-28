//! Standard folders per OS.

use std::path::{Path, PathBuf};

/// Where Znimok keeps its files.
///
/// | | Windows | macOS | Linux |
/// |---|---|---|---|
/// | config (`settings.json`) | `%APPDATA%\Znimok` | `~/Library/Application Support/Znimok` | `$XDG_CONFIG_HOME/znimok` |
/// | data (library, IPC token) | `%LOCALAPPDATA%\Znimok` | `~/Library/Application Support/Znimok` | `$XDG_DATA_HOME/znimok` |
/// | cache (thumbnails) | `%LOCALAPPDATA%\Znimok\Cache` | `~/Library/Caches/Znimok` | `$XDG_CACHE_HOME/znimok` |
/// | logs (znimok-log) | `%LOCALAPPDATA%\Znimok\Logs` | `~/Library/Logs/Znimok` | `$XDG_STATE_HOME/znimok/logs` |
///
/// Settings go to the roaming profile on Windows (they follow the user); the library, cache and
/// logs stay on the machine. `ZNIMOK_HOME` overrides everything: `<home>/config`, `<home>/data`,
/// `<home>/cache`, `<home>/logs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
}

impl Dirs {
    /// The folders for the current user, or `None` if the OS does not say where home is.
    pub fn system() -> Option<Self> {
        if let Some(h) = std::env::var_os("ZNIMOK_HOME").filter(|h| !h.is_empty()) {
            return Some(Self::under(Path::new(&h)));
        }
        Self::for_os(|k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
    }

    /// Everything under one folder.
    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            logs: root.join("logs"),
        }
    }

    fn for_os(var: impl Fn(&str) -> Option<PathBuf>) -> Option<Self> {
        if cfg!(windows) {
            let roaming = var("APPDATA")?;
            let local = var("LOCALAPPDATA")?;
            let data = local.join("Znimok");
            return Some(Self {
                config: roaming.join("Znimok"),
                cache: data.join("Cache"),
                logs: data.join("Logs"),
                data,
            });
        }
        let home = var("HOME")?;
        if cfg!(target_os = "macos") {
            let support = home.join("Library/Application Support/Znimok");
            return Some(Self {
                config: support.clone(),
                data: support,
                cache: home.join("Library/Caches/Znimok"),
                logs: home.join("Library/Logs/Znimok"),
            });
        }
        let xdg = |k: &str, fallback: &str| {
            var(k)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
                .join("znimok")
        };
        let state = xdg("XDG_STATE_HOME", ".local/state");
        Some(Self {
            config: xdg("XDG_CONFIG_HOME", ".config"),
            data: xdg("XDG_DATA_HOME", ".local/share"),
            cache: xdg("XDG_CACHE_HOME", ".cache"),
            logs: state.join("logs"),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// The library when the user has not chosen a folder (`Library::dir` is empty). The same place
    /// as `znimok library` in the CLI; `ZNIMOK_LIBRARY` overrides it.
    pub fn default_library(&self) -> PathBuf {
        match std::env::var_os("ZNIMOK_LIBRARY").filter(|v| !v.is_empty()) {
            Some(d) => d.into(),
            None => self.data.join("Library"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_one_root() {
        let d = Dirs::under(Path::new("x"));
        assert_eq!(
            d.settings_file(),
            Path::new("x").join("config").join("settings.json")
        );
        assert_eq!(d.logs, Path::new("x").join("logs"));
    }

    #[test]
    fn per_os_layout() {
        let env = |k: &str| match k {
            "APPDATA" => Some(PathBuf::from("R")),
            "LOCALAPPDATA" => Some(PathBuf::from("L")),
            "HOME" => Some(PathBuf::from("/h")),
            _ => None,
        };
        let d = Dirs::for_os(env).unwrap();
        if cfg!(windows) {
            assert_eq!(d.config, Path::new("R").join("Znimok"));
            assert_eq!(d.data, Path::new("L").join("Znimok"));
            // Same folder znimok-log and znimok-ipc use.
            assert_eq!(d.logs, Path::new("L").join("Znimok").join("Logs"));
        } else if cfg!(target_os = "macos") {
            assert_eq!(d.config, Path::new("/h/Library/Application Support/Znimok"));
            assert_eq!(d.logs, Path::new("/h/Library/Logs/Znimok"));
        } else {
            assert_eq!(d.config, Path::new("/h/.config/znimok"));
        }
        assert!(Dirs::for_os(|_| None).is_none());
    }
}
