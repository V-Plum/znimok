//! Where the work happens (ZK-68).
//!
//! - **Documents, rendering, OCR, masking** — always here, headless: the files are the truth.
//! - **Screenshots** — Windows: here (WGC needs no permission). macOS: only the app may capture
//!   (the Screen Recording permission belongs to Znimok.app), so the request goes to the running
//!   app over IPC; if it is not running, it is started in the background first.
//! - **Asking the person** and the **«агент працює» indicator** — the app, over IPC. Without the
//!   app nobody can be asked: the call is refused with instructions.
//!
//! IPC methods the app answers (see `docs/IPC.md`): `agents.ask`, `agents.activity`,
//! `capture.displays`, `capture.windows`, `capture.take`, `app.open`.

use crate::permissions::{Grant, Scope};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use znimok_core::Raster;
use znimok_ipc::{CallError, Client, Config};
use znimok_platform::{CaptureTarget, DisplayInfo, WindowInfo};

/// The running app, reached over IPC.
#[derive(Default)]
pub struct Gui {
    cfg: Config,
}

impl Gui {
    pub fn with_config(cfg: Config) -> Self {
        Self { cfg }
    }

    fn client(&self) -> Option<Client> {
        Client::connect(&self.cfg, "znimok mcp").ok()
    }

    pub fn running(&self) -> bool {
        self.client().is_some()
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut c = self.client().ok_or("Znimok is not running")?;
        c.call(method, params).map_err(|e| match e {
            CallError::Rpc(e) if e.code == znimok_ipc::RpcError::NO_METHOD => {
                format!("this Znimok version does not support «{method}» yet")
            }
            e => e.to_string(),
        })
    }

    /// Shows the permission dialog; `None` = refused or nobody to ask.
    pub fn ask(&self, client: &str, scope: Scope, tool: &str) -> Option<Grant> {
        let v = self
            .call(
                "agents.ask",
                json!({"client": client, "scope": scope, "tool": tool}),
            )
            .ok()?;
        serde_json::from_value(v["grant"].clone()).ok()
    }

    /// The indicator in the tray and the «агент працює» plate; best effort.
    pub fn activity(&self, client: &str, tool: &str, active: bool) {
        let _ = self.call(
            "agents.activity",
            json!({"client": client, "tool": tool, "active": active}),
        );
    }

    /// Starts the app in the background (macOS) and waits for its IPC.
    pub fn ensure_running(&self) -> Result<(), String> {
        if self.running() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .args(["-g", "-b", "ua.plum.znimok.app", "--args", "--background"])
                .status()
                .map_err(|e| format!("cannot start Znimok: {e}"))?;
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(15) {
                if self.running() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        let _ = (Instant::now(), Duration::ZERO);
        Err("Znimok is not running; start it and try again".into())
    }
}

/// Takes screenshots and lists what can be taken.
pub trait Capturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String>;
    fn windows(&self) -> Result<Vec<WindowInfo>, String>;
    /// The picture as sRGB RGBA, plus where it came from (`screen` / `window` / `region`).
    fn take(&self, target: &CaptureTarget) -> Result<Shot, String>;
}

pub enum Shot {
    /// Pixels to be saved as a new library document here.
    Pixels(Raster),
    /// The app already saved it as a library document (macOS).
    Saved(PathBuf),
}

/// In this process (Windows).
#[cfg(windows)]
pub struct Local(znimok_win::WinCapture);

#[cfg(windows)]
impl Capturer for Local {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        use znimok_platform::Capture;
        self.0.displays().map_err(|e| e.to_string())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        use znimok_platform::WindowList;
        Ok(self
            .0
            .windows()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|w| !w.own && !w.minimized)
            .collect())
    }
    fn take(&self, target: &CaptureTarget) -> Result<Shot, String> {
        use znimok_platform::Capture;
        let opts = Default::default();
        // WGC first; Desktop Duplication when WGC refuses (some remote and virtual displays).
        let f = match self.0.capture(target, &opts) {
            Ok(f) => f,
            Err(e) if matches!(target, CaptureTarget::Window { .. }) => return Err(e.to_string()),
            Err(e) => znimok_win::WinCapture::with_api(znimok_win::Api::Dxgi)
                .capture(target, &opts)
                .map_err(|e2| format!("{e}; Desktop Duplication: {e2}"))?,
        };
        Ok(Shot::Pixels(Raster::new(f.width, f.height, f.to_srgb8())))
    }
}

/// Through the running app (macOS; also usable on Windows when the app should own the shot).
pub struct ViaGui(pub Gui);

impl Capturer for ViaGui {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        self.0.ensure_running()?;
        serde_json::from_value(self.0.call("capture.displays", json!({}))?)
            .map_err(|e| e.to_string())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        self.0.ensure_running()?;
        serde_json::from_value(self.0.call("capture.windows", json!({}))?)
            .map_err(|e| e.to_string())
    }
    fn take(&self, target: &CaptureTarget) -> Result<Shot, String> {
        self.0.ensure_running()?;
        let v = self.0.call("capture.take", json!({ "target": target }))?;
        let path = v["path"]
            .as_str()
            .ok_or("the app did not return a document")?;
        Ok(Shot::Saved(path.into()))
    }
}

/// Any `znimok-platform` implementation (the synthetic OS in end-to-end tests, or the app's own
/// `Platform` when it runs the tools itself).
pub struct FromPlatform {
    pub capture: std::sync::Arc<dyn znimok_platform::Capture>,
    pub windows: std::sync::Arc<dyn znimok_platform::WindowList>,
}

impl Capturer for FromPlatform {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        self.capture.displays().map_err(|e| e.to_string())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        Ok(self
            .windows
            .windows()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|w| !w.own && !w.minimized)
            .collect())
    }
    fn take(&self, target: &CaptureTarget) -> Result<Shot, String> {
        let f = self
            .capture
            .capture(target, &Default::default())
            .map_err(|e| e.to_string())?;
        Ok(Shot::Pixels(Raster::new(f.width, f.height, f.to_srgb8())))
    }
}

/// The capturer for this OS.
pub fn capturer() -> Box<dyn Capturer> {
    #[cfg(windows)]
    return Box::new(Local(znimok_win::WinCapture::new()));
    #[cfg(not(windows))]
    return Box::new(ViaGui(Gui::default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_ipc::{RpcError, Server};

    fn cfg(tag: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("zka-{tag}-{}", std::process::id()));
        Config {
            suffix: Some(format!("a{tag}{}", std::process::id())),
            dir: Some(dir),
            ..Config::default()
        }
    }

    #[test]
    fn asks_the_app_and_reads_its_answer() {
        let c = cfg("ask");
        let gui = Gui::with_config(c.clone());
        assert!(!gui.running());
        assert_eq!(
            gui.ask("Claude", Scope::Capture, "capture_screen"),
            None,
            "nobody to ask"
        );
        let _s = Server::start(c, |m: &str, p: Value| match m {
            "agents.ask" => {
                assert_eq!(p["scope"], "capture");
                Ok(json!({"grant": "session"}))
            }
            "agents.activity" => Ok(Value::Null),
            _ => Err(RpcError::method_not_found(m)),
        })
        .unwrap();
        assert!(gui.running());
        assert_eq!(
            gui.ask("Claude", Scope::Capture, "capture_screen"),
            Some(Grant::Session)
        );
        gui.activity("Claude", "capture_screen", true);
        let e = gui.call("capture.take", json!({})).unwrap_err();
        assert!(e.contains("does not support"), "{e}");
    }

    #[test]
    fn capture_through_the_app_returns_its_document() {
        let c = cfg("take");
        let _s = Server::start(c.clone(), |m: &str, _p: Value| match m {
            "capture.take" => Ok(json!({"path": "/lib/Znimok-1.znimok"})),
            "capture.displays" => Ok(json!([])),
            _ => Err(RpcError::method_not_found(m)),
        })
        .unwrap();
        let v = ViaGui(Gui::with_config(c));
        assert!(v.displays().unwrap().is_empty());
        match v
            .take(&CaptureTarget::Region {
                rect: znimok_platform::Rect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            })
            .unwrap()
        {
            Shot::Saved(p) => assert_eq!(p, PathBuf::from("/lib/Znimok-1.znimok")),
            Shot::Pixels(_) => panic!("expected the app's document"),
        }
    }
}
