//! COM entry points are exported PRIVATE (by name for GetProcAddress, not in the import
//! library), as regsvr32 and the shell expect — otherwise MSVC warns LNK4104.

fn main() {
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if msvc {
        let def =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("exports.def");
        println!("cargo:rerun-if-changed={}", def.display());
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
    }
}
