//! Commands and queries — the one vocabulary shared by the GUI, CLI, MCP agents, the assistant
//! and tests (PLAN §5.4). Every action is a [`Command`] value, every read a [`Query`]; both are
//! plain JSON with a `"cmd"` / `"query"` tag, versioned by [`SCHEMA_VERSION`], and described by
//! a generated JSON Schema ([`command_schema`], [`query_schema`]) that agents can read.
//!
//! Objects are addressed by their stable [`ObjectId`], never by index: indices shift when
//! something is deleted or reordered, ids do not.
//!
//! The document commands are executed by [`crate::editor::Editor`]. The application-level ones
//! (capture, export, share, library, settings, agents) are part of the same vocabulary so an
//! agent sees one API, but they are carried out by the app; the core answers them with
//! [`CoreError::NotInCore`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::history::MergeKey;
use crate::model::{
    Align, Corners, CounterGroup, Dash, Data, Effect, GroupId, IRect, Meta, Object, ObjectId,
    Recipe, Rgb, Style,
};

/// Version of the command/query schema. Bumped on incompatible changes; additions of optional
/// fields or new commands do not bump it.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arrange {
    Front,
    Back,
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlignEdge {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum CaptureTarget {
    /// The display under the pointer, or the given display.
    Screen {
        display: Option<u32>,
    },
    Window {
        window: u64,
    },
    Region {
        rect: IRect,
    },
    /// Interactive overlay: the user chooses.
    Interactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptureDest {
    Editor,
    Overlay,
    Clipboard,
    Library,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Html,
}

/// Partial update of a style: only the given fields change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StylePatch {
    pub color: Option<Rgb>,
    pub thick: Option<i32>,
    pub alpha: Option<u8>,
    pub no_main: Option<bool>,
    /// `Some(None)` removes the second colour.
    #[serde(with = "double_option", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Rgb>")]
    pub color2: Option<Option<Rgb>>,
    pub alpha2: Option<u8>,
    pub dash: Option<Dash>,
    pub corners: Option<Corners>,
    pub corner_px: Option<i32>,
    pub shadow: Option<Effect>,
    pub glow: Option<Effect>,
}

impl StylePatch {
    pub fn apply(&self, s: &mut Style) {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(v) = self.$f { s.$f = v; } )* };
        }
        set!(
            color, thick, alpha, no_main, alpha2, dash, corners, corner_px, shadow, glow
        );
        if let Some(c2) = self.color2 {
            s.color2 = c2;
        }
        s.alpha = s.alpha.clamp(10, 100);
        s.alpha2 = s.alpha2.clamp(10, 100);
        s.thick = s.thick.max(1);
    }
}

/// Partial update of objects. Fields that do not apply to a kind are ignored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectPatch {
    pub rect: Option<IRect>,
    pub style: Option<StylePatch>,
    pub rot: Option<u16>,
    /// `Some(None)` clears the name (back to the automatic one).
    #[serde(with = "double_option", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub name: Option<Option<String>>,
    pub hidden: Option<bool>,
    /// Text marks: new text.
    pub text: Option<String>,
    /// Text marks: font size in screenshot pixels.
    pub size: Option<i32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub align: Option<Align>,
    /// Replace the kind-specific data entirely; must keep the same kind.
    pub data: Option<Data>,
}

/// Every action on Znimok. See the module docs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    // ---- document: objects
    /// Adds a mark on top. The id is assigned by the document (returned in `Applied.created`);
    /// counters get the next sequence number and their group's start number.
    AddObject {
        object: Object,
        #[serde(default)]
        select: bool,
    },
    UpdateObjects {
        ids: Vec<ObjectId>,
        patch: ObjectPatch,
        #[serde(default)]
        merge: Option<MergeKey>,
    },
    DeleteObjects {
        ids: Vec<ObjectId>,
    },
    /// Moves marks by an offset in screenshot pixels (arrow keys use `merge: nudge`).
    MoveObjects {
        ids: Vec<ObjectId>,
        dx: i32,
        dy: i32,
        #[serde(default)]
        merge: Option<MergeKey>,
    },
    /// Drags handle `handle` of one mark by (dx, dy) from `orig` (the rectangle when the drag began).
    ResizeObject {
        id: ObjectId,
        handle: usize,
        orig: IRect,
        dx: i32,
        dy: i32,
        #[serde(default)]
        merge: Option<MergeKey>,
    },

    // ---- document: selection and structure (selection alone is not an undo step)
    Select {
        ids: Vec<ObjectId>,
        #[serde(default)]
        add: bool,
    },
    SelectAll,
    ClearSelection,
    Group {
        ids: Vec<ObjectId>,
    },
    Ungroup {
        ids: Vec<ObjectId>,
    },
    RenameGroup {
        group: GroupId,
        name: String,
    },
    Arrange {
        ids: Vec<ObjectId>,
        to: Arrange,
    },
    /// Aligns to the common bounds of the marks, or to the frame when there is only one.
    Align {
        ids: Vec<ObjectId>,
        edge: AlignEdge,
    },
    /// Equal gaps between three or more marks along an axis.
    Distribute {
        ids: Vec<ObjectId>,
        axis: Axis,
    },
    SetCounterStart {
        group: CounterGroup,
        start: i32,
    },

    // ---- document: picture
    /// `None` removes the crop.
    SetCrop {
        rect: Option<IRect>,
    },
    /// Changes the tone part of the recipe; missing fields stay.
    SetTone {
        #[serde(default)]
        exposure: Option<f32>,
        #[serde(default)]
        gamma: Option<f32>,
        #[serde(default)]
        contrast: Option<i32>,
        #[serde(default)]
        merge: Option<MergeKey>,
    },
    ResetTone,
    /// Quarter turns clockwise (negative = counter-clockwise).
    Rotate {
        quarters: i32,
    },
    /// Resamples the picture; mark geometry follows, thicknesses and font sizes do not unless
    /// `scale_text`. Bakes the turns into a new original (the old one stays for undo).
    ResizeImage {
        width: u32,
        height: u32,
        #[serde(default)]
        scale_text: bool,
    },
    /// New canvas rectangle in picture coordinates (may reach beyond it); new area gets `fill`
    /// or stays transparent.
    ResizeCanvas {
        rect: IRect,
        #[serde(default)]
        fill: Option<Rgb>,
    },
    Mirror,
    SetName {
        name: String,
    },
    SetMeta {
        meta: Meta,
    },

    // ---- history
    Undo,
    Redo,

    // ---- application level (carried out by the app, not the core)
    Capture {
        target: CaptureTarget,
        dest: CaptureDest,
    },
    Export {
        format: ImageFormat,
        /// Destination file; `None` = clipboard.
        #[serde(default)]
        path: Option<String>,
    },
    Share {
        target: String,
    },
    Open {
        path: String,
    },
    Save,
    SaveCopy {
        path: String,
    },
    LibraryRename {
        record: String,
        name: String,
    },
    LibraryDelete {
        record: String,
    },
    LibraryRestore {
        record: String,
    },
    SetSetting {
        key: String,
        value: serde_json::Value,
    },
    AgentGrant {
        client: String,
        scopes: Vec<String>,
    },
    AgentRevoke {
        client: String,
    },
}

impl Command {
    /// The `cmd` tag as written in JSON.
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("cmd").and_then(|c| c.as_str()).map(str::to_owned))
            .unwrap_or_default()
    }

    /// Whether the core executes it (document and history commands).
    pub fn is_document(&self) -> bool {
        !matches!(
            self,
            Command::Capture { .. }
                | Command::Export { .. }
                | Command::Share { .. }
                | Command::Open { .. }
                | Command::Save
                | Command::SaveCopy { .. }
                | Command::LibraryRename { .. }
                | Command::LibraryDelete { .. }
                | Command::LibraryRestore { .. }
                | Command::SetSetting { .. }
                | Command::AgentGrant { .. }
                | Command::AgentRevoke { .. }
        )
    }
}

/// Every read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "query", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    GetDocument,
    ListObjects,
    GetObject {
        id: ObjectId,
    },
    GetSelection,
    GetState,
    /// Top-most mark under a point in screenshot pixels; `px_per_doc` = current zoom (default 1).
    HitTest {
        x: f64,
        y: f64,
        #[serde(default)]
        px_per_doc: Option<f64>,
    },
    /// Marks whose bounds intersect a rectangle.
    ObjectsInRect {
        rect: IRect,
    },
}

/// What changed, for the UI to refresh only what is needed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Change {
    Objects { ids: Vec<ObjectId> },
    Added { id: ObjectId },
    Removed { ids: Vec<ObjectId> },
    Order,
    Selection,
    Crop,
    Recipe,
    Meta,
    History,
}

/// Result of a command.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Applied {
    pub changes: Vec<Change>,
    /// Id of a mark created by the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<ObjectId>,
}

impl Applied {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectInfo {
    pub index: usize,
    pub bounds: IRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter_number: Option<i32>,
    pub object: Object,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentInfo {
    pub id: String,
    pub name: String,
    pub meta: Meta,
    pub image_size: (u32, u32),
    pub frame: IRect,
    pub crop: Option<IRect>,
    pub recipe: Recipe,
    pub shot_scale: u16,
    pub object_count: usize,
    /// Sizes of the pixel banks (0 = current original unless `source` says otherwise).
    pub banks: Vec<(u32, u32)>,
    pub source: u32,
    pub group_names: Vec<(GroupId, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditorState {
    pub schema_version: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_steps: usize,
    pub redo_steps: usize,
    /// Changed since the last save mark.
    pub dirty: bool,
    pub selection: Vec<ObjectId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum QueryResult {
    Document { document: DocumentInfo },
    Objects { objects: Vec<ObjectInfo> },
    Object { object: Option<ObjectInfo> },
    Selection { ids: Vec<ObjectId> },
    State { state: EditorState },
    Hit { id: Option<ObjectId> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum CoreError {
    /// An application-level command reached the core.
    NotInCore(String),
    UnknownObject(ObjectId),
    /// The command is well-formed but cannot apply (explained).
    Invalid(String),
    /// JSON did not parse or had the wrong schema version.
    BadRequest(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::NotInCore(c) => write!(
                f,
                "command '{c}' is carried out by the application, not the document core"
            ),
            CoreError::UnknownObject(id) => write!(f, "no object with id {id}"),
            CoreError::Invalid(m) => write!(f, "{m}"),
            CoreError::BadRequest(m) => write!(f, "bad request: {m}"),
        }
    }
}

impl std::error::Error for CoreError {}

/// A command as it travels over IPC/MCP: `{"v": 1, "cmd": "...", ...}`. `v` may be omitted
/// (then the current version is assumed).
pub fn parse_command(json: &str) -> Result<Command, CoreError> {
    let mut v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| CoreError::BadRequest(e.to_string()))?;
    check_version(&mut v)?;
    serde_json::from_value(v).map_err(|e| CoreError::BadRequest(e.to_string()))
}

pub fn parse_query(json: &str) -> Result<Query, CoreError> {
    let mut v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| CoreError::BadRequest(e.to_string()))?;
    check_version(&mut v)?;
    serde_json::from_value(v).map_err(|e| CoreError::BadRequest(e.to_string()))
}

fn check_version(v: &mut serde_json::Value) -> Result<(), CoreError> {
    if let Some(obj) = v.as_object_mut()
        && let Some(ver) = obj.remove("v")
    {
        let n = ver
            .as_u64()
            .ok_or_else(|| CoreError::BadRequest("'v' must be a number".into()))?;
        if n != SCHEMA_VERSION as u64 {
            return Err(CoreError::BadRequest(format!(
                "schema version {n} is not supported (this build speaks {SCHEMA_VERSION})"
            )));
        }
    }
    Ok(())
}

/// JSON Schema of [`Command`], for agent documentation (`SKILL.md`, MCP tool descriptions).
pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).unwrap_or_default()
}

pub fn query_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Query)).unwrap_or_default()
}

/// `Option<Option<T>>` that tells "absent" from "null".
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        v: &Option<Option<T>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// One example of every command and query: used by the round-trip test and as reference
/// material for agent documentation.
pub fn examples() -> (Vec<Command>, Vec<Query>) {
    use crate::model::{CounterShape, Head, HideMode};
    let obj = |data| Object::new(IRect::new(10, 20, 100, 50), data);
    let merge = Some(MergeKey::Nudge);
    let cmds = vec![
        Command::AddObject {
            object: obj(Data::Rect),
            select: true,
        },
        Command::AddObject {
            object: obj(Data::Line {
                head_front: Head::Triangle,
                head_back: Head::None,
                head_size: 1,
            }),
            select: false,
        },
        Command::AddObject {
            object: obj(Data::Counter {
                seq: 0,
                group: 1,
                start: 1,
                shape: CounterShape::Pin,
            }),
            select: false,
        },
        Command::AddObject {
            object: obj(Data::Hide {
                mode: HideMode::Pixelate,
                strength: 60,
            }),
            select: false,
        },
        Command::UpdateObjects {
            ids: vec![1, 2],
            patch: ObjectPatch {
                style: Some(StylePatch {
                    color: Some(Rgb::BLUE),
                    color2: Some(None),
                    ..Default::default()
                }),
                name: Some(Some("Кнопка".into())),
                ..Default::default()
            },
            merge: None,
        },
        Command::DeleteObjects { ids: vec![3] },
        Command::MoveObjects {
            ids: vec![1],
            dx: -1,
            dy: 0,
            merge,
        },
        Command::ResizeObject {
            id: 1,
            handle: 4,
            orig: IRect::new(10, 20, 100, 50),
            dx: 5,
            dy: 5,
            merge: Some(MergeKey::Drag { id: 42 }),
        },
        Command::Select {
            ids: vec![1, 2],
            add: false,
        },
        Command::SelectAll,
        Command::ClearSelection,
        Command::Group { ids: vec![1, 2] },
        Command::Ungroup { ids: vec![1] },
        Command::RenameGroup {
            group: 1,
            name: "Крок 1".into(),
        },
        Command::Arrange {
            ids: vec![1],
            to: Arrange::Front,
        },
        Command::Align {
            ids: vec![1, 2],
            edge: AlignEdge::VCenter,
        },
        Command::Distribute {
            ids: vec![1, 2, 4],
            axis: Axis::Vertical,
        },
        Command::SetCounterStart { group: 1, start: 5 },
        Command::SetCrop {
            rect: Some(IRect::new(0, 0, 640, 480)),
        },
        Command::SetTone {
            exposure: Some(0.5),
            gamma: None,
            contrast: Some(10),
            merge: Some(MergeKey::Drag { id: 7 }),
        },
        Command::ResetTone,
        Command::Rotate { quarters: -1 },
        Command::ResizeImage {
            width: 800,
            height: 600,
            scale_text: false,
        },
        Command::ResizeCanvas {
            rect: IRect::new(-20, -20, 840, 640),
            fill: Some(Rgb::WHITE),
        },
        Command::Mirror,
        Command::SetName {
            name: "Налаштування друку".into(),
        },
        Command::SetMeta {
            meta: Meta {
                tags: vec!["друк".into()],
                ..Default::default()
            },
        },
        Command::Undo,
        Command::Redo,
        Command::Capture {
            target: CaptureTarget::Screen { display: None },
            dest: CaptureDest::Editor,
        },
        Command::Export {
            format: ImageFormat::Png,
            path: None,
        },
        Command::Share {
            target: "system".into(),
        },
        Command::Open {
            path: "C:/shot.znimok".into(),
        },
        Command::Save,
        Command::SaveCopy {
            path: "/tmp/copy.znimok".into(),
        },
        Command::LibraryRename {
            record: "uuid".into(),
            name: "Нова назва".into(),
        },
        Command::LibraryDelete {
            record: "uuid".into(),
        },
        Command::LibraryRestore {
            record: "uuid".into(),
        },
        Command::SetSetting {
            key: "editor.autosave".into(),
            value: serde_json::json!(true),
        },
        Command::AgentGrant {
            client: "claude-code".into(),
            scopes: vec!["capture".into(), "library.read".into()],
        },
        Command::AgentRevoke {
            client: "claude-code".into(),
        },
    ];
    let queries = vec![
        Query::GetDocument,
        Query::ListObjects,
        Query::GetObject { id: 1 },
        Query::GetSelection,
        Query::GetState,
        Query::HitTest {
            x: 12.5,
            y: 30.0,
            px_per_doc: Some(2.0),
        },
        Query::ObjectsInRect {
            rect: IRect::new(0, 0, 50, 50),
        },
    ];
    (cmds, queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every `const`/`enum` value of the tag property in a generated schema = variant names.
    fn tag_names(schema: &serde_json::Value, tag: &str) -> BTreeSet<String> {
        fn walk(v: &serde_json::Value, tag: &str, out: &mut BTreeSet<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    if let Some(p) = m.get("properties").and_then(|p| p.get(tag)) {
                        if let Some(c) = p.get("const").and_then(|c| c.as_str()) {
                            out.insert(c.to_string());
                        }
                        if let Some(e) = p.get("enum").and_then(|e| e.as_array()) {
                            out.extend(e.iter().filter_map(|x| x.as_str().map(str::to_owned)));
                        }
                    }
                    for x in m.values() {
                        walk(x, tag, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, tag, out)),
                _ => {}
            }
        }
        let mut out = BTreeSet::new();
        walk(schema, tag, &mut out);
        out
    }

    #[test]
    fn every_command_round_trips_and_every_variant_has_an_example() {
        let (cmds, queries) = examples();
        for c in &cmds {
            let json = serde_json::to_string(c).unwrap();
            let back = parse_command(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(&back, c, "{json}");
            let versioned = format!("{{\"v\":{SCHEMA_VERSION},{}", &json[1..]);
            assert_eq!(parse_command(&versioned).unwrap(), *c);
        }
        for q in &queries {
            let json = serde_json::to_string(q).unwrap();
            assert_eq!(&parse_query(&json).unwrap(), q, "{json}");
        }
        let in_schema = tag_names(&command_schema(), "cmd");
        let in_examples: BTreeSet<String> = cmds.iter().map(Command::name).collect();
        assert!(
            in_schema.len() >= 30,
            "schema parse found only {in_schema:?}"
        );
        assert_eq!(
            in_schema, in_examples,
            "every command needs an example in examples()"
        );
        let q_schema = tag_names(&query_schema(), "query");
        let q_examples: BTreeSet<String> = queries
            .iter()
            .map(|q| {
                serde_json::to_value(q).unwrap()["query"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(q_schema, q_examples);
    }

    #[test]
    fn schema_is_written_for_agents() {
        let s = serde_json::to_string_pretty(&command_schema()).unwrap();
        assert!(s.contains("add_object") && s.contains("Adds a mark on top"));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
        std::fs::create_dir_all(&dir).unwrap();
        // Rewritten only when the content changes (line endings ignored), so a test run does
        // not dirty the working tree on Windows checkouts with CRLF.
        let put = |name: &str, text: String| {
            let path = dir.join(name);
            let (cr, lf) = (char::from(13u8), char::from(10u8));
            let old: String = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .chars()
                .filter(|c| *c != cr)
                .collect();
            let new = format!("{text}{lf}");
            if old != new {
                std::fs::write(&path, new).unwrap();
            }
        };
        put("command.schema.json", s);
        put(
            "query.schema.json",
            serde_json::to_string_pretty(&query_schema()).unwrap(),
        );
    }
}
