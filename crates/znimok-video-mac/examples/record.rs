//! A recording of the main display for a few seconds (needs «Screen Recording» for the process
//! that runs it): `cargo run -p znimok-video-mac --example record -- out.mp4 [seconds] [--sound]`.

#[cfg(target_os = "macos")]
fn main() {
    use znimok_video_mac::{RecordRequest, Recording, Target};
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "rec.mp4".into()));
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let sound = args.iter().any(|a| a == "--sound");
    // SAFETY: plain CoreGraphics call.
    let main_display = unsafe { CGMainDisplayID() };
    let mut req = RecordRequest::new(
        Target::Display {
            id: main_display,
            region: None,
        },
        out.clone(),
    );
    req.system_audio = sound;
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("start: {e}");
            std::process::exit(1);
        }
    };
    println!("started: {:?}", rec.started());
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let f = rec.stop();
    println!("result: {:?}", f.result);
    println!("file: {:?}", f.path);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
}

#[cfg(not(target_os = "macos"))]
fn main() {}
