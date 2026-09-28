//! Compiles the Slint UI with the interface languages built in. The reference strings live in
//! `i18n/*.ftl` (ZK-29); Slint wants gettext, so the `.po` files are generated here from the
//! Fluent files every build — the `.ftl` files stay the single source of truth.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../i18n");
    let en = std::fs::read_to_string(root.join("en.ftl")).expect("i18n/en.ftl");
    println!("cargo:rerun-if-changed={}", root.join("en.ftl").display());

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("tr");
    let domain = std::env::var("CARGO_PKG_NAME").unwrap();
    for (lang, _) in znimok_i18n::BUILT_IN
        .iter()
        .filter(|(l, _)| *l != znimok_i18n::FALLBACK)
    {
        let file = root.join(format!("{lang}.ftl"));
        println!("cargo:rerun-if-changed={}", file.display());
        let src = std::fs::read_to_string(&file).expect("language file");
        let po = znimok_i18n::po::to_po(&en, &src, lang)
            .unwrap_or_else(|e| panic!("{lang}.ftl → .po: {e}"));
        let dir = out.join(lang).join("LC_MESSAGES");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{domain}.po")), po).unwrap();
    }

    // macOS: screencapturekit's Swift bridge links `@rpath/libswift_Concurrency.dylib`; a
    // dependency's link args never reach the final binary (P3, ZK-16).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
    }

    // `MouseCursor.custom(...)` (the tailless arrow of the Select tool, ZK-47) is behind the
    // compiler's experimental switch in Slint 1.18; nothing else here relies on it.
    // SAFETY: the build script is single-threaded at this point.
    unsafe { std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1") };
    println!("cargo:rerun-if-changed=ui/cursors");
    check_tr_texts(&en);

    let config = slint_build::CompilerConfiguration::new().with_bundled_translations(&out);
    slint_build::compile_with_config("ui/app.slint", config).expect("slint compile");
}

/// Slint looks a translation up by the English text, so the text inside `@tr("id" => "…")` must be
/// the English of `id` in en.ftl word for word — otherwise the UI silently stays in English (or
/// empty). Checked at every build; messages with variables are left to the reviewer.
fn check_tr_texts(en: &str) {
    let mut values = std::collections::HashMap::new();
    for line in en.lines() {
        if let Some((k, v)) = line.split_once(" = ")
            && !k.is_empty()
            && k.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            values.insert(k.to_string(), v.to_string());
        }
    }
    let ui = std::fs::read_to_string("ui/app.slint").expect("ui/app.slint");
    println!("cargo:rerun-if-changed=ui/app.slint");
    let mut bad = Vec::new();
    let mut rest = ui.as_str();
    while let Some(i) = rest.find("@tr(\"") {
        rest = &rest[i + 5..];
        let Some(j) = rest.find('"') else { break };
        let id = &rest[..j];
        let after = &rest[j + 1..];
        let Some(text) = after.strip_prefix(" => \"") else {
            continue;
        };
        let mut end = 0;
        let bytes = text.as_bytes();
        while end < bytes.len() && !(bytes[end] == b'"' && (end == 0 || bytes[end - 1] != b'\\')) {
            end += 1;
        }
        let text = text[..end].replace("\\\"", "\"");
        match values.get(id) {
            Some(v) if v.contains('{') || *v == text => {}
            Some(v) => bad.push(format!("{id}: \"{text}\" ≠ en.ftl \"{v}\"")),
            None => bad.push(format!("{id}: not in en.ftl")),
        }
    }
    if !bad.is_empty() {
        panic!(
            "@tr texts in ui/app.slint must match en.ftl (Slint finds translations by them):\n  {}",
            bad.join("\n  ")
        );
    }
}
