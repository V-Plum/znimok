//! The MCP tools (PLAN §5.9) over the command layer, with permissions, the activity indicator
//! and the audit journal around every call.

use crate::audit::{Audit, Entry};
use crate::backend::{Capturer, Gui, Shot};
use crate::library::{self, ExportFormat, Library};
use crate::permissions::{Decision, Grant, Permissions, Scope};
use serde_json::{Value, json};
use std::path::PathBuf;
use znimok_core::{Command, Document, Editor, Raster};
use znimok_models::Rgba;
use znimok_platform::{CaptureTarget, DisplayId, Rect, WindowId};

pub const DOC_MIME: &str = "application/x-znimok";

/// Everything a tool call needs.
pub struct Agent {
    pub lib: Library,
    pub perms: Permissions,
    pub audit: Option<Audit>,
    pub gui: Gui,
    pub capture: Box<dyn Capturer>,
    /// MCP switched on in the settings (off by default, PLAN decision 23).
    pub enabled: bool,
    /// Where `export` writes when no path is given.
    pub export_dir: PathBuf,
}

/// The result of one tool call, in MCP terms.
#[derive(Debug, Default)]
pub struct Output {
    pub content: Vec<Value>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl Output {
    pub(crate) fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": msg.into()})],
            structured: None,
            is_error: true,
        }
    }

    pub(crate) fn ok(structured: Value, mut extra: Vec<Value>) -> Self {
        let mut content = vec![json!({
            "type": "text",
            "text": serde_json::to_string_pretty(&structured).unwrap_or_default()
        })];
        content.append(&mut extra);
        Self {
            content,
            structured: Some(structured),
            is_error: false,
        }
    }
}

pub(crate) struct Tool {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) scope: Option<Scope>,
    pub(crate) read_only: bool,
    pub(crate) schema: fn() -> Value,
}

pub(crate) fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

pub(crate) fn doc_arg() -> Value {
    json!({"type": "string", "description": "Library document: its id (or the first 8+ characters of it) from library_search, or a path to a .znimok file"})
}

/// The commands of the app, not of a document: not for `annotate` (ZK-250).
const APP_COMMANDS: &[&str] = &[
    "capture",
    "export",
    "share",
    "open",
    "save",
    "save_copy",
    "library_rename",
    "library_delete",
    "library_restore",
    "set_setting",
    "agent_grant",
    "agent_revoke",
];

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "list_displays",
        title: "List displays",
        description: "Displays of this computer: id, name, bounds in desktop units, scale, primary.",
        scope: None,
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "list_windows",
        title: "List windows",
        description: "Visible top-level windows: id, title, app (executable or bundle name), bounds. Use an id with capture_window.",
        scope: Some(Scope::Capture),
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
    Tool {
        name: "capture_screen",
        title: "Capture a display",
        description: "Screenshot of a whole display (the primary one by default). Saved as a new library document; returns the picture and its id.",
        scope: Some(Scope::Capture),
        read_only: false,
        schema: || {
            obj(
                json!({"display": {"type": "string", "description": "Display id from list_displays"},
                       "delay_seconds": crate::apptools::delay_arg()}),
                &[],
            )
        },
    },
    Tool {
        name: "capture_window",
        title: "Capture a window",
        description: "Screenshot of one window without what covers it. Saved as a new library document.",
        scope: Some(Scope::Capture),
        read_only: false,
        schema: || {
            obj(
                json!({"window": {"type": "integer", "description": "Window id from list_windows"},
                       "delay_seconds": crate::apptools::delay_arg()}),
                &["window"],
            )
        },
    },
    Tool {
        name: "capture_region",
        title: "Capture a region",
        description: "Screenshot of a rectangle in desktop units (within one display). Saved as a new library document.",
        scope: Some(Scope::Capture),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "x": {"type": "integer"}, "y": {"type": "integer"},
                    "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1},
                    "delay_seconds": crate::apptools::delay_arg()
                }),
                &["x", "y", "width", "height"],
            )
        },
    },
    Tool {
        name: "annotate",
        title: "Annotate a document",
        description: "Applies editor commands (add arrows, frames, text, counters, hide areas…) to a library document and saves it. Coordinates are screenshot pixels. Returns the result.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            // `$defs` of the command schema go to the root, where `#/$defs/…` resolve.
            let mut cmd = serde_json::to_value(schemars::schema_for!(Command)).unwrap_or_default();
            let defs = cmd.as_object_mut().and_then(|o| {
                o.remove("$schema");
                o.remove("$defs")
            });
            // The document's own commands: the core refuses the app's (capture, export, the
            // settings, the grants) anyway, and a client reads the schema as what the tool may do.
            if let Some(list) = cmd["oneOf"].as_array_mut() {
                list.retain(|v| {
                    let c = &v["properties"]["cmd"];
                    let name = c["const"]
                        .as_str()
                        .or_else(|| c["enum"][0].as_str())
                        .unwrap_or("");
                    !APP_COMMANDS.contains(&name)
                });
            }
            let mut s = obj(
                json!({
                    "document": doc_arg(),
                    "commands": {"type": "array", "items": cmd,
                                 "description": "Commands in order; each is one step of undo"}
                }),
                &["document", "commands"],
            );
            if let Some(d) = defs {
                s["$defs"] = d;
            }
            s
        },
    },
    Tool {
        name: "export",
        title: "Export a document",
        description: "Writes a document to a file. A screenshot: PNG, JPEG, WebP or one self-contained HTML page, with its marks. A recording: MP4 (the cuts, the marks in their time, the sound), GIF, html (one page with the video — and the browser's DevTools log beside it when the recording has one), report (that page) or zreport (an archive: the page, the video, log.json), or one frame as PNG / JPEG / WebP (at_ms). Sensitive values of the log are hidden as the person set it. Never writes over a file.",
        scope: Some(Scope::LibraryRead),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "format": {"type": "string", "enum": ["png", "jpeg", "webp", "html", "mp4", "gif", "report", "zreport"]},
                    "path": {"type": "string", "description": "Output file; default: a new file in Znimok's export folder"},
                    "at_ms": {"type": "integer", "minimum": 0, "description": "A recording to a picture: the frame at this time (as recorded)"},
                    "sound": {"type": "boolean", "description": "MP4: with the sound tracks (true)"},
                    "gif_width": {"type": "integer", "minimum": 80, "maximum": 1920, "description": "GIF: width (640 or less by default)"},
                    "gif_fps": {"type": "number", "minimum": 1, "maximum": 30, "description": "GIF: frames a second (10)"},
                    "language": {"type": "string", "enum": ["uk", "en"], "description": "The report page's language (as set in Znimok by default)"},
                    "hide": {"type": "boolean", "description": "The report: hide sensitive values (true), when the settings leave it to the export"}
                }),
                &["document", "format"],
            )
        },
    },
    Tool {
        name: "library_search",
        title: "Search the library",
        description: "Library documents, newest first: by the text in name, description or tags, by a tag, by kind (screenshot or video), pinned, with the browser's DevTools log, by the day they were made. trash=true lists the trash instead.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "query": {"type": "string"}, "tag": {"type": "string"},
                    "kind": {"type": "string", "enum": ["screenshot", "video"]},
                    "pinned": {"type": "boolean"},
                    "has_log": {"type": "boolean", "description": "Recordings with the browser's DevTools log"},
                    "since": {"type": "string", "description": "Made on or after this day, YYYY-MM-DD (local time)"},
                    "until": {"type": "string", "description": "Made before this day, YYYY-MM-DD"},
                    "trash": {"type": "boolean", "description": "List the trash instead of the library"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200},
                    "with_tags": {"type": "boolean", "description": "Also every tag of the library with its count, the most used first"}
                }),
                &[],
            )
        },
    },
    Tool {
        name: "library_get",
        title: "Get a document",
        description: "One library document: its picture with marks and its metadata.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "ocr",
        title: "Recognise text",
        description: "Text of a library document, on this computer (nothing leaves it), with line boxes in pixels.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "languages": {"type": "array", "items": {"type": "string"}, "description": "BCP 47 tags in order of preference, e.g. [\"uk\", \"en\"]"}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "read_codes",
        title: "Read QR codes and barcodes",
        description: "QR codes, Data Matrix, Aztec, PDF417 and 1-D barcodes on a library document or on the screen (a display, or a region in desktop units), read on this computer. Each: the text, its kind (link, wifi, contact, event, email, phone, text) with the parsed fields, and its box in pixels. Reading the screen does not add a document (on macOS the app takes the shot and keeps it in the library). Links are only reported — open one only if the person asks.",
        // Library documents: library_read; the screen: capture (see `scope_for`).
        scope: Some(Scope::Capture),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "display": {"type": "string", "description": "Display id from list_displays (the primary one when neither document nor region is given)"},
                    "x": {"type": "integer"}, "y": {"type": "integer"},
                    "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1}
                }),
                &[],
            )
        },
    },
    Tool {
        name: "redact_pii",
        title: "Hide secrets and personal data",
        description: "Finds keys, passwords, e-mails, phones, cards, IBANs and faces on the document (on this computer) and, unless apply=false, covers them with Hide marks and saves.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "apply": {"type": "boolean", "default": true},
                    "faces": {"type": "boolean", "default": true}
                }),
                &["document"],
            )
        },
    },
];

/// Every tool: the first ones, then the editing ones (ZK-233).
/// Every inner tool, the handlers behind the published ones.
fn inner() -> impl Iterator<Item = &'static Tool> {
    TOOLS
        .iter()
        .chain(crate::edit::TOOLS.iter())
        .chain(crate::libtools::TOOLS.iter())
        .chain(crate::vidtools::TOOLS.iter())
        .chain(crate::rectools::TOOLS.iter())
        .chain(crate::apptools::TOOLS.iter())
}

/// The published tools (ZK-250): the facade's, and the inner ones it keeps as they are.
fn all() -> impl Iterator<Item = &'static Tool> {
    crate::facade::TOOLS
        .iter()
        .chain(inner().filter(|t| crate::facade::KEPT.contains(&t.name)))
}

/// `tools/list` entries, in a fixed order.
pub fn list() -> Vec<Value> {
    all()
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "title": t.title,
                    "readOnlyHint": t.read_only,
                    "idempotentHint": t.read_only,
                    "destructiveHint": crate::edit::destructive(t.name) || crate::libtools::destructive(t.name),
                    "openWorldHint": false
                }
            })
        })
        .collect()
}

/// A picture for the client: scaled to what models take, as PNG.
pub(crate) fn image(r: &Raster) -> Option<Value> {
    let img = Rgba::new(r.width, r.height, r.rgba.clone())?;
    let p = znimok_models::image_prep::prepare(&img).ok()?;
    Some(json!({"type": "image", "data": p.base64, "mimeType": p.media_type}))
}

pub(crate) fn link(doc: &Document) -> Value {
    json!({
        "type": "resource_link",
        "uri": format!("znimok://library/{}", doc.id),
        "name": doc.name,
        "mimeType": DOC_MIME,
        "description": "The library document (original pixels and marks)"
    })
}

pub(crate) fn summary(doc: &Document, path: &std::path::Path) -> Value {
    let f = doc.frame();
    json!({
        "id": doc.id.to_string(),
        "name": doc.name,
        "path": path.display().to_string(),
        "width": f.w, "height": f.h,
        "marks": doc.objects.len(),
        "source": doc.meta.source,
        "tags": doc.meta.tags,
        "description": doc.meta.description,
        "pinned": doc.meta.pinned,
    })
}

pub(crate) fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

/// A required whole-number argument. Missing and wrong-typed are told apart, and a number sent
/// as a string ("197496") is accepted — agents do that (ZK-123).
/// The permission a call needs: the tool's, except `read_codes` on a library document, which only
/// reads the library.
fn scope_for(tool: &Tool, args: &Value) -> Option<Scope> {
    if tool.name == "read_codes" && arg_str(args, "document").is_some() {
        return Some(Scope::LibraryRead);
    }
    tool.scope
}

/// One code for the agent: the text, the kind with its fields, the box.
fn code_json(c: &znimok_codes::Code) -> Value {
    use znimok_codes::Kind;
    let (x, y, w, h) = c.bounds;
    let mut v = json!({
        "format": c.format,
        "text": c.text,
        "bounds": {"x": x, "y": y, "w": w, "h": h},
    });
    let (kind, extra) = match &c.kind {
        Kind::Link(url) => ("link", json!({"url": url})),
        Kind::Wifi {
            ssid,
            password,
            security,
            hidden,
        } => (
            "wifi",
            json!({"ssid": ssid, "password": password, "security": security, "hidden": hidden}),
        ),
        Kind::Contact => ("contact", json!({})),
        Kind::Event => ("event", json!({})),
        Kind::Email(a) => ("email", json!({"address": a})),
        Kind::Phone(n) => ("phone", json!({"number": n})),
        Kind::Text => ("text", json!({})),
    };
    v["kind"] = json!(kind);
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

pub(crate) fn arg_int(a: &Value, k: &str) -> Result<i64, String> {
    match a.get(k) {
        None | Some(Value::Null) => Err(format!("«{k}» is required")),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .ok_or_else(|| format!("«{k}» must be a whole number, got {v}")),
    }
}

impl Agent {
    pub(crate) fn authorize(
        &self,
        client: &str,
        scope: Scope,
        tool: &str,
    ) -> Result<Grant, String> {
        match self.perms.check(client, scope) {
            Decision::Allowed(g) => Ok(g),
            Decision::Ask => match self.gui.ask_all(client, scope, tool) {
                Some((g, all)) => {
                    // «This session» / «always» for everything (ZK-251), but never the sound of a
                    // recording or sending out (ZK-274) — those stay their own questions.
                    let scopes: Vec<Scope> = if all {
                        Scope::ALL
                            .into_iter()
                            .filter(|s| !s.asked_alone())
                            .collect()
                    } else {
                        vec![scope]
                    };
                    for s in scopes {
                        let _ = self.perms.grant(client, s, g);
                    }
                    Ok(g)
                }
                None => Err(format!(
                    "Znimok did not allow «{client}» to use {} ({tool}). The person approves it in \
                     the Znimok window when it asks; without the app running, allow it with \
                     `znimok agents allow \"{client}\" {}`.",
                    scope.name(),
                    scope.name()
                )),
            },
        }
    }

    /// One tool call: switched on? allowed? then run, show activity, write the journal.
    pub fn call(&self, client: &str, name: &str, args: &Value) -> Output {
        let Some(tool) = all().find(|t| t.name == name) else {
            return Output::error(format!("unknown tool «{name}»"));
        };
        let scope = scope_for(tool, args);
        let mut entry = Entry {
            ts: chrono::Utc::now().timestamp_millis(),
            client: client.into(),
            tool: name.into(),
            scope,
            grant: None,
            capture: scope == Some(Scope::Capture) && name != "list_targets",
            document: arg_str(args, "document").map(str::to_string),
            ok: false,
            error: None,
        };
        let out = if !self.enabled {
            Output::error(
                "The Znimok MCP server is switched off. Turn it on in Znimok → Settings → Agents \
                 (or `znimok agents enable`).",
            )
        } else {
            match scope.map(|s| self.authorize(client, s, name)).transpose() {
                Err(e) => Output::error(e),
                Ok(g) => {
                    entry.grant = g.or(Some(Grant::Once));
                    self.gui.activity(client, name, true);
                    let r = self.run(client, name, args);
                    self.gui.activity(client, name, false);
                    r.unwrap_or_else(Output::error)
                }
            }
        };
        entry.ok = !out.is_error;
        if out.is_error {
            entry.error = out
                .content
                .first()
                .and_then(|c| c["text"].as_str())
                .map(str::to_string);
            if entry.grant.is_none() {
                entry.capture = false;
            }
        }
        if let Some(a) = &self.audit {
            a.record(&entry);
        }
        out
    }

    /// Where an export goes: the agent's `path` (the right extension, never over a file), or a
    /// new file in Znimok's export folder.
    fn export_path(&self, args: &Value, doc: &Document, ext: &str) -> Result<PathBuf, String> {
        if let Some(p) = arg_str(args, "path") {
            let p = PathBuf::from(p);
            if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                e.eq_ignore_ascii_case(ext) || (ext == "jpg" && e.eq_ignore_ascii_case("jpeg"))
            }) {
                return Err(format!("the path must end with .{ext}"));
            }
            if p.exists() {
                return Err(format!(
                    "{} exists; Znimok does not overwrite files",
                    p.display()
                ));
            }
            return Ok(p);
        }
        std::fs::create_dir_all(&self.export_dir).map_err(|e| e.to_string())?;
        let stem = znimok_platform::clipfile::file_stem(&doc.name);
        let mut out = self.export_dir.join(format!("{stem}.{ext}"));
        let mut n = 2;
        while out.exists() {
            out = self.export_dir.join(format!("{stem} ({n}).{ext}"));
            n += 1;
        }
        Ok(out)
    }

    pub(crate) fn doc(&self, args: &Value) -> Result<(Document, PathBuf), String> {
        let d = arg_str(args, "document").ok_or("«document» is required")?;
        let path = self
            .lib
            .resolve(d)
            .ok_or_else(|| format!("no document «{d}» in the library"))?;
        Ok((library::load(&path)?, path))
    }

    pub(crate) fn new_doc(
        &self,
        raster: Raster,
        source: &str,
    ) -> Result<(Document, PathBuf), String> {
        let now = chrono::Local::now();
        let tr = znimok_i18n::Localizer::for_system(None);
        let mut a = znimok_i18n::FluentArgs::new();
        a.set("date", now.format("%Y-%m-%d").to_string());
        a.set("time", now.format("%H.%M.%S").to_string());
        let mut doc = Document::from_raster(tr.tr_args("doc-untitled", &a), raster);
        doc.meta.created_ms = now.timestamp_millis();
        doc.meta.source = source.into();
        let path = self.lib.new_path(&doc);
        library::save(&path, &doc)?;
        Ok((doc, path))
    }

    fn shot(&self, target: CaptureTarget, source: &str) -> Result<Output, String> {
        let (doc, path) = match self.capture.take(&target)? {
            Shot::Pixels(r) => self.new_doc(r, source)?,
            Shot::Saved(p) => (library::load(&p)?, p),
        };
        let r = library::render(&doc, 1.0);
        Ok(Output::ok(
            summary(&doc, &path),
            image(&r).into_iter().chain([link(&doc)]).collect(),
        ))
    }

    fn run(&self, client: &str, name: &str, args: &Value) -> Result<Output, String> {
        crate::facade::run(self, client, name, args)
            .unwrap_or_else(|| self.run_inner(client, name, args))
    }

    /// An inner tool by its own name (the facade dispatches here).
    pub(crate) fn run_inner(
        &self,
        client: &str,
        name: &str,
        args: &Value,
    ) -> Result<Output, String> {
        match name {
            "list_displays" => {
                let d: Vec<Value> = self
                    .capture
                    .displays()?
                    .into_iter()
                    .map(|d| {
                        json!({"id": d.id.0, "name": d.name, "bounds": d.bounds,
                                    "scale": d.scale_factor, "primary": d.primary})
                    })
                    .collect();
                Ok(Output::ok(json!({"displays": d}), vec![]))
            }
            "list_windows" => {
                let w: Vec<Value> = self
                    .capture
                    .windows()?
                    .into_iter()
                    .map(|w| json!({"id": w.id.0, "title": w.title, "app": w.app, "bounds": w.bounds}))
                    .collect();
                Ok(Output::ok(json!({"windows": w}), vec![]))
            }
            "capture_screen" => {
                let id = match arg_str(args, "display") {
                    Some(id) => DisplayId(id.into()),
                    None => {
                        self.capture
                            .displays()?
                            .into_iter()
                            .find(|d| d.primary)
                            .ok_or("no display")?
                            .id
                    }
                };
                crate::apptools::delay(args);
                self.shot(CaptureTarget::Display { id }, "screen")
            }
            "capture_window" => {
                let id = u64::try_from(arg_int(args, "window")?)
                    .map_err(|_| "«window» must be an id from list_windows".to_string())?;
                crate::apptools::delay(args);
                self.shot(CaptureTarget::Window { id: WindowId(id) }, "window")
            }
            "capture_region" => {
                let n = |k: &str| arg_int(args, k);
                let (w, h) = (n("width")?, n("height")?);
                if w < 1 || h < 1 {
                    return Err("width and height must be positive".into());
                }
                let rect = Rect {
                    x: n("x")? as i32,
                    y: n("y")? as i32,
                    width: w as u32,
                    height: h as u32,
                };
                crate::apptools::delay(args);
                self.shot(CaptureTarget::Region { rect }, "region")
            }
            "annotate" => {
                let (doc, path) = self.doc(args)?;
                let cmds = args["commands"]
                    .as_array()
                    .ok_or("«commands» must be an array")?;
                let mut ed = Editor::new(doc);
                for (i, c) in cmds.iter().enumerate() {
                    let cmd: Command = serde_json::from_value(c.clone())
                        .map_err(|e| format!("command {i}: {e}"))?;
                    ed.apply(cmd).map_err(|e| format!("command {i}: {e}"))?;
                }
                library::save(&path, &ed.doc)?;
                let r = library::render(&ed.doc, 1.0);
                Ok(Output::ok(
                    summary(&ed.doc, &path),
                    image(&r).into_iter().collect(),
                ))
            }
            "export" => {
                let (doc, src) = self.doc(args)?;
                let f = arg_str(args, "format").ok_or("«format» is required")?;
                // A recording (ZK-243): MP4, GIF, its page or report, or one frame — made here.
                if library::peek_item(&src).is_some_and(|i| i.kind == "video") {
                    let ext = crate::vexport::FORMATS
                        .iter()
                        .find(|(n, _)| *n == f)
                        .map(|(_, e)| *e)
                        .ok_or_else(|| {
                            format!("«{f}» is not a format of a recording: mp4, gif, html, report, zreport, png, jpeg, webp")
                        })?;
                    let out = self.export_path(args, &doc, ext)?;
                    return crate::vexport::export(&src, f, args, &out);
                }
                if !matches!(f, "png" | "jpeg" | "webp" | "html") {
                    return Err(format!(
                        "«{f}» is for a recording; a screenshot exports as png, jpeg, webp or html"
                    ));
                }
                let fmt = ExportFormat::parse(f).ok_or_else(|| format!("unknown format «{f}»"))?;
                let ext = match fmt {
                    ExportFormat::Jpeg => "jpg",
                    _ => f,
                };
                let out = match arg_str(args, "path") {
                    // An agent never overwrites a file and only writes what it says (ZK-113):
                    // library_read must not become «replace any file of the person».
                    Some(p) => {
                        let p = PathBuf::from(p);
                        let ok_ext = p
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| ExportFormat::parse(e) == Some(fmt));
                        if !ok_ext {
                            return Err(format!("the path must end with .{ext} for {f}"));
                        }
                        if p.exists() {
                            return Err(format!(
                                "{} exists; Znimok does not overwrite files",
                                p.display()
                            ));
                        }
                        p
                    }
                    None => {
                        std::fs::create_dir_all(&self.export_dir).map_err(|e| e.to_string())?;
                        self.export_dir.join(format!(
                            "{}.{ext}",
                            znimok_platform::clipfile::file_stem(&doc.name)
                        ))
                    }
                };
                library::export(&doc, fmt, &out)?;
                Ok(Output::ok(
                    json!({"path": out.display().to_string(), "format": f}),
                    vec![],
                ))
            }
            "library_search" => {
                let limit = args["limit"].as_u64().unwrap_or(20).min(200) as usize;
                let day = |k: &str| -> Result<Option<i64>, String> {
                    let Some(s) = arg_str(args, k) else {
                        return Ok(None);
                    };
                    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                        .map_err(|_| format!("«{k}» must be a day, YYYY-MM-DD"))?;
                    Ok(d.and_hms_opt(0, 0, 0)
                        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
                        .map(|t| t.timestamp_millis()))
                };
                let items = self.lib.find(
                    &library::Filter {
                        query: arg_str(args, "query").unwrap_or("").to_string(),
                        tag: arg_str(args, "tag").map(str::to_string),
                        kind: arg_str(args, "kind").map(str::to_string),
                        pinned: args["pinned"].as_bool(),
                        has_log: args["has_log"].as_bool(),
                        since_ms: day("since")?,
                        until_ms: day("until")?,
                        trash: args["trash"].as_bool().unwrap_or(false),
                    },
                    limit,
                );
                let mut out = json!({"documents": items});
                if args["with_tags"].as_bool() == Some(true) {
                    out["tags"] = self
                        .run_inner(client, "library_tags", &json!({}))?
                        .structured
                        .unwrap_or_default()["tags"]
                        .take();
                }
                Ok(Output::ok(out, vec![]))
            }
            "library_get" => {
                let (doc, path) = self.doc(args)?;
                let r = library::render(&doc, 1.0);
                let mut s = summary(&doc, &path);
                s["description"] = json!(doc.meta.description);
                s["created_ms"] = json!(doc.meta.created_ms);
                Ok(Output::ok(
                    s,
                    image(&r).into_iter().chain([link(&doc)]).collect(),
                ))
            }
            "ocr" => {
                let (doc, _) = self.doc(args)?;
                let langs: Vec<String> = args["languages"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let langs: Vec<&str> = langs.iter().map(String::as_str).collect();
                let r = library::render(&doc, 1.0);
                let img = Rgba::new(r.width, r.height, r.rgba).ok_or("empty picture")?;
                let engine =
                    znimok_models::ocr::system().ok_or("no text recognition on this system")?;
                let res = engine.recognize(&img, &langs).map_err(|e| e.to_string())?;
                let lines: Vec<Value> = res
                    .lines
                    .iter()
                    .map(|l| json!({"text": l.text, "x": l.rect.x, "y": l.rect.y, "w": l.rect.w, "h": l.rect.h}))
                    .collect();
                Ok(Output::ok(
                    json!({"text": res.text(), "lines": lines, "languages": res.languages, "missing_languages": res.missing}),
                    vec![],
                ))
            }
            "read_codes" => {
                let region = ["x", "y", "width", "height"]
                    .map(|k| args.get(k).is_some_and(|v| !v.is_null()));
                let (raster, from) = if arg_str(args, "document").is_some() {
                    if region.contains(&true) || arg_str(args, "display").is_some() {
                        return Err(
                            "give either «document» or a place on the screen, not both".into()
                        );
                    }
                    let (doc, _) = self.doc(args)?;
                    let from = json!({"document": doc.id.to_string()});
                    (library::render(&doc, 1.0), from)
                } else {
                    let target = if region.iter().all(|r| *r) {
                        let n = |k: &str| arg_int(args, k);
                        let (w, h) = (n("width")?, n("height")?);
                        if w < 1 || h < 1 {
                            return Err("width and height must be positive".into());
                        }
                        CaptureTarget::Region {
                            rect: Rect {
                                x: n("x")? as i32,
                                y: n("y")? as i32,
                                width: w as u32,
                                height: h as u32,
                            },
                        }
                    } else if region.contains(&true) {
                        return Err("a region needs all of «x», «y», «width» and «height»".into());
                    } else {
                        let id = match arg_str(args, "display") {
                            Some(id) => DisplayId(id.into()),
                            None => {
                                self.capture
                                    .displays()?
                                    .into_iter()
                                    .find(|d| d.primary)
                                    .ok_or("no display")?
                                    .id
                            }
                        };
                        CaptureTarget::Display { id }
                    };
                    match self.capture.take(&target)? {
                        Shot::Pixels(r) => (r, json!({"screen": true})),
                        // macOS: the app took the shot and keeps it in the library.
                        Shot::Saved(p) => {
                            let doc = library::load(&p)?;
                            let from = json!({"screen": true, "saved_as": doc.id.to_string()});
                            (library::render(&doc, 1.0), from)
                        }
                    }
                };
                let codes = znimok_codes::read(raster.width, raster.height, &raster.rgba);
                let mut v = json!({"codes": codes.iter().map(code_json).collect::<Vec<_>>()});
                if let (Some(o), Some(f)) = (v.as_object_mut(), from.as_object()) {
                    o.extend(f.clone());
                }
                Ok(Output::ok(v, vec![]))
            }
            "redact_pii" => {
                let (doc, path) = self.doc(args)?;
                let apply = args["apply"].as_bool().unwrap_or(true);
                let r = library::render(&doc, 1.0);
                let img = Rgba::new(r.width, r.height, r.rgba.clone()).ok_or("empty picture")?;
                // Both readings: the masking one reads Latin (e-mail, keys) better (ZK-120).
                let text = znimok_models::ocr::system()
                    .and_then(|o| znimok_models::ocr::read_for_masking(o.as_ref(), &img));
                let faces = if args["faces"].as_bool().unwrap_or(true) {
                    znimok_models::faces::detect(&img).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let found = znimok_mask::suggest(text.as_ref(), &faces, r.width, r.height);
                let listed: Vec<Value> = found
                    .iter()
                    .map(|s| json!({"kind": s.kind, "rect": s.rect, "preview": s.preview}))
                    .collect();
                if !apply || found.is_empty() {
                    return Ok(Output::ok(
                        json!({"found": listed, "applied": false}),
                        vec![],
                    ));
                }
                let mut ed = Editor::new(doc);
                for c in znimok_mask::commands(&found, None) {
                    ed.apply(c).map_err(|e| e.to_string())?;
                }
                library::save(&path, &ed.doc)?;
                let r = library::render(&ed.doc, 1.0);
                Ok(Output::ok(
                    json!({"found": listed, "applied": true, "document": summary(&ed.doc, &path)}),
                    image(&r).into_iter().collect(),
                ))
            }
            _ => crate::edit::run(self, name, args)
                .or_else(|| crate::libtools::run(self, client, name, args))
                .or_else(|| crate::vidtools::run(self, name, args))
                .or_else(|| crate::rectools::run(self, client, name, args))
                .or_else(|| crate::apptools::run(self, name, args))
                .unwrap_or_else(|| Err(format!("unknown tool «{name}»"))),
        }
    }

    /// `resources/list`: the library documents (needs library_read).
    pub fn resources(&self, client: &str) -> Result<Vec<Value>, String> {
        self.authorize(client, Scope::LibraryRead, "resources/list")?;
        Ok(self
            .lib
            .items()
            .into_iter()
            .flat_map(|i| {
                let mut v = vec![json!({"uri": format!("znimok://library/{}", i.id), "name": i.name,
                       "mimeType": "image/png", "description": i.description})];
                if i.has_log {
                    v.push(json!({"uri": format!("znimok://library/{}/log", i.id),
                        "name": format!("{} — DevTools log", i.name), "mimeType": "application/json",
                        "description": "The browser's DevTools log of the recording"}));
                }
                v
            })
            .collect())
    }

    /// `resources/read`: the document rendered as PNG.
    pub fn read_resource(&self, client: &str, uri: &str) -> Result<Value, String> {
        self.authorize(client, Scope::LibraryRead, "resources/read")?;
        let id = uri
            .strip_prefix("znimok://library/")
            .ok_or("unknown resource")?;
        // A recording's browser log (ZK-235), as JSON.
        if let Some(id) = id.strip_suffix("/log") {
            let path = self.lib.resolve(id).ok_or("unknown resource")?;
            let (_, part) = znimok_format::open_parts(&path).map_err(|e| e.to_string())?;
            let part = part.ok_or("unknown resource")?;
            let (events, _) = crate::vidtools::events(&part);
            return Ok(json!({"uri": uri, "mimeType": "application/json",
                             "text": serde_json::to_string(&events).unwrap_or_default()}));
        }
        let path = self.lib.resolve(id).ok_or("unknown resource")?;
        let doc = library::load(&path)?;
        let png = library::encode_png(&library::render(&doc, 1.0))?;
        use base64::Engine;
        Ok(json!({"uri": uri, "mimeType": "image/png",
                  "blob": base64::engine::general_purpose::STANDARD.encode(png)}))
    }
}

#[cfg(test)]
mod arg_tests {
    use super::arg_int;
    use serde_json::json;

    /// Missing, wrong type and a number sent as a string are told apart (ZK-123).
    #[test]
    fn whole_numbers_and_their_errors() {
        assert_eq!(arg_int(&json!({"window": 197496}), "window"), Ok(197496));
        assert_eq!(arg_int(&json!({"window": "197496"}), "window"), Ok(197496));
        assert_eq!(arg_int(&json!({"x": -40.0}), "x"), Ok(-40));
        assert_eq!(
            arg_int(&json!({}), "window"),
            Err("«window» is required".into())
        );
        assert_eq!(
            arg_int(&json!({"window": null}), "window"),
            Err("«window» is required".into())
        );
        let e = arg_int(&json!({"window": "Explorer"}), "window").unwrap_err();
        assert!(e.contains("must be a whole number"), "{e}");
        assert!(arg_int(&json!({"x": 1.5}), "x").is_err());
    }
}
