//! `znimok-i18n` — tools for the language files in `i18n/`.
//!
//!   znimok-i18n check [DIR]            all *.ftl in DIR (default: the repo's i18n/) against the rules
//!   znimok-i18n sync-comments [DIR]    copy en.ftl's header, order and comments into the other files
//!   znimok-i18n po LANG [-o FILE]      gettext .po for Slint (stdout by default)
//!   znimok-i18n slint-refs             the @tr("id" => "…") of every message
//!   znimok-i18n stats [DIR]            messages per language and whether it is complete

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use znimok_i18n::{check, po, sync};

fn repo_i18n() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("i18n")
}

fn load(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let mut files = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|x| x == "ftl") {
            let lang = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            files.push((lang, src));
        }
    }
    files.sort();
    Ok(files)
}

fn run(args: &[String]) -> Result<bool, String> {
    let dir = |i: usize| {
        args.get(i)
            .filter(|a| !a.starts_with('-'))
            .map(PathBuf::from)
            .unwrap_or_else(repo_i18n)
    };
    match args.first().map(String::as_str) {
        Some("check") => {
            let files = load(&dir(1))?;
            let refs: Vec<(&str, &str)> = files
                .iter()
                .map(|(l, s)| (l.as_str(), s.as_str()))
                .collect();
            let r = check::check_sources(&refs);
            println!("{}", r.render());
            Ok(r.errors() == 0)
        }
        Some("stats") => {
            let files = load(&dir(1))?;
            let refs: Vec<(&str, &str)> = files
                .iter()
                .map(|(l, s)| (l.as_str(), s.as_str()))
                .collect();
            let r = check::check_sources(&refs);
            for (l, n) in &r.counts {
                let errs = r
                    .problems
                    .iter()
                    .filter(|p| p.error && &p.lang == l)
                    .count();
                println!(
                    "{l}: {n} повідомлень, {}",
                    if errs == 0 {
                        "повна".to_string()
                    } else {
                        format!("{errs} помилок")
                    }
                );
            }
            Ok(true)
        }
        Some("sync-comments") => {
            let d = dir(1);
            let en_path = d.join("en.ftl");
            let en = std::fs::read_to_string(&en_path).map_err(|e| e.to_string())?;
            for (lang, src) in load(&d)? {
                let out = sync::sync_comments(&en, &src).map_err(|e| format!("{lang}: {e}"))?;
                if out != src {
                    std::fs::write(d.join(format!("{lang}.ftl")), &out)
                        .map_err(|e| e.to_string())?;
                    println!("{lang}.ftl оновлено");
                }
            }
            Ok(true)
        }
        Some("po") => {
            let lang = args.get(1).ok_or("po LANG [-o FILE]")?;
            let d = repo_i18n();
            let en = std::fs::read_to_string(d.join("en.ftl")).map_err(|e| e.to_string())?;
            let tg = std::fs::read_to_string(d.join(format!("{lang}.ftl")))
                .map_err(|e| e.to_string())?;
            let out = po::to_po(&en, &tg, lang)?;
            match args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
            {
                Some(f) => {
                    if let Some(parent) = Path::new(f).parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(f, out).map_err(|e| e.to_string())?
                }
                None => print!("{out}"),
            }
            Ok(true)
        }
        Some("slint-refs") => {
            let en =
                std::fs::read_to_string(repo_i18n().join("en.ftl")).map_err(|e| e.to_string())?;
            print!("{}", po::slint_refs(&en)?);
            Ok(true)
        }
        _ => Err(
            "команда: check | sync-comments | po LANG | slint-refs | stats (див. main.rs)".into(),
        ),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("помилка: {e}");
            ExitCode::FAILURE
        }
    }
}
