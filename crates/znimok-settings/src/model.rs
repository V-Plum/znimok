//! What the user can change. Sections follow the settings pages; every field has a default.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use znimok_platform::KeyCombo;

/// The version of this layout. A newer program may add sections and fields (kept by an older one
/// untouched); a change of meaning bumps it.
pub const SETTINGS_VERSION: u32 = 1;

/// Everything in `settings.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub general: General,
    pub capture: Capture,
    pub editor: Editor,
    pub library: Library,
    pub video: Video,
    pub gif: Gif,
    pub report: Report,
    pub updates: Updates,
    pub agents: Agents,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            general: General::default(),
            capture: Capture::default(),
            editor: Editor::default(),
            library: Library::default(),
            video: Video::default(),
            gif: Gif::default(),
            report: Report::default(),
            updates: Updates::default(),
            agents: Agents::default(),
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct General {
    /// Interface language (`"uk"`, `"en"`); empty = the system language (znimok-i18n decides).
    pub language: Option<String>,
    pub theme: Theme,
    /// The first-run guide (library folder, permissions, hotkeys) is done.
    pub onboarding_done: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the OS.
    #[default]
    Auto,
    Light,
    Dark,
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Capture {
    /// Screenshots by global hotkeys; off releases the keys (LH CAPS-87).
    pub enabled: bool,
    pub hotkeys: Hotkeys,
    /// Shots taken past the editor (to the clipboard) go to the library too.
    pub quick_save_to_library: bool,
    /// The last Esc in the overlay editor saves before closing (instead of just closing).
    pub overlay_esc_saves: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkeys: Hotkeys::default(),
            quick_save_to_library: true,
            overlay_esc_saves: false,
        }
    }
}

/// Global hotkeys; empty = off. Physical keys (see `znimok_platform::KeyCombo`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Hotkeys {
    /// The selection overlay (region / window / screen).
    pub region: Option<KeyCombo>,
    /// The whole display under the pointer.
    pub screen: Option<KeyCombo>,
    /// Image from the clipboard into the editor.
    pub clipboard: Option<KeyCombo>,
    /// An empty editor.
    pub editor: Option<KeyCombo>,
    /// Start / stop video recording.
    pub video: Option<KeyCombo>,
}

impl Hotkeys {
    /// The defaults for this OS. Windows: the Little Helpers keys. macOS: ⌃⇧ instead of the
    /// system ⌘⇧3/4/5 (the first-run guide offers to take those over once the user turns the
    /// system shortcuts off — DESIGN-HANDOFF §2.3).
    pub fn for_os(os: znimok_platform::Os) -> Self {
        let k = |s: &str| Some(KeyCombo::parse(s).expect("default hotkey"));
        match os {
            znimok_platform::Os::MacOs => Self {
                region: k("Ctrl+Shift+4"),
                screen: k("Ctrl+Shift+3"),
                clipboard: k("Ctrl+Alt+4"),
                editor: k("Ctrl+Alt+E"),
                video: k("Ctrl+Shift+5"),
            },
            znimok_platform::Os::Windows => Self {
                region: k("Alt+Shift+4"),
                screen: k("Alt+Shift+3"),
                clipboard: k("Ctrl+Alt+4"),
                editor: k("Ctrl+Alt+E"),
                video: k("Alt+Shift+5"),
            },
        }
    }

    /// `(name, combo)` in a fixed order — the settings page, conflict checks.
    pub fn iter(&self) -> [(&'static str, Option<KeyCombo>); 5] {
        [
            ("region", self.region),
            ("screen", self.screen),
            ("clipboard", self.clipboard),
            ("editor", self.editor),
            ("video", self.video),
        ]
    }

    fn slot(&mut self, name: &str) -> &mut Option<KeyCombo> {
        match name {
            "region" => &mut self.region,
            "screen" => &mut self.screen,
            "clipboard" => &mut self.clipboard,
            "editor" => &mut self.editor,
            _ => &mut self.video,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self::for_os(znimok_platform::Os::current())
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Editor {
    /// Documents save themselves (on by default; off → Ctrl+S and a question on close).
    pub autosave: bool,
    /// A drawing tool stays active after a mark is drawn.
    pub keep_tool: bool,
    /// Smart guides while moving marks.
    pub smart_guides: bool,
    /// What the main button did last time.
    pub last_action: SaveAction,
    /// The last folder of «Зберегти як…».
    pub save_dir: Option<PathBuf>,
    /// Exported files carry the title, description, author, rights, tags and date (ZK-61).
    pub write_metadata: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            autosave: true,
            keep_tool: true,
            smart_guides: true,
            last_action: SaveAction::Clipboard,
            save_dir: None,
            write_metadata: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveAction {
    #[default]
    Clipboard,
    File,
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Library {
    /// The library folder chosen by the user (asked on first run; iCloud/OneDrive/NAS allowed);
    /// empty = `Dirs::default_library`.
    pub dir: Option<PathBuf>,
    pub filter: LibraryFilter,
    pub retention: Retention,
    /// Size limit for videos, MB (separate from screenshots).
    pub video_limit_mb: u64,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            dir: None,
            filter: LibraryFilter::All,
            retention: Retention::default(),
            video_limit_mb: 5120,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryFilter {
    #[default]
    All,
    Shots,
    Videos,
}

/// How many screenshots the library keeps before the oldest go to the trash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Retention {
    pub by: RetentionBy,
    pub count: u32,
    pub size_mb: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            by: RetentionBy::Count,
            count: 100,
            size_mb: 500,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetentionBy {
    #[default]
    Count,
    Size,
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Video {
    /// Recording by its hotkey.
    pub enabled: bool,
    /// 30 or 60.
    pub fps: u32,
    pub quality: Quality,
    /// A window chosen by click is followed as it moves (else the region stays put).
    pub follow_window: bool,
    pub cursor: bool,
    pub clicks: bool,
    pub click_color: ClickColor,
    pub audio: Audio,
    /// Keep the browser DevTools log next to the recording (v2, the extension).
    pub devtools_log: bool,
    /// The browser extension may start and stop recording.
    pub extension_control: bool,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            enabled: true,
            fps: 30,
            quality: Quality::Normal,
            follow_window: true,
            cursor: true,
            clicks: true,
            click_color: ClickColor::Yellow,
            audio: Audio::default(),
            devtools_log: true,
            extension_control: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Small,
    #[default]
    Normal,
    High,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClickColor {
    #[default]
    Yellow,
    Red,
    Blue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Audio {
    pub system: bool,
    pub microphone: bool,
    /// Device ids; empty = the OS default device.
    pub system_device: Option<String>,
    pub microphone_device: Option<String>,
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Gif {
    pub fps: u32,
    /// Output width in pixels; 0 = as recorded.
    pub width: u32,
    /// Palette size, 2–256.
    pub colors: u32,
    pub dither: bool,
    /// How many times to play; 0 = forever.
    pub loops: u16,
}

impl Default for Gif {
    fn default() -> Self {
        Self {
            fps: 15,
            width: 0,
            colors: 256,
            dither: true,
            loops: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Report {
    pub format: ReportFormat,
    /// Mask secrets in URLs and headers (tokens, keys) in developer reports.
    pub mask_secrets: bool,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            format: ReportFormat::Archive,
            mask_secrets: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportFormat {
    /// `.zreport` (ZIP).
    #[default]
    Archive,
    /// One HTML file.
    Html,
    Folder,
}

// ---------------------------------------------------------------------------------------------

/// Update checks. Off until the user turns them on — no network without the user's action
/// (PLAN §5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Updates {
    pub check_daily: bool,
    /// Unix seconds of the last check.
    pub last_check: u64,
    /// The release the user was told about (no second reminder for it).
    pub notified_tag: Option<String>,
    /// A release found and not installed yet.
    pub available_tag: Option<String>,
    /// Local day number of the last reminder.
    pub reminded_day: u32,
}

/// Agents and the assistant. Cloud features stay off until the user adds a key and turns them on;
/// the key itself lives in the OS store ([`crate::Secret::AnthropicApiKey`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Agents {
    /// Model of the Ctrl+K assistant (owner, 28.09: the current Sonnet by default, Opus
    /// selectable — Sonnet 5.5 since its release on 29.09).
    pub assistant_model: String,
    pub cloud_enabled: bool,
    /// The local MCP server for agents on this machine.
    pub mcp_enabled: bool,
    /// Per cloud feature: ask before sending, send without asking, or never (ZK-70). A feature
    /// missing here is «ask».
    pub consent: std::collections::BTreeMap<CloudFeature, Consent>,
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            assistant_model: DEFAULT_MODEL.into(),
            cloud_enabled: false,
            mcp_enabled: false,
            consent: Default::default(),
        }
    }
}

impl Agents {
    pub fn consent_for(&self, f: CloudFeature) -> Consent {
        self.consent.get(&f).copied().unwrap_or_default()
    }
}

/// Things that send a picture or text to the cloud model.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CloudFeature {
    /// The Ctrl+K assistant sees the picture.
    Assistant,
    /// «Опиши знімок» / alt text.
    Describe,
    /// Text recognition in the cloud (e.g. Ukrainian on Windows, where the OS has no OCR for it).
    Ocr,
    /// Smart masking suggestions from the cloud.
    SmartMask,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    /// Show what will be sent and wait for «Надіслати».
    #[default]
    Ask,
    Allowed,
    Never,
}

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

// ---------------------------------------------------------------------------------------------

impl Settings {
    /// Brings values into their ranges, the way Little Helpers clamped registry values. Returns the
    /// dotted names of the fields it changed.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut changed = Vec::new();
        let mut fix = |name: &str, bad: bool| {
            if bad {
                changed.push(name.to_string());
            }
            bad
        };
        fn clamp<T: PartialOrd + Copy>(v: &mut T, lo: T, hi: T) -> bool {
            let c = if *v < lo {
                lo
            } else if *v > hi {
                hi
            } else {
                *v
            };
            let bad = c != *v;
            *v = c;
            bad
        }

        if self.version != SETTINGS_VERSION && fix("version", true) {
            self.version = SETTINGS_VERSION;
        }
        let lang = &mut self.general.language;
        if fix(
            "general.language",
            lang.as_deref().is_some_and(|l| l.trim().is_empty()),
        ) {
            *lang = None;
        }

        // One combination, one action: a repeat is switched off.
        let mut seen = Vec::new();
        for (name, combo) in self.capture.hotkeys.iter() {
            if let Some(c) = combo {
                if seen.contains(&c) {
                    *self.capture.hotkeys.slot(name) = None;
                    fix(&format!("capture.hotkeys.{name}"), true);
                } else {
                    seen.push(c);
                }
            }
        }

        for (name, dir) in [
            ("editor.save_dir", &mut self.editor.save_dir),
            ("library.dir", &mut self.library.dir),
        ] {
            let bad = dir
                .as_ref()
                .is_some_and(|d| d.as_os_str().is_empty() || !d.is_absolute());
            if fix(name, bad) {
                *dir = None;
            }
        }

        let r = &mut self.library.retention;
        fix("library.retention.count", clamp(&mut r.count, 1, 100_000));
        fix(
            "library.retention.size_mb",
            clamp(&mut r.size_mb, 10, 10_000_000),
        );
        fix(
            "library.video_limit_mb",
            clamp(&mut self.library.video_limit_mb, 100, 10_000_000),
        );

        let fps = if self.video.fps >= 45 { 60 } else { 30 };
        if fix("video.fps", fps != self.video.fps) {
            self.video.fps = fps;
        }
        for (name, dev) in [
            (
                "video.audio.system_device",
                &mut self.video.audio.system_device,
            ),
            (
                "video.audio.microphone_device",
                &mut self.video.audio.microphone_device,
            ),
        ] {
            if fix(name, dev.as_deref().is_some_and(str::is_empty)) {
                *dev = None;
            }
        }

        let g = &mut self.gif;
        fix("gif.fps", clamp(&mut g.fps, 1, 50));
        fix("gif.colors", clamp(&mut g.colors, 2, 256));
        if g.width != 0 {
            fix("gif.width", clamp(&mut g.width, 16, 8192));
        }

        let m = &mut self.agents.assistant_model;
        if fix("agents.assistant_model", m.trim().is_empty()) {
            *m = DEFAULT_MODEL.into();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use znimok_platform::Os;

    #[test]
    fn defaults_are_sane_and_round_trip() {
        let mut s = Settings::default();
        assert!(s.sanitize().is_empty(), "defaults need no fixing");
        let j = serde_json::to_string_pretty(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&j).unwrap(), s);
        assert!(!s.updates.check_daily, "no network by default");
        assert!(!s.agents.cloud_enabled);
        assert!(s.editor.autosave);
    }

    #[test]
    fn hotkey_defaults_per_os_are_valid_and_distinct() {
        for os in [Os::Windows, Os::MacOs] {
            let h = Hotkeys::for_os(os);
            let all: Vec<_> = h.iter().into_iter().filter_map(|(_, c)| c).collect();
            assert_eq!(all.len(), 5);
            for (i, a) in all.iter().enumerate() {
                assert!(!all[i + 1..].contains(a), "{os:?}: {a} twice");
            }
        }
        // macOS does not take the system ⌘⇧3/4/5 by default.
        let mac = Hotkeys::for_os(Os::MacOs);
        assert!(
            mac.iter()
                .into_iter()
                .all(|(_, c)| !c.unwrap().mods.contains(znimok_platform::Modifiers::META))
        );
        assert_eq!(
            Hotkeys::for_os(Os::Windows).region.unwrap().to_string(),
            "Alt+Shift+4"
        );
    }

    #[test]
    fn sanitize_clamps_and_reports() {
        let mut s = Settings::default();
        s.video.fps = 50;
        s.gif.colors = 1000;
        s.gif.width = 3;
        s.library.retention.count = 0;
        s.library.dir = Some("relative/path".into());
        s.general.language = Some(" ".into());
        s.agents.assistant_model = String::new();
        s.capture.hotkeys.video = s.capture.hotkeys.region;
        let mut ch = s.sanitize();
        ch.sort();
        assert_eq!(
            ch,
            [
                "agents.assistant_model",
                "capture.hotkeys.video",
                "general.language",
                "gif.colors",
                "gif.width",
                "library.dir",
                "library.retention.count",
                "video.fps",
            ]
        );
        assert_eq!(s.video.fps, 60);
        assert_eq!(s.gif.colors, 256);
        assert_eq!(s.gif.width, 16);
        assert_eq!(s.library.retention.count, 1);
        assert_eq!(s.capture.hotkeys.video, None);
        assert!(s.capture.hotkeys.region.is_some(), "the first one stays");
        assert_eq!(s.agents.assistant_model, DEFAULT_MODEL);
        assert!(s.sanitize().is_empty(), "idempotent");
    }

    #[test]
    fn hotkeys_are_stored_as_text_and_may_be_off() {
        let mut s = Settings::default();
        s.capture.hotkeys.video = None;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["capture"]["hotkeys"]["video"], Value::Null);
        assert!(v["capture"]["hotkeys"]["region"].is_string());
    }

    #[test]
    fn consent_defaults_to_ask_and_round_trips() {
        let mut s = Settings::default();
        assert_eq!(s.agents.consent_for(CloudFeature::Describe), Consent::Ask);
        s.agents.consent.insert(CloudFeature::Ocr, Consent::Allowed);
        s.agents
            .consent
            .insert(CloudFeature::SmartMask, Consent::Never);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["agents"]["consent"]["ocr"], "allowed");
        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(
            back.agents.consent_for(CloudFeature::SmartMask),
            Consent::Never
        );
        assert_eq!(back.agents.assistant_model, "claude-sonnet-5-5");
    }

    #[test]
    fn schema_is_generated() {
        let schema = schemars::schema_for!(Settings);
        let j = serde_json::to_value(&schema).unwrap();
        assert!(j["properties"]["capture"].is_object(), "{j}");
    }
}
