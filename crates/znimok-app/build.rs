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

    let config = slint_build::CompilerConfiguration::new().with_bundled_translations(&out);
    slint_build::compile_with_config("ui/app.slint", config).expect("slint compile");
}
