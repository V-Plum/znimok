//! macOS: znimok-mac (clipboard, capture) links screencapturekit's Swift bridge, which needs
//! `@rpath/libswift_Concurrency.dylib`; a dependency's link args never reach this crate's own
//! binaries and tests.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
