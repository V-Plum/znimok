//! Embeds the release public key (`keys/znimok-release-p256.pub.pem`, ZK-111) when it is
//! committed; without it the updater reports that updates are not set up instead of checking.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let key = root.join("keys").join("znimok-release-p256.pub.pem");
    println!("cargo:rerun-if-changed={}", key.display());
    let value = match std::fs::read_to_string(&key) {
        Ok(pem) => format!("Some({pem:?})"),
        Err(_) => "None".to_string(),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("release_key.rs");
    std::fs::write(
        out,
        format!("/// The committed release key (PEM), if any.\npub const RELEASE_KEY: Option<&str> = {value};\n"),
    )
    .unwrap();
}
