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
    fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": msg.into()})],
            structured: None,
            is_error: true,
        }
    }

    fn ok(structured: Value, mut extra: Vec<Value>) -> Self {
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

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    scope: Option<Scope>,
    read_only: bool,
    schema: fn() -> Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn doc_arg() -> Value {
    json!({"type": "string", "description": "Library document: its id (or the first 8+ characters of it) from library_search, or a path to a .znimok file"})
}

const TOOLS: &[Tool] = &[
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
                json!({"display": {"type": "string", "description": "Display id from list_displays"}}),
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
                json!({"window": {"type": "integer", "description": "Window id from list_windows"}}),
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
                    "width": {"type": "integer", "minimum": 1}, "height": {"type": "integer", "minimum": 1}
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
        description: "Writes the document with its marks as PNG, JPEG, WebP or one self-contained HTML page.",
        scope: Some(Scope::LibraryRead),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "format": {"type": "string", "enum": ["png", "jpeg", "webp", "html"]},
                    "path": {"type": "string", "description": "Output file; default: a file in Znimok's export folder"}
                }),
                &["document", "format"],
            )
        },
    },
    Tool {
        name: "library_search",
        title: "Search the library",
        description: "Library documents, newest first, matching the text in name, description or tags.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || {
            obj(
                json!({
                    "query": {"type": "string"}, "tag": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200}
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

/// `tools/list` entries, in a fixed order.
pub fn list() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "title": t.title,
                    "readOnlyHint": t.read_only,
                    "destructiveHint": false,
                    "openWorldHint": false
                }
            })
        })
        .collect()
}

/// A picture for the client: scaled to what models take, as PNG.
fn image(r: &Raster) -> Option<Value> {
    let img = Rgba::new(r.width, r.height, r.rgba.clone())?;
    let p = znimok_models::image_prep::prepare(&img).ok()?;
    Some(json!({"type": "image", "data": p.base64, "mimeType": p.media_type}))
}

fn link(doc: &Document) -> Value {
    json!({
        "type": "resource_link",
        "uri": format!("znimok://library/{}", doc.id),
        "name": doc.name,
        "mimeType": DOC_MIME,
        "description": "The library document (original pixels and marks)"
    })
}

fn summary(doc: &Document, path: &std::path::Path) -> Value {
    let f = doc.frame();
    json!({
        "id": doc.id.to_string(),
        "name": doc.name,
        "path": path.display().to_string(),
        "width": f.w, "height": f.h,
        "marks": doc.objects.len(),
        "source": doc.meta.source,
        "tags": doc.meta.tags,
    })
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
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

fn arg_int(a: &Value, k: &str) -> Result<i64, String> {
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
    fn authorize(&self, client: &str, scope: Scope, tool: &str) -> Result<Grant, String> {
        match self.perms.check(client, scope) {
            Decision::Allowed(g) => Ok(g),
            Decision::Ask => match self.gui.ask(client, scope, tool) {
                Some(g) => {
                    let _ = self.perms.grant(client, scope, g);
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
        let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
            return Output::error(format!("unknown tool «{name}»"));
        };
        let scope = scope_for(tool, args);
        let mut entry = Entry {
            ts: chrono::Utc::now().timestamp_millis(),
            client: client.into(),
            tool: name.into(),
            scope,
            grant: None,
            capture: scope == Some(Scope::Capture) && name != "list_windows",
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
                    let r = self.run(name, args);
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

    fn doc(&self, args: &Value) -> Result<(Document, PathBuf), String> {
        let d = arg_str(args, "document").ok_or("«document» is required")?;
        let path = self
            .lib
            .resolve(d)
            .ok_or_else(|| format!("no document «{d}» in the library"))?;
        Ok((library::load(&path)?, path))
    }

    fn new_doc(&self, raster: Raster, source: &str) -> Result<(Document, PathBuf), String> {
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

    fn run(&self, name: &str, args: &Value) -> Result<Output, String> {
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
                self.shot(CaptureTarget::Display { id }, "screen")
            }
            "capture_window" => {
                let id = u64::try_from(arg_int(args, "window")?)
                    .map_err(|_| "«window» must be an id from list_windows".to_string())?;
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
                let (doc, _) = self.doc(args)?;
                let f = arg_str(args, "format").ok_or("«format» is required")?;
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
                let items = self.lib.search(
                    arg_str(args, "query").unwrap_or(""),
                    arg_str(args, "tag"),
                    limit,
                );
                Ok(Output::ok(json!({"documents": items}), vec![]))
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
            _ => Err(format!("unknown tool «{name}»")),
        }
    }

    /// `resources/list`: the library documents (needs library_read).
    pub fn resources(&self, client: &str) -> Result<Vec<Value>, String> {
        self.authorize(client, Scope::LibraryRead, "resources/list")?;
        Ok(self
            .lib
            .items()
            .into_iter()
            .map(|i| {
                json!({"uri": format!("znimok://library/{}", i.id), "name": i.name,
                       "mimeType": "image/png", "description": i.description})
            })
            .collect())
    }

    /// `resources/read`: the document rendered as PNG.
    pub fn read_resource(&self, client: &str, uri: &str) -> Result<Value, String> {
        self.authorize(client, Scope::LibraryRead, "resources/read")?;
        let id = uri
            .strip_prefix("znimok://library/")
            .ok_or("unknown resource")?;
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
