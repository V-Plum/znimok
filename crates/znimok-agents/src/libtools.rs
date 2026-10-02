//! The library tools (ZK-234): a document's name and meta, a picture brought in from a file, the
//! trash, copies, the tags. The trash needs no question — it is undone with `library_restore`;
//! deleting for good is confirmed by the person in the Znimok window every time.

use crate::edit::apply;
use crate::library;
use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, arg_str, doc_arg, image, link, obj, summary};
use serde_json::{Value, json};
use std::path::PathBuf;
use znimok_core::{Command, Document, Raster};

fn strings(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": what})
}

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "set_meta",
        title: "Name, description, tags",
        description: "Changes what a library document is called and what is written about it: name, description, tags (replace them all, or add / remove some), author, copyright, and whether it is pinned in the library. Only what is given changes.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "document": doc_arg(),
                    "name": {"type": "string"},
                    "description": {"type": "string"},
                    "tags": strings("The whole list of tags (replaces what was there)"),
                    "add_tags": strings("Tags to add"),
                    "remove_tags": strings("Tags to take away"),
                    "author": {"type": "string"},
                    "copyright": {"type": "string"},
                    "pinned": {"type": "boolean"}
                }),
                &["document"],
            )
        },
    },
    Tool {
        name: "library_import",
        title: "Bring a file into the library",
        description: "Adds a picture file (PNG, JPEG, WebP) or a .znimok document from anywhere on this computer to the library as a new document; returns it with its picture.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({
                    "path": {"type": "string", "description": "The file to bring in"},
                    "name": {"type": "string", "description": "Name of the new document (the file's name by default)"}
                }),
                &["path"],
            )
        },
    },
    Tool {
        name: "library_duplicate",
        title: "Copy a document",
        description: "Makes a copy of a library document (a screenshot or a recording, with its marks) as a new document — to try changes without touching the original.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || {
            obj(
                json!({"document": doc_arg(), "name": {"type": "string", "description": "Name of the copy"}}),
                &["document"],
            )
        },
    },
    Tool {
        name: "library_trash",
        title: "Move to the trash",
        description: "Moves a library document to Znimok's trash. It stays there for the days set in Znimok (7 by default) and comes back with library_restore.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "library_restore",
        title: "Bring back from the trash",
        description: "Brings a document back from Znimok's trash into the library (its id from library_trash, or from library_search with trash=true).",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "library_delete",
        title: "Delete for good",
        description: "Deletes a document for good, from the library or from the trash. The person confirms it in the Znimok window every time; without the app running it is refused. Prefer library_trash.",
        scope: Some(Scope::LibraryWrite),
        read_only: false,
        schema: || obj(json!({"document": doc_arg()}), &["document"]),
    },
    Tool {
        name: "library_tags",
        title: "The tags of the library",
        description: "Every tag used in the library with the number of documents that carry it, the most used first.",
        scope: Some(Scope::LibraryRead),
        read_only: true,
        schema: || obj(json!({}), &[]),
    },
];

/// Tools that take something away for good.
pub(crate) fn destructive(name: &str) -> bool {
    name == "library_delete"
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn list(args: &Value, k: &str) -> Option<Vec<String>> {
    args.get(k).and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect()
    })
}

fn shown(agent: &Agent, doc: &Document, path: &std::path::Path) -> Output {
    let _ = agent;
    let r = library::render(doc, 1.0);
    Output::ok(
        summary(doc, path),
        image(&r).into_iter().chain([link(doc)]).collect(),
    )
}

/// `None`: not a library tool.
pub(crate) fn run(
    agent: &Agent,
    client: &str,
    name: &str,
    args: &Value,
) -> Option<Result<Output, String>> {
    Some(match name {
        "set_meta" => apply(agent, args, |doc| {
            let mut out = Vec::new();
            if let Some(n) = arg_str(args, "name") {
                let n = n.trim();
                if n.is_empty() {
                    return Err("«name» is empty".into());
                }
                if n != doc.name {
                    out.push(Command::SetName {
                        name: n.to_string(),
                    });
                }
            }
            let mut meta = doc.meta.clone();
            if let Some(d) = arg_str(args, "description") {
                meta.description = d.to_string();
            }
            if let Some(a) = arg_str(args, "author") {
                meta.author = a.to_string();
            }
            if let Some(c) = arg_str(args, "copyright") {
                meta.copyright = c.to_string();
            }
            if let Some(t) = list(args, "tags") {
                meta.tags = t;
            }
            for t in list(args, "add_tags").unwrap_or_default() {
                if !meta.tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                    meta.tags.push(t);
                }
            }
            if let Some(gone) = list(args, "remove_tags") {
                meta.tags
                    .retain(|x| !gone.iter().any(|g| g.eq_ignore_ascii_case(x)));
            }
            if let Some(p) = args["pinned"].as_bool() {
                meta.pinned = p;
            }
            if meta != doc.meta {
                out.push(Command::SetMeta { meta });
            }
            if out.is_empty() {
                return Err("nothing to change: give a name, a description, tags…".into());
            }
            Ok(out)
        }),
        "library_import" => (|| {
            let p = PathBuf::from(arg_str(args, "path").ok_or("«path» is required")?);
            if !p.is_file() {
                return Err(format!("{}: no such file", p.display()));
            }
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let (mut doc, video) = if ext == "znimok" {
                znimok_format::open_parts(&p).map_err(|e| format!("{}: {e}", p.display()))?
            } else {
                let img = image::open(&p)
                    .map_err(|e| format!("{}: {e} (PNG, JPEG and WebP are read)", p.display()))?
                    .to_rgba8();
                let (w, h) = img.dimensions();
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut doc = Document::from_raster(stem, Raster::new(w, h, img.into_raw()));
                doc.meta.created_ms = now_ms();
                doc.meta.source = "file".into();
                (doc, None)
            };
            if let Some(n) = arg_str(args, "name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                doc.name = n.to_string();
            }
            // The same document brought in twice is two documents.
            let id = doc.id.to_string();
            if agent.lib.items().iter().any(|i| i.id == id) {
                doc.id = uuid::Uuid::new_v4();
            }
            let path = agent.lib.new_path(&doc);
            library::save_parts(&path, &doc, video.as_ref())?;
            Ok(shown(agent, &doc, &path))
        })(),
        "library_duplicate" => (|| {
            let d = arg_str(args, "document").ok_or("«document» is required")?;
            let src = agent
                .lib
                .resolve(d)
                .ok_or_else(|| format!("no document «{d}» in the library"))?;
            let (mut doc, video) =
                znimok_format::open_parts(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            doc.id = uuid::Uuid::new_v4();
            doc.name = match arg_str(args, "name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                Some(n) => n.to_string(),
                None => {
                    let tr = znimok_i18n::Localizer::for_system(None);
                    let mut a = znimok_i18n::FluentArgs::new();
                    a.set("name", doc.name.clone());
                    tr.tr_args("doc-copy-name", &a)
                }
            };
            doc.meta.created_ms = now_ms();
            doc.meta.pinned = false;
            let path = agent.lib.new_path(&doc);
            library::save_parts(&path, &doc, video.as_ref())?;
            Ok(shown(agent, &doc, &path))
        })(),
        "library_trash" => (|| {
            let d = arg_str(args, "document").ok_or("«document» is required")?;
            let path = agent
                .lib
                .resolve(d)
                .ok_or_else(|| format!("no document «{d}» in the library"))?;
            let item = library::peek_item(&path);
            let to = agent.lib.move_to_trash(&path).map_err(|e| e.to_string())?;
            Ok(Output::ok(
                json!({
                    "trashed": true,
                    "id": item.as_ref().map(|i| i.id.clone()),
                    "name": item.as_ref().map(|i| i.name.clone()),
                    "path": to.display().to_string(),
                }),
                vec![],
            ))
        })(),
        "library_restore" => (|| {
            let d = arg_str(args, "document").ok_or("«document» is required")?;
            let trashed = agent.lib.resolve_trashed(d).ok_or_else(|| {
                format!("no document «{d}» in the trash (library_search with trash=true lists it)")
            })?;
            let path = agent.lib.restore(&trashed).map_err(|e| e.to_string())?;
            let doc = library::load(&path)?;
            Ok(shown(agent, &doc, &path))
        })(),
        "library_delete" => (|| {
            let d = arg_str(args, "document").ok_or("«document» is required")?;
            let path = agent
                .lib
                .resolve(d)
                .or_else(|| agent.lib.resolve_trashed(d))
                .ok_or_else(|| format!("no document «{d}» in the library or its trash"))?;
            let name = library::peek_item(&path)
                .map(|i| i.name)
                .unwrap_or_default();
            // For good: the person says yes in the Znimok window, this time and every time —
            // a lasting permission does not cover it (the owner's decision, ZK-232).
            if !agent.gui.confirm_delete(client, "library_delete", &name) {
                return Err(format!(
                    "Deleting «{name}» for good was not confirmed in the Znimok window (the app must be running). \
                     library_trash moves it to the trash without a question."
                ));
            }
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Output::ok(json!({"deleted": true, "name": name}), vec![]))
        })(),
        "library_tags" => {
            let mut count: std::collections::BTreeMap<String, usize> = Default::default();
            for i in agent.lib.items() {
                for t in i.tags {
                    *count.entry(t).or_default() += 1;
                }
            }
            let mut tags: Vec<(String, usize)> = count.into_iter().collect();
            tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            Ok(Output::ok(
                json!({"tags": tags.iter().map(|(t, n)| json!({"tag": t, "documents": n})).collect::<Vec<_>>()}),
                vec![],
            ))
        }
        _ => return None,
    })
}
