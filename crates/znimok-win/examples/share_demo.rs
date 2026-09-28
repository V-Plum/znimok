//! Live check of the Share sheet (ZK-63): `cargo run -p znimok-win --example share_demo [file]`.
//! Opens a small window and the system Share sheet for the file (a generated PNG by default);
//! pick Mail / Phone Link / Teams… and see that the picture arrives. Close the window to exit.

#[cfg(windows)]
fn main() {
    use std::path::PathBuf;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, MSG, SW_SHOW, ShowWindow, TranslateMessage,
        WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::w;
    use znimok_platform::Share;

    let file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let p = std::env::temp_dir().join("Znimok share demo.png");
            let px: Vec<u8> = (0..200 * 120)
                .flat_map(|i| [(i % 200) as u8, 90, 200, 255])
                .collect();
            image::save_buffer(&p, &px, 200, 120, image::ExtendedColorType::Rgba8).unwrap();
            p
        });
    // SAFETY: a plain top-level window of this thread.
    let hwnd: HWND = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("Znimok — Share demo (close to exit)"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            200,
            200,
            480,
            160,
            None,
            None,
            None,
            None,
        )
    }
    .expect("window");
    // SAFETY: our window.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    let share = znimok_win::WinShare::new(hwnd.0 as isize, "Znimok");
    println!("available: {}", share.available());
    match share.share(std::slice::from_ref(&file), None) {
        Ok(()) => println!("share sheet requested for {}", file.display()),
        Err(e) => {
            eprintln!("share failed: {e}");
            std::process::exit(1);
        }
    }
    let mut msg = MSG::default();
    // SAFETY: the standard message loop of this thread.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn main() {}
