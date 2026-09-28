//! Manual check of the crash handlers: `cargo run -p znimok-log --example crash -- panic|native <dir>`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.get(1).map(std::path::PathBuf::from);
    let _log = znimok_log::init(znimok_log::Config {
        dir,
        ..znimok_log::Config::for_app("crash-example", "0.0.0")
    });
    tracing::info!("about to crash: {:?}", args.first());
    match args.first().map(String::as_str) {
        Some("native") => unsafe {
            // A deliberate access violation (never reaches the panic hook).
            std::ptr::write_volatile(std::ptr::null_mut::<u32>(), 1);
        },
        _ => panic!("навмисна паніка з прикладу"),
    }
}
