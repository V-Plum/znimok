//! «Передати агенту» (ZK-71): the screenshot plus what an agent needs to understand it, handed to
//! Claude Code in a new terminal or put on the clipboard for any other agent or chat.
//!
//! [`prepare`] writes a hand-off folder `<data>/Handoff/<id>/`:
//! - `screenshot.png` — the document with its marks; by default **a copy with secrets, personal
//!   data and faces hidden** (the agent may send it to a cloud model; the library document is not
//!   changed);
//! - `context.json` — window, application, source, time, size, the person's note, the recognised
//!   text with secrets masked;
//! - `handoff.md` — the same for reading, pointing at the picture.
//!
//! [`send`] then opens Claude Code with a prompt that only names `handoff.md` (no screen text on
//! a command line), or fills the clipboard: picture + file + the brief as text.

use crate::library;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use znimok_core::{Document, Editor};
use znimok_models::Rgba;
use znimok_settings::{Handoff as Settings, HandoffTarget};

/// What the app knows about where the picture came from (not stored in the document).
#[derive(Clone, Debug, Default)]
pub struct Origin {
    pub window_title: Option<String>,
    pub app: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Prepared {
    pub dir: PathBuf,
    pub picture: PathBuf,
    pub context: PathBuf,
    pub brief: PathBuf,
    pub brief_text: String,
    /// What was hidden on the copy (kinds), for the plate «приховано: 2 ключі, 1 e-mail».
    pub hidden: Vec<String>,
}

/// Writes the hand-off folder for the document at `doc_path`.
pub fn prepare(
    doc_path: &Path,
    note: &str,
    origin: &Origin,
    settings: &Settings,
    root: &Path,
) -> Result<Prepared, String> {
    let doc = library::load(doc_path)?;
    let dir = root.join(format!("{}", doc.id.simple()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let raster = library::render(&doc, 1.0);
    let img = Rgba::new(raster.width, raster.height, raster.rgba.clone()).ok_or("empty picture")?;
    let ocr = settings
        .include_text
        .then(|| znimok_models::ocr::system().and_then(|o| o.recognize(&img, &[]).ok()))
        .flatten();

    let mut hidden = Vec::new();
    let shown: Document = if settings.redact {
        let faces = znimok_models::faces::detect(&img).unwrap_or_default();
        let found = znimok_mask::suggest(ocr.as_ref(), &faces, raster.width, raster.height);
        hidden = found
            .iter()
            .map(|s| format!("{:?}", s.kind).to_lowercase())
            .collect();
        let mut ed = Editor::new(doc.clone());
        for c in znimok_mask::commands(&found, None) {
            ed.apply(c).map_err(|e| e.to_string())?;
        }
        ed.doc
    } else {
        doc.clone()
    };
    let picture = dir.join("screenshot.png");
    let png = library::encode_png(&library::render(&shown, 1.0))?;
    std::fs::write(&picture, png).map_err(|e| e.to_string())?;

    let text = ocr
        .as_ref()
        .map(|o| znimok_mask::mask_text(&o.text()))
        .filter(|t| !t.trim().is_empty());
    let when = chrono::DateTime::from_timestamp_millis(doc.meta.created_ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    let context = json!({
        "picture": picture.display().to_string(),
        "document": {"id": doc.id.to_string(), "name": doc.name, "path": doc_path.display().to_string()},
        "taken": when,
        "source": doc.meta.source,
        "window": origin.window_title,
        "app": origin.app,
        "size": {"width": raster.width, "height": raster.height},
        "marks": doc.objects.len(),
        "note": note,
        "hidden_on_copy": hidden,
        "text": text,
        "text_languages": ocr.as_ref().map(|o| o.languages.clone()),
    });
    let context_path = dir.join("context.json");
    std::fs::write(
        &context_path,
        serde_json::to_vec_pretty(&context).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    let brief_text = brief(&context, &picture);
    let brief_path = dir.join("handoff.md");
    std::fs::write(&brief_path, &brief_text).map_err(|e| e.to_string())?;
    Ok(Prepared {
        dir,
        picture,
        context: context_path,
        brief: brief_path,
        brief_text,
        hidden,
    })
}

fn brief(c: &Value, picture: &Path) -> String {
    let mut s = String::from("# Screenshot from Znimok\n\n");
    s += &format!("Picture: {}\n", picture.display());
    let origin = match (c["window"].as_str(), c["app"].as_str()) {
        (Some(w), Some(a)) => format!("the window «{w}» of {a}"),
        (Some(w), None) => format!("the window «{w}»"),
        (None, Some(a)) => a.to_string(),
        (None, None) => c["source"].as_str().unwrap_or("screen").to_string(),
    };
    s += &format!(
        "Taken: {} — {}, {}×{} px, {} marks\n",
        c["taken"].as_str().unwrap_or(""),
        origin,
        c["size"]["width"],
        c["size"]["height"],
        c["marks"]
    );
    if let Some(h) = c["hidden_on_copy"].as_array().filter(|h| !h.is_empty()) {
        s += &format!(
            "Hidden on this copy before sharing: {}\n",
            h.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(n) = c["note"].as_str().filter(|n| !n.trim().is_empty()) {
        s += &format!("\n## Request\n\n{n}\n");
    }
    if let Some(t) = c["text"].as_str() {
        s += &format!("\n## Text on the screenshot (secrets masked)\n\n```\n{t}\n```\n");
    }
    s += "\nMore detail: context.json next to this file.\n";
    s
}

/// Where `claude` is, if Claude Code is installed (PATH, then the usual install places).
pub fn claude_path() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if let Some(h) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        dirs.push(PathBuf::from(&h).join(".local").join("bin"));
    }
    dirs.iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// The prompt Claude Code starts with: only the path of the brief (screen text never goes on a
/// command line).
pub fn prompt(p: &Prepared) -> String {
    format!(
        "Znimok handed you a screenshot. Read {} (it names the picture and its context) and help me with it.",
        p.brief.display()
    )
}

/// `'…'` for a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A string literal for AppleScript.
pub fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The `cmd /k` line on Windows: `claude --add-dir "<dir>" "<prompt>"`. `%` would be expanded by
/// cmd even inside quotes, `"` cannot occur in Windows paths.
pub fn windows_command_line(dir: &Path, prompt: &str) -> Result<String, String> {
    let d = dir.display().to_string();
    if d.contains('%') || prompt.contains('%') || prompt.contains('"') {
        return Err("the hand-off path cannot be passed to cmd safely".into());
    }
    Ok(format!("claude --add-dir \"{d}\" \"{prompt}\""))
}

/// The Terminal script on macOS.
pub fn mac_script(cwd: &Path, dir: &Path, prompt: &str) -> String {
    let shell = format!(
        "cd {} && claude --add-dir {} {}",
        sh_quote(&cwd.display().to_string()),
        sh_quote(&dir.display().to_string()),
        sh_quote(prompt)
    );
    format!(
        "tell application \"Terminal\"\nactivate\ndo script {}\nend tell",
        applescript_string(&shell)
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    ClaudeCode,
    Clipboard,
}

/// Hands the prepared folder over as the settings say; Claude Code falls back to the clipboard
/// when it is not installed.
pub fn send(p: &Prepared, s: &Settings) -> Result<Sent, String> {
    if s.target == HandoffTarget::ClaudeCode && claude_path().is_some() {
        let cwd = s
            .working_dir
            .clone()
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| p.dir.clone());
        launch_claude(&cwd, p)?;
        return Ok(Sent::ClaudeCode);
    }
    to_clipboard(p)?;
    Ok(Sent::Clipboard)
}

#[cfg(windows)]
fn launch_claude(cwd: &Path, p: &Prepared) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let line = windows_command_line(&p.dir, &prompt(p))?;
    // Windows Terminal when present, else a console window.
    let wt = std::process::Command::new("wt.exe")
        .arg("-w")
        .arg("new")
        .arg("-d")
        .arg(cwd)
        .raw_arg(format!("cmd /k {}", line.replace(';', "\\;")))
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    std::process::Command::new("cmd.exe")
        .current_dir(cwd)
        .raw_arg(format!("/c start \"Znimok → Claude Code\" cmd /k {line}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open a terminal: {e}"))
}

#[cfg(target_os = "macos")]
fn launch_claude(cwd: &Path, p: &Prepared) -> Result<(), String> {
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(mac_script(cwd, &p.dir, &prompt(p)))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open Terminal: {e}"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn launch_claude(_: &Path, _: &Prepared) -> Result<(), String> {
    Err("Claude Code launch is not supported here".into())
}

/// The picture (pixels + file) and the brief as text.
fn to_clipboard(p: &Prepared) -> Result<(), String> {
    use znimok_platform::{ClipImage, ClipItem, Clipboard};
    let png = std::fs::read(&p.picture).map_err(|e| e.to_string())?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let items = [
        ClipItem::Image(ClipImage {
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw(),
            png: Some(png),
        }),
        ClipItem::Files(vec![p.picture.clone()]),
        ClipItem::Text(p.brief_text.clone()),
    ];
    #[cfg(windows)]
    let cb = znimok_win::WinClipboard::new();
    #[cfg(target_os = "macos")]
    let cb = znimok_mac::MacClipboard::new();
    #[cfg(any(windows, target_os = "macos"))]
    return cb.write(&items).map_err(|e| e.to_string());
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = items;
        Err("no clipboard here".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_with_picture_context_and_brief() {
        let root = std::env::temp_dir().join(format!("zk-handoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib = crate::library::Library {
            dir: root.join("lib"),
        };
        let d = crate::library::test_doc(200, 120);
        let path = lib.new_path(&d);
        crate::library::save(&path, &d).unwrap();
        let s = Settings {
            include_text: false,
            ..Settings::default()
        };
        let origin = Origin {
            window_title: Some("Налаштування".into()),
            app: Some("explorer.exe".into()),
        };
        let p = prepare(&path, "Чому кнопка сіра?", &origin, &s, &root.join("h")).unwrap();
        assert!(p.picture.exists() && p.context.exists() && p.brief.exists());
        let c: Value = serde_json::from_slice(&std::fs::read(&p.context).unwrap()).unwrap();
        assert_eq!(c["window"], "Налаштування");
        assert_eq!(c["note"], "Чому кнопка сіра?");
        assert_eq!(c["size"]["width"], 200);
        assert!(
            p.brief_text
                .contains("the window «Налаштування» of explorer.exe"),
            "{}",
            p.brief_text
        );
        assert!(p.brief_text.contains("## Request\n\nЧому кнопка сіра?"));
        assert!(prompt(&p).contains("handoff.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn brief_masks_secrets_in_the_text() {
        let c = json!({"window": null, "app": null, "source": "region", "taken": "2026-09-29 10:00",
            "size": {"width": 1, "height": 1}, "marks": 0, "note": "",
            "hidden_on_copy": ["secret"], "text": znimok_mask::mask_text("password: hunter2")});
        let b = brief(&c, Path::new("/x/screenshot.png"));
        assert!(b.contains("password: •••") && !b.contains("hunter2"), "{b}");
        assert!(b.contains("Hidden on this copy before sharing: secret"));
    }

    #[test]
    fn command_lines_are_quoted() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(applescript_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        let script = mac_script(
            Path::new("/Users/v/My Project"),
            Path::new("/tmp/h"),
            "Read /tmp/h/handoff.md",
        );
        assert!(script.contains(r#"do script "cd '/Users/v/My Project' && claude --add-dir '/tmp/h' 'Read /tmp/h/handoff.md'""#), "{script}");
        assert_eq!(
            windows_command_line(Path::new(r"C:\Users\V S\h"), "Read it").unwrap(),
            r#"claude --add-dir "C:\Users\V S\h" "Read it""#
        );
        assert!(windows_command_line(Path::new(r"C:\%TEMP%\h"), "x").is_err());
    }
}
