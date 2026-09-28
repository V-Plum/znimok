//! Live check of the Windows capture on a real desk (mixed DPI, HDR) — prints JSON, writes PNG.
//!
//!   cargo run -p znimok-win --example probe -- displays
//!   cargo run -p znimok-win --example probe -- windows
//!   cargo run -p znimok-win --example probe -- shot display <n> [--dxgi] [-o out.png]
//!   cargo run -p znimok-win --example probe -- shot window <part of title> [-o out.png]
//!   cargo run -p znimok-win --example probe -- shot region <x> <y> <w> <h> [-o out.png]
//!
//! For a window, `size_matches_dwm_bounds` must be true on every monitor (move the window from a
//! 100 % display to a 150 % one and shoot again).

#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("помилка: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("лише Windows");
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use znimok_platform::{Capture, CaptureOptions, CaptureTarget, PixelFormat, Rect, WindowList};
    use znimok_win::{Api, WinCapture};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |n: &str| {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let api = if args.iter().any(|a| a == "--dxgi") {
        Api::Dxgi
    } else {
        Api::Wgc
    };
    let c = WinCapture::with_api(api);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["displays", ..] => {
            for d in c.displays().map_err(|e| e.to_string())? {
                println!("{d:?}");
            }
            println!("caps: {:?}", c.caps());
        }
        ["windows", ..] => {
            for w in c.windows().map_err(|e| e.to_string())? {
                println!(
                    "{:>10X}  {:?}  ×{}  {}  «{}»",
                    w.id.0, w.bounds, w.scale_factor, w.app, w.title
                );
            }
        }
        ["shot", kind, rest @ ..] => {
            let (target, bounds) = match *kind {
                "display" => {
                    let n: usize = rest
                        .first()
                        .and_then(|s| s.parse().ok())
                        .ok_or("номер дисплея")?;
                    let d = c
                        .displays()
                        .map_err(|e| e.to_string())?
                        .get(n)
                        .cloned()
                        .ok_or("немає такого дисплея")?;
                    (CaptureTarget::Display { id: d.id }, d.bounds)
                }
                "window" => {
                    let part = rest.first().ok_or("частина назви вікна")?.to_lowercase();
                    let w = c
                        .windows()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|w| !w.own && w.title.to_lowercase().contains(&part))
                        .ok_or("вікно не знайдено")?;
                    println!(
                        "вікно: «{}» {:?} масштаб {}",
                        w.title, w.bounds, w.scale_factor
                    );
                    (CaptureTarget::Window { id: w.id }, w.bounds)
                }
                "region" => {
                    let n: Vec<i64> = rest.iter().take(4).filter_map(|s| s.parse().ok()).collect();
                    let [x, y, w, h] = n[..] else {
                        return Err("region x y w h".into());
                    };
                    let r = Rect::new(x as i32, y as i32, w as u32, h as u32);
                    (CaptureTarget::Region { rect: r }, r)
                }
                _ => return Err("shot display|window|region".into()),
            };
            let t = std::time::Instant::now();
            let f = c
                .capture(&target, &CaptureOptions::default())
                .map_err(|e| e.to_string())?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!(
                "кадр {}×{} {:?} {:?} за {ms:.1} мс; межі {:?}; size_matches_dwm_bounds: {}",
                f.width,
                f.height,
                f.format,
                f.color,
                bounds,
                (f.width, f.height) == (bounds.width, bounds.height)
            );
            let out = flag("-o").unwrap_or_else(|| "probe.png".into());
            let rgba = match f.format {
                PixelFormat::Bgra8 | PixelFormat::Rgba8 => f.to_rgba8().unwrap(),
                PixelFormat::Rgba16Float => scrgb_preview(&f),
                PixelFormat::Rgb10A2 => {
                    return Err("PQ-кадр: перегляд не реалізовано (ZK-38)".into());
                }
            };
            image::save_buffer(&out, &rgba, f.width, f.height, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            println!("PNG: {out}");
        }
        _ => return Err("displays | windows | shot … (див. початок файла)".into()),
    }
    Ok(())
}

/// Rough scRGB → sRGB preview (LH formula: value / (white/80), clip, sRGB curve). Not the product's
/// tone mapping (ZK-38), just enough to look at the shot.
#[cfg(windows)]
fn scrgb_preview(f: &znimok_platform::Frame) -> Vec<u8> {
    fn half(b: u16) -> f32 {
        let (s, e, m) = ((b >> 15) & 1, ((b >> 10) & 0x1f) as i32, (b & 0x3ff) as f32);
        let v = match e {
            0 => m / 1024.0 * 2f32.powi(-14),
            31 => f32::INFINITY,
            _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
        };
        if s == 1 { -v } else { v }
    }
    let k = 80.0 / f.color.sdr_white_nits;
    let enc = |v: f32| {
        let v = (v * k).clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    let mut out = Vec::with_capacity((f.width * f.height * 4) as usize);
    for y in 0..f.height {
        for p in f.row(y).as_chunks::<8>().0 {
            let ch = |i: usize| half(u16::from_le_bytes([p[2 * i], p[2 * i + 1]]));
            out.extend_from_slice(&[enc(ch(0)), enc(ch(1)), enc(ch(2)), 255]);
        }
    }
    out
}
