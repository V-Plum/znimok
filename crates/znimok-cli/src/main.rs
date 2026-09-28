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
    /// Renders the document with its marks to a PNG (the crop, 1:1 unless --scale).
    Render {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Exports to PNG, JPEG or WebP; the format comes from --format or the file extension.
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
    let bytes = std::fs::read(path).map_err(io(path))?;
    Ok(znimok_format::read(&bytes)?)
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

fn save(path: &Path, doc: &Document) -> Result<(), Fail> {
    let opts = WriteOptions {
        app_version: format!("znimok CLI {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(thumbnail(doc)),
        ..Default::default()
    };
    Ok(znimok_format::save(path, doc, &opts)?)
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
            save(&output, &doc)?;
            out(
                json!({ "output": output.display().to_string(), "id": doc.id.to_string(), "width": w, "height": h }),
                format!("created {} ({w}×{h})", output.display()),
            );
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
            let mut editor = Editor::new(load(&file)?);
            let mut results = Vec::new();
            for (i, c) in commands.iter().enumerate() {
                match editor.apply_json(c) {
                    Ok(applied) => results.push(serde_json::to_value(&applied).unwrap_or_default()),
                    // Nothing is saved when any command fails: all or nothing.
                    Err(e) => return Err(Fail(7, format!("command {} rejected: {e}", i + 1))),
                }
            }
            let target = output.unwrap_or(file);
            save(&target, &editor.doc)?;
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
