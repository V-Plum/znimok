//! Tests of this crate are binaries too: screencapturekit's Swift bridge links
//! `@rpath/libswift_Concurrency.dylib`, so they need the rpath the app sets in its own build.rs.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
