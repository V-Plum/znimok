fn main() {
    #[cfg(target_os = "macos")]
    {
        slint_build::compile("ui/p3.slint").expect("slint compile");
        // screencapturekit's Swift bridge links `@rpath/libswift_Concurrency.dylib`. The crate
        // prints this rpath in its own build script, but link args of a dependency never reach
        // the final binary — without it dyld aborts before `main` ("no LC_RPATH's found").
        // /usr/lib/swift is the system Swift runtime (dyld shared cache), present on every
        // supported macOS; no Xcode path is baked in.
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
    }
}
