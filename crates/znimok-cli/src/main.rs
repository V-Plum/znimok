//! `znimok` — the command line of Znimok. Everything here goes through the same layers as the
//! app: `znimok-format` to read and write documents, `znimok-core` commands to change them,
//! `znimok-render` to produce pixels. No window, no GPU — usable by scripts and agents.
//!
//! Exit codes (stable, documented in `--help`):
//!   0 success · 2 wrong usage · 3 file cannot be read or written · 4 not a Znimok document ·
//!   5 made by a newer Znimok · 6 document is damaged · 7 a command was rejected.

use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use znimok_core::{Document, Editor, Raster};
use znimok_format::{FormatError, WriteOptions};
use znimok_render::vello_cpu::Pixmap;
use znimok_render::{Renderer, View};

const AFTER_HELP: &str = "Exit codes: 0 success, 2 wrong usage, 3 file cannot be read or written, \
4 not a Znimok document, 5 made by a newer Znimok, 6 document is damaged, 7 a command was rejected.\n\
Commands for `apply` and queries for `query` follow `znimok schema command` / `znimok schema query`.";

#[derive(Parser)]
#[command(name = "znimok", version, about = "Znimok documents from the command line", after_help = AFTER_HELP)]
struct Cli {
    /// Print results as JSON (for scripts and agents).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Name, size, marks and metadata of a document (reads only its head).
    Info { file: PathBuf },
    /// Creates a document from a PNG, JPEG or WebP picture.
    New {
        image: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// QR codes and barcodes on a document (as rendered) or a PNG / JPEG / WebP picture, read on
    /// this device (ZK-119).
    Codes { file: PathBuf },
    /// Renders the document with its marks to a PNG (the crop, 1:1 unless --scale).
    Render {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Exports to PNG, JPEG, WebP or a self-contained HTML page; the format comes from --format
    /// or the file extension.
    Export {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// JPEG quality 1–100 (transparent areas become white).
        #[arg(long, default_value_t = 92)]
        quality: u8,
    },
    /// Applies commands (JSON, see `znimok schema command`) and saves the document.
    Apply {
        file: PathBuf,
        /// A command; repeatable. Applied in order.
        #[arg(short = 'c', long = "cmd")]
        commands: Vec<String>,
        /// Also read one command per line from standard input.
        #[arg(long)]
        stdin: bool,
        /// Save to another file instead of in place.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Answers a query (JSON, see `znimok schema query`).
    Query { file: PathBuf, query: String },
    /// Lists the documents of a library folder.
    Library {
        #[command(subcommand)]
        cmd: LibraryCmd,
    },
    /// Prints the JSON Schema of commands or queries.
    Schema {
        #[arg(value_enum, default_value_t = SchemaKind::Command)]
        kind: SchemaKind,
    },
    /// «Передати агенту»: the screenshot (a copy with secrets hidden), its context and a brief,
    /// to Claude Code in a new terminal or onto the clipboard.
    Handoff {
        file: PathBuf,
        /// What you want from the agent.
        #[arg(long, default_value = "")]
        note: String,
        /// claude (default: from the settings) or clipboard.
        #[arg(long)]
        to: Option<String>,
        /// Hand over the picture as it is, without hiding secrets.
        #[arg(long)]
        no_redact: bool,
        /// Prepare the folder and print what would be run, without running it.
        #[arg(long)]
        dry_run: bool,
    },
    /// MCP server for AI agents on standard input/output (e.g. `claude mcp add znimok -- znimok mcp`).
    Mcp,
    /// What AI agents may do: switch MCP on/off, list, allow and revoke permissions, the journal.
    Agents {
        #[command(subcommand)]
        cmd: AgentsCmd,
    },
    /// Updates from GitHub releases: the signature of the checksums is verified with the key
    /// built into Znimok before anything is downloaded (ZK-122).
    Update {
        #[command(subcommand)]
        cmd: UpdateCmd,
    },
}

#[derive(Subcommand)]
enum UpdateCmd {
    /// Whether a newer release exists (asks GitHub once; nothing is downloaded).
    Check,
    /// Downloads the newer release's installer into the user's own folder and verifies it
    /// (signature, then checksum); prints its path. Installing is a separate step.
    Download,
    /// Windows: installs a downloaded update after the app exits, checks that the new version
    /// starts and goes back to the previous one if it does not. Runs from a copy of itself.
    Install {
        /// The downloaded installer (in the updates folder).
        #[arg(long)]
        msi: PathBuf,
        /// Its version (the app confirms the start of this version).
        #[arg(long)]
        version: String,
        /// Wait for this process (the app) to exit first.
        #[arg(long)]
        wait_pid: Option<u32>,
        /// The app to start after installing; default: znimok-app.exe next to this program.
        #[arg(long)]
        app: Option<PathBuf>,
        /// Arguments for the app (repeatable).
        #[arg(long = "app-arg")]
        app_args: Vec<String>,
        /// Internal: this process is the copy outside the install folder.
        #[arg(long, hide = true)]
        as_runner: bool,
    },
    /// The app has started: confirms a running update of VERSION (the app does this itself).
    MarkStarted { version: String },
}

#[derive(Subcommand)]
enum AgentsCmd {
    /// Clients with lasting permissions and whether MCP is on.
    List,
    /// Switches the MCP server on.
    Enable,
    /// Switches the MCP server off (tools refuse).
    Disable,
    /// Allows a client a scope for good: capture, library_read, library_write, settings.
    Allow { client: String, scopes: Vec<String> },
    /// Takes back everything from one client, or from all with --all.
    Revoke {
        client: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// The latest entries of the agents' journal.
    Log {
        #[arg(long, default_value_t = 20)]
        last: usize,
    },
}

#[derive(Subcommand)]
enum LibraryCmd {
    /// Documents in the folder (default: $ZNIMOK_LIBRARY, else the standard library folder).
    List {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Png,
    Jpeg,
    Webp,
    /// One self-contained page: the picture with annotations, their list and the metadata.
    Html,
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaKind {
    Command,
    Query,
}

/// An error with its exit code.
struct Fail(u8, String);

impl From<FormatError> for Fail {
    fn from(e: FormatError) -> Self {
        let code = match e {
            FormatError::NotZnimok => 4,
            FormatError::TooNew { .. } => 5,
            FormatError::Corrupt(_) => 6,
            FormatError::Io(_) => 3,
        };
        Fail(code, e.to_string())
    }
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> Fail + '_ {
    move |e| Fail(3, format!("{}: {e}", path.display()))
}

fn load(path: &Path) -> Result<Document, Fail> {
    Ok(load_parts(path)?.0)
}

/// The document and, for a video, what it takes to write it back as a video (ZK-145).
fn load_parts(path: &Path) -> Result<(Document, Option<znimok_format::VideoPart>), Fail> {
    if !path.exists() {
        return Err(Fail(3, format!("{}: no such file", path.display())));
    }
    Ok(znimok_format::open_parts(path)?)
}

fn render(doc: &Document, scale: f64) -> Pixmap {
    let mut view = View::one_to_one(doc);
    if (scale - 1.0).abs() > 1e-9 {
        let f = doc.frame();
        view.scale = scale;
        view.width = ((f.w as f64 * scale).round() as i64).clamp(1, 65535) as u16;
        view.height = ((f.h as f64 * scale).round() as i64).clamp(1, 65535) as u16;
    }
    let mut pix = Pixmap::new(1, 1);
    Renderer::new().render(doc, view, &mut pix);
    pix
}

/// Thumbnail for the library (≤ 320×240), rendered like the document itself.
fn thumbnail(doc: &Document) -> Raster {
    let f = doc.frame();
    let s = (320.0 / f.w as f64).min(240.0 / f.h as f64).min(1.0);
    let pix = render(doc, s);
    Raster::new(
        pix.width() as u32,
        pix.height() as u32,
        znimok_render::pixmap_to_rgba(&pix),
    )
}

/// Writes the document as the kind it was opened as (a video stays a video, ZK-145).
fn save(path: &Path, doc: &Document, video: Option<&znimok_format::VideoPart>) -> Result<(), Fail> {
    let opts = WriteOptions {
        app_version: format!("znimok CLI {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(thumbnail(doc)),
        ..Default::default()
    };
    znimok_format::save_same_kind(path, doc, video, &opts)?;
    Ok(())
}

fn write_image(
    path: &Path,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    format: Format,
    quality: u8,
) -> Result<(), Fail> {
    let img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| Fail(3, "image buffer size mismatch".into()))?;
    let file = std::fs::File::create(path).map_err(io(path))?;
    let mut out = std::io::BufWriter::new(file);
    let enc_err = |e: image::ImageError| Fail(3, format!("{}: {e}", path.display()));
    match format {
        Format::Png => image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(enc_err)?,
        Format::Webp => image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::WebP)
            .map_err(enc_err)?,
        Format::Html => return Err(Fail(2, "HTML is written by write_html".into())),
        Format::Jpeg => {
            // No alpha in JPEG: composite over white (Little Helpers rule), quality as asked.
            let rgb = image::RgbImage::from_fn(w, h, |x, y| {
                let p = img.get_pixel(x, y).0;
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
            });
            let enc =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            rgb.write_with_encoder(enc).map_err(enc_err)?;
        }
    }
    out.flush().map_err(io(path))
}

/// The self-contained page (ZK-66) in the system language.
fn write_html(path: &Path, doc: &Document, w: u32, h: u32, rgba: Vec<u8>) -> Result<(), Fail> {
    let img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| Fail(3, "image buffer size mismatch".into()))?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| Fail(3, format!("{}: {e}", path.display())))?;
    let tr = znimok_i18n::Localizer::for_system(None);
    let html = znimok_html::page(doc, png.get_ref(), w, h, &tr);
    std::fs::write(path, html).map_err(io(path))
}

fn format_for(path: &Path, explicit: Option<Format>) -> Result<Format, Fail> {
    if let Some(f) = explicit {
        return Ok(f);
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok(Format::Png),
        Some("jpg" | "jpeg") => Ok(Format::Jpeg),
        Some("webp") => Ok(Format::Webp),
        Some("html" | "htm") => Ok(Format::Html),
        _ => Err(Fail(
            2,
            format!("cannot tell the format of {}; use --format", path.display()),
        )),
    }
}

fn default_library() -> PathBuf {
    if let Some(d) = std::env::var_os("ZNIMOK_LIBRARY") {
        return d.into();
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").unwrap_or_default();
        return PathBuf::from(home).join("Library/Application Support/Znimok/Library");
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(base).join("Znimok").join("Library")
}

fn peek_json(path: &Path, p: &znimok_format::Peek, size: u64) -> serde_json::Value {
    json!({
        "path": path.display().to_string(),
        "id": p.id.map(|u| u.to_string()),
        "name": p.name,
        "created_ms": p.meta.created_ms,
        "source": p.meta.source,
        "width": p.width,
        "height": p.height,
        "objects": p.object_count,
        "has_thumbnail": p.thumbnail_png.is_some(),
        "tags": p.meta.tags,
        "description": p.meta.description,
        "app_version": p.app_version,
        "bytes": size,
    })
}

fn run(cli: Cli) -> Result<(), Fail> {
    let out = |v: serde_json::Value, text: String| {
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        } else {
            println!("{text}");
        }
    };
    match cli.cmd {
        Cmd::Info { file } => {
            let bytes = std::fs::read(&file).map_err(io(&file))?;
            let p = znimok_format::peek(&bytes)?;
            let v = peek_json(&file, &p, bytes.len() as u64);
            let text = format!(
                "{}\n  {}×{} px, {} marks, {} bytes\n  id {}\n  source: {}, created {} ms UTC{}",
                if p.name.is_empty() {
                    "(untitled)"
                } else {
                    &p.name
                },
                p.width,
                p.height,
                p.object_count,
                bytes.len(),
                p.id.map(|u| u.to_string()).unwrap_or_default(),
                if p.meta.source.is_empty() {
                    "—"
                } else {
                    &p.meta.source
                },
                p.meta.created_ms,
                if p.meta.tags.is_empty() {
                    String::new()
                } else {
                    format!("\n  tags: {}", p.meta.tags.join(", "))
                }
            );
            out(v, text);
        }
        Cmd::New {
            image,
            output,
            name,
        } => {
            let img = image::open(&image)
                .map_err(|e| Fail(3, format!("{}: {e}", image.display())))?
                .to_rgba8();
            let (w, h) = img.dimensions();
            let name = name.unwrap_or_else(|| {
                image
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let mut doc = Document::from_raster(name, Raster::new(w, h, img.into_raw()));
            doc.meta.source = "file".into();
            doc.meta.created_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            save(&output, &doc, None)?;
            out(
                json!({ "output": output.display().to_string(), "id": doc.id.to_string(), "width": w, "height": h }),
                format!("created {} ({w}×{h})", output.display()),
            );
        }
        Cmd::Codes { file } => {
            let (w, h, rgba) =
                if znimok_format::is_znimok(&std::fs::read(&file).map_err(io(&file))?) {
                    let pix = render(&load(&file)?, 1.0);
                    (
                        pix.width() as u32,
                        pix.height() as u32,
                        znimok_render::pixmap_to_rgba(&pix),
                    )
                } else {
                    let img = image::open(&file)
                        .map_err(|e| Fail(3, format!("{}: {e}", file.display())))?
                        .to_rgba8();
                    let (w, h) = img.dimensions();
                    (w, h, img.into_raw())
                };
            let codes = znimok_codes::read(w, h, &rgba);
            let list: Vec<serde_json::Value> = codes
                .iter()
                .map(|c| {
                    let kind = match &c.kind {
                        znimok_codes::Kind::Link(_) => "link",
                        znimok_codes::Kind::Wifi { .. } => "wifi",
                        znimok_codes::Kind::Contact => "contact",
                        znimok_codes::Kind::Event => "event",
                        znimok_codes::Kind::Email(_) => "email",
                        znimok_codes::Kind::Phone(_) => "phone",
                        znimok_codes::Kind::Text => "text",
                    };
                    json!({
                        "format": c.format,
                        "kind": kind,
                        "text": c.text,
                        "bounds": [c.bounds.0, c.bounds.1, c.bounds.2, c.bounds.3],
                    })
                })
                .collect();
            let text = if codes.is_empty() {
                "no codes".to_string()
            } else {
                codes
                    .iter()
                    .map(|c| format!("{}\t{}", c.format, c.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            out(json!({ "codes": list }), text);
        }
        Cmd::Render {
            file,
            output,
            scale,
        } => {
            if !(scale > 0.0 && scale <= 16.0) {
                return Err(Fail(2, "--scale must be in (0, 16]".into()));
            }
            let doc = load(&file)?;
            let pix = render(&doc, scale);
            let (w, h) = (pix.width() as u32, pix.height() as u32);
            write_image(
                &output,
                w,
                h,
                znimok_render::pixmap_to_rgba(&pix),
                Format::Png,
                100,
            )?;
            out(
                json!({ "output": output.display().to_string(), "width": w, "height": h }),
                format!("rendered {} ({w}×{h})", output.display()),
            );
        }
        Cmd::Export {
            file,
            output,
            format,
            quality,
        } => {
            let fmt = format_for(&output, format)?;
            let doc = load(&file)?;
            let pix = render(&doc, 1.0);
            let (w, h) = (pix.width() as u32, pix.height() as u32);
            if matches!(fmt, Format::Html) {
                write_html(&output, &doc, w, h, znimok_render::pixmap_to_rgba(&pix))?;
                out(
                    json!({ "output": output.display().to_string(), "width": w, "height": h, "format": "html" }),
                    format!("exported {} ({w}×{h}, HTML)", output.display()),
                );
                return Ok(());
            }
            write_image(
                &output,
                w,
                h,
                znimok_render::pixmap_to_rgba(&pix),
                fmt,
                quality,
            )?;
            out(
                json!({ "output": output.display().to_string(), "width": w, "height": h }),
                format!("exported {} ({w}×{h})", output.display()),
            );
        }
        Cmd::Apply {
            file,
            mut commands,
            stdin,
            output,
        } => {
            if stdin {
                for line in std::io::stdin().lock().lines() {
                    let line = line.map_err(|e| Fail(3, format!("stdin: {e}")))?;
                    if !line.trim().is_empty() {
                        commands.push(line);
                    }
                }
            }
            if commands.is_empty() {
                return Err(Fail(2, "no commands: use --cmd '<json>' or --stdin".into()));
            }
            let (doc, video) = load_parts(&file)?;
            let mut editor = Editor::new(doc);
            let mut results = Vec::new();
            for (i, c) in commands.iter().enumerate() {
                match editor.apply_json(c) {
                    Ok(applied) => results.push(serde_json::to_value(&applied).unwrap_or_default()),
                    // Nothing is saved when any command fails: all or nothing.
                    Err(e) => return Err(Fail(7, format!("command {} rejected: {e}", i + 1))),
                }
            }
            let target = output.unwrap_or(file);
            save(&target, &editor.doc, video.as_ref())?;
            out(
                json!({ "output": target.display().to_string(), "results": results }),
                format!(
                    "applied {} command(s), saved {}",
                    commands.len(),
                    target.display()
                ),
            );
        }
        Cmd::Query { file, query } => {
            let editor = Editor::new(load(&file)?);
            let answer = editor
                .query_json(&query)
                .map_err(|e| Fail(7, e.to_string()))?;
            // Queries always answer in JSON.
            let v: serde_json::Value = serde_json::from_str(&answer).unwrap_or_default();
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or(answer));
        }
        Cmd::Library {
            cmd: LibraryCmd::List { dir },
        } => {
            let dir = dir.unwrap_or_else(default_library);
            let entries = std::fs::read_dir(&dir).map_err(io(&dir))?;
            let mut rows = Vec::new();
            for e in entries.flatten() {
                let path = e.path();
                if !path.is_file() {
                    continue;
                }
                // Recognised by magic, not extension; read the head only.
                let Ok(mut f) = std::fs::File::open(&path) else {
                    continue;
                };
                let mut head = vec![0u8; 1 << 20];
                let n = std::io::Read::read(&mut f, &mut head).unwrap_or(0);
                head.truncate(n);
                if !znimok_format::is_znimok(&head) {
                    continue;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let p = znimok_format::peek(&head).or_else(|_| {
                    std::fs::read(&path)
                        .map_err(|e| FormatError::Io(e.to_string()))
                        .and_then(|b| znimok_format::peek(&b))
                });
                match p {
                    Ok(p) => rows.push(peek_json(&path, &p, size)),
                    Err(err) => rows.push(
                        json!({ "path": path.display().to_string(), "error": err.to_string() }),
                    ),
                }
            }
            rows.sort_by_key(|r| std::cmp::Reverse(r["created_ms"].as_i64().unwrap_or(0)));
            let text = if rows.is_empty() {
                format!("no documents in {}", dir.display())
            } else {
                rows.iter()
                    .map(|r| match r.get("error") {
                        Some(e) => format!(
                            "!  {}  ({})",
                            r["path"].as_str().unwrap_or(""),
                            e.as_str().unwrap_or("")
                        ),
                        None => format!(
                            "{:>5}×{:<5} {:>3} marks  {}",
                            r["width"],
                            r["height"],
                            r["objects"],
                            r["name"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(r["path"].as_str().unwrap_or(""))
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            out(
                json!({ "dir": dir.display().to_string(), "documents": rows }),
                text,
            );
        }
        Cmd::Handoff {
            file,
            note,
            to,
            no_redact,
            dry_run,
        } => {
            use znimok_agents::handoff;
            let mut hs = znimok_settings::Store::open_default()
                .map(|s| s.get().agents.handoff)
                .unwrap_or_default();
            match to.as_deref() {
                None => {}
                Some("claude") => hs.target = znimok_settings::HandoffTarget::ClaudeCode,
                Some("clipboard") => hs.target = znimok_settings::HandoffTarget::Clipboard,
                Some(o) => return Err(Fail(2, format!("--to: claude or clipboard, not «{o}»"))),
            }
            if no_redact {
                hs.redact = false;
            }
            let root = znimok_agents::data_dir().join("Handoff");
            let p = handoff::prepare(&file, &note, &Default::default(), &hs, &root)
                .map_err(|e| Fail(3, e))?;
            let claude = handoff::claude_path();
            let v = json!({
                "folder": p.dir.display().to_string(),
                "picture": p.picture.display().to_string(),
                "brief": p.brief.display().to_string(),
                "hidden": p.hidden,
                "claude": claude.as_ref().map(|c| c.display().to_string()),
                "prompt": handoff::prompt(&p),
            });
            if dry_run {
                out(v, format!("prepared {}\n{}", p.dir.display(), p.brief_text));
            } else {
                let sent = handoff::send(&p, &hs).map_err(|e| Fail(3, e))?;
                out(v, format!("handed over to {sent:?}: {}", p.dir.display()));
            }
        }
        Cmd::Mcp => {
            znimok_agents::serve_stdio().map_err(|e| Fail(3, e.to_string()))?;
        }
        Cmd::Agents { cmd } => agents(cmd, &out)?,
        Cmd::Update { cmd } => update(cmd, &out)?,
        Cmd::Schema { kind } => {
            let s = match kind {
                SchemaKind::Command => znimok_core::command::command_schema(),
                SchemaKind::Query => znimok_core::command::query_schema(),
            };
            println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        }
    }
    Ok(())
}

fn agents(cmd: AgentsCmd, out: &dyn Fn(serde_json::Value, String)) -> Result<(), Fail> {
    use znimok_agents::permissions::{Grant, Permissions, Scope};
    let data = znimok_agents::data_dir();
    let perms = Permissions::new(data.join("agents.json"));
    let settings = znimok_settings::Store::open_default()
        .ok_or_else(|| Fail(3, "no settings folder for this user".into()))?;
    let set_enabled = |on: bool| -> Result<(), Fail> {
        settings
            .update(|s| s.agents.mcp_enabled = on)
            .map(|_| ())
            .map_err(|e| Fail(3, e.to_string()))
    };
    match cmd {
        AgentsCmd::List => {
            let on = settings.get().agents.mcp_enabled;
            let clients = perms.clients();
            let text = std::iter::once(format!("MCP: {}", if on { "on" } else { "off" }))
                .chain(clients.iter().map(|(c, s)| {
                    let names: Vec<&str> = s.iter().map(|x| x.name()).collect();
                    format!("  {c}: {}", names.join(", "))
                }))
                .collect::<Vec<_>>()
                .join("\n");
            out(json!({"mcp_enabled": on, "clients": clients}), text);
        }
        AgentsCmd::Enable => {
            set_enabled(true)?;
            out(json!({"mcp_enabled": true}), "MCP: on".into());
        }
        AgentsCmd::Disable => {
            set_enabled(false)?;
            out(json!({"mcp_enabled": false}), "MCP: off".into());
        }
        AgentsCmd::Allow { client, scopes } => {
            if scopes.is_empty() {
                return Err(Fail(
                    2,
                    "name at least one scope: capture, library_read, library_write, settings"
                        .into(),
                ));
            }
            for s in &scopes {
                let scope =
                    Scope::parse(s).ok_or_else(|| Fail(2, format!("unknown scope «{s}»")))?;
                perms
                    .grant(&client, scope, Grant::Always)
                    .map_err(|e| Fail(3, e.to_string()))?;
            }
            out(
                json!({"client": client, "allowed": scopes}),
                format!("{client}: allowed {}", scopes.join(", ")),
            );
        }
        AgentsCmd::Revoke { client, all } => {
            match (client, all) {
                (_, true) => perms.revoke_all(),
                (Some(c), false) => perms.revoke(&c),
                (None, false) => return Err(Fail(2, "name a client or use --all".into())),
            }
            .map_err(|e| Fail(3, e.to_string()))?;
            out(json!({"revoked": true}), "revoked".into());
        }
        AgentsCmd::Log { last } => {
            let audit = znimok_agents::audit::Audit::open(data.join("agents-audit.jsonl"));
            let e = audit.entries();
            let tail: Vec<_> = e.iter().rev().take(last).rev().collect();
            let text = tail
                .iter()
                .map(|x| {
                    format!(
                        "{} {} {} {}",
                        chrono_ms(x.ts),
                        x.client,
                        x.tool,
                        if x.ok {
                            "ok"
                        } else {
                            x.error.as_deref().unwrap_or("error")
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            out(json!(tail), text);
        }
    }
    Ok(())
}

/// Unix milliseconds as local `YYYY-MM-DD HH:MM:SS` (no chrono in the CLI: via the std clock).
fn chrono_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let (h, m, s) = (
        secs.rem_euclid(86_400) / 3600,
        secs.rem_euclid(3600) / 60,
        secs.rem_euclid(60),
    );
    // Civil from days (H. Hinnant), UTC.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

fn update(cmd: UpdateCmd, out: &dyn Fn(serde_json::Value, String)) -> Result<(), Fail> {
    let current = env!("CARGO_PKG_VERSION");
    let updates = znimok_agents::data_dir().join("Updates");
    match cmd {
        UpdateCmd::MarkStarted { version } => {
            znimok_update::apply::mark_started(&updates, &version)
                .map_err(|e| Fail(3, e.to_string()))?;
            out(
                json!({ "started": version }),
                format!("start of {version} confirmed"),
            );
            return Ok(());
        }
        #[cfg(windows)]
        UpdateCmd::Install {
            msi,
            version,
            wait_pid,
            app,
            app_args,
            as_runner,
        } => {
            let app = match app {
                Some(a) => a,
                None => std::env::current_exe()
                    .map_err(|e| Fail(3, e.to_string()))?
                    .with_file_name("znimok-app.exe"),
            };
            if !as_runner {
                // From a copy: the MSI replaces the files of the install folder, this one too.
                let mut args = vec![
                    "update".into(),
                    "install".into(),
                    "--msi".into(),
                    msi.display().to_string(),
                    "--version".into(),
                    version,
                    "--app".into(),
                    app.display().to_string(),
                    "--as-runner".into(),
                ];
                if let Some(pid) = wait_pid {
                    args.extend(["--wait-pid".into(), pid.to_string()]);
                }
                for a in app_args {
                    args.extend(["--app-arg".into(), a]);
                }
                let exe = std::env::current_exe().map_err(|e| Fail(3, e.to_string()))?;
                znimok_update::apply::run_detached_copy(&updates, &exe, &args)
                    .map_err(|e| Fail(3, e.to_string()))?;
                out(
                    json!({ "started": true }),
                    "installing in the background".into(),
                );
                return Ok(());
            }
            if let Some(pid) = wait_pid
                && !znimok_update::apply::wait_for_exit(pid, std::time::Duration::from_secs(120))
            {
                return Err(Fail(3, "the app did not exit".into()));
            }
            let mut app_cmd = vec![app.display().to_string()];
            app_cmd.extend(app_args);
            let o = znimok_update::apply::install(
                &updates,
                &msi,
                &version,
                &app_cmd,
                znimok_update::apply::START_WAIT,
            );
            let ok = matches!(o, znimok_update::apply::Outcome::Installed { .. });
            out(json!({ "outcome": format!("{o:?}") }), format!("{o:?}"));
            return if ok {
                Ok(())
            } else {
                Err(Fail(3, format!("{o:?}")))
            };
        }
        #[cfg(not(windows))]
        UpdateCmd::Install { .. } => {
            return Err(Fail(3, "on macOS updates come through Sparkle".into()));
        }
        _ => {}
    }
    let platform = znimok_update::Platform::current().ok_or_else(|| {
        Fail(
            3,
            "updates exist for Windows x64 and macOS on Apple silicon".into(),
        )
    })?;
    let http = znimok_models::http::system();
    let fail = |e: znimok_update::UpdateError| Fail(3, e.to_string());
    let found = znimok_update::check(http.as_ref(), current, platform).map_err(fail)?;
    let Some(a) = found else {
        out(
            json!({ "current": current, "available": null }),
            format!("Znimok {current} is up to date"),
        );
        return Ok(());
    };
    match cmd {
        UpdateCmd::Check => out(
            json!({ "current": current, "available": a.version, "page": a.page,
                    "installer": a.installer_name(), "size": a.installer_size() }),
            format!(
                "Znimok {} is available (now {current}): {}",
                a.version, a.page
            ),
        ),
        UpdateCmd::Download => {
            let path =
                znimok_update::download(http.as_ref(), &a, current, &updates).map_err(fail)?;
            out(
                json!({ "version": a.version, "installer": path.display().to_string(), "verified": true }),
                format!("{} — signature and checksum verified", path.display()),
            );
        }
        UpdateCmd::Install { .. } | UpdateCmd::MarkStarted { .. } => unreachable!(),
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    let json = cli.json;
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail(code, msg)) => {
            if json {
                let _ = writeln!(
                    std::io::stderr(),
                    "{}",
                    json!({ "error": msg, "code": code })
                );
            } else {
                let _ = writeln!(std::io::stderr(), "znimok: {msg}");
            }
            ExitCode::from(code)
        }
    }
}
