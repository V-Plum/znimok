//! Windows side of P2: commands `displays`, `windows`, `shot`.

pub mod display;
pub mod dxgi;
pub mod wgc;
pub mod winlist;

use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

fn init() {
    // SAFETY: process-wide settings at start-up, before any window or COM object exists.
    unsafe {
        // Physical pixels everywhere (LH trap 17: PMv2, or window coordinates drift on mixed DPI).
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

pub fn run(cmd: &str, args: &[String]) -> Result<(), String> {
    init();
    match cmd {
        "displays" => print_json(&display::list()?),
        "windows" => print_json(&winlist::list()?),
        "shot" => shot(args),
        "border-probe" => border_probe(args),
        other => Err(format!("невідома команда «{other}»")),
    }
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn shot(args: &[String]) -> Result<(), String> {
    let kind = args
        .first()
        .map(String::as_str)
        .ok_or("shot display <n> | shot window <hwnd|назва>")?;
    let what = args.get(1).ok_or("бракує номера монітора або вікна")?;
    let cpu = args.iter().any(|a| a == "--cpu");
    let out = opt(args, "-o").unwrap_or("p2-shot.png").to_string();
    let monitors = display::list()?;
    match kind {
        "display" => {
            let i: usize = what
                .parse()
                .map_err(|_| "номер монітора — число (див. displays)")?;
            let m = monitors.get(i).ok_or("немає такого монітора")?;
            let api = opt(args, "--api").unwrap_or("wgc");
            let (frame, meta) = match api {
                "wgc" => wgc::capture(wgc::Target::Display(m.handle()))
                    .map(|(f, m)| (f, serde_json::to_value(m).unwrap_or_default()))?,
                "dxgi" => dxgi::capture(m.handle())
                    .map(|(f, m)| (f, serde_json::to_value(m).unwrap_or_default()))?,
                _ => return Err("--api wgc | dxgi".into()),
            };
            let white = m.white();
            let png = crate::save_png(&frame, white, cpu, &out)?;
            let rect = m.rect;
            let expect = [rect[2] - rect[0], rect[3] - rect[1]];
            print_json(&serde_json::json!({
                "target": { "display": m.index, "device": m.device, "hdr": m.hdr, "dpi": m.dpi, "rect": rect },
                "capture": meta, "format": frame.format_name(), "transfer": frame.transfer,
                "size": [frame.width, frame.height], "size_matches_monitor": ([frame.width as i32, frame.height as i32] == expect),
                "white_nits": white, "white_from_system": m.sdr_white_nits.is_some(), "result": png,
            }))
        }
        "window" => {
            let wins = winlist::list()?;
            let w = find_window(&wins, what).ok_or("вікно не знайдено (див. windows)")?;
            // SAFETY: a window handle from the list.
            let hmon = unsafe { MonitorFromWindow(w.handle(), MONITOR_DEFAULTTONEAREST) };
            let m = monitors.iter().find(|m| m.hmonitor == hmon.0 as isize);
            let white = m.map_or(crate::tone::FALLBACK_WHITE_SDR, display::Monitor::white);
            let (frame, meta) = wgc::capture(wgc::Target::Window(w.handle()))?;
            let png = crate::save_png(&frame, white, cpu, &out)?;
            let (bw, bh) = w.size();
            print_json(&serde_json::json!({
                "target": { "hwnd": format!("0x{:X}", w.hwnd), "title": w.title, "exe": w.exe, "dwm_bounds": w.bounds,
                            "window_rect": w.window_rect, "dpi": w.dpi, "monitor": m.map(|m| m.index) },
                "capture": meta, "format": frame.format_name(), "transfer": frame.transfer,
                "size": [frame.width, frame.height], "dwm_size": [bw, bh],
                "size_matches_dwm_bounds": (frame.width as i32 == bw && frame.height as i32 == bh),
                "white_nits": white, "result": png,
            }))
        }
        _ => Err("shot display <n> | shot window <hwnd|назва>".into()),
    }
}

fn find_window<'a>(wins: &'a [winlist::Window], what: &str) -> Option<&'a winlist::Window> {
    let h = what
        .strip_prefix("0x")
        .and_then(|x| isize::from_str_radix(x, 16).ok())
        .or_else(|| what.parse::<isize>().ok());
    if let Some(h) = h {
        return wins.iter().find(|w| w.hwnd == h);
    }
    let needle = what.to_lowercase();
    wins.iter()
        .find(|w| w.title.to_lowercase().contains(&needle))
}

/// Is the yellow capture border drawn on screen? Hold a WGC session on the window with
/// `IsBorderRequired` on|off, grab the monitor through DXGI meanwhile and count yellow pixels in a
/// ring along the window edge. "on" is the control run: it proves the detector sees a border.
fn border_probe(args: &[String]) -> Result<(), String> {
    let what = args.first().ok_or("border-probe <hwnd|назва> on|off")?;
    let border = match args.get(1).map(String::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => return Err("border-probe <hwnd|назва> on|off".into()),
    };
    let wins = winlist::list()?;
    let w = find_window(&wins, what).ok_or("вікно не знайдено")?;
    let monitors = display::list()?;
    // SAFETY: a window handle from the list.
    let hmon = unsafe { MonitorFromWindow(w.handle(), MONITOR_DEFAULTTONEAREST) };
    let m = monitors
        .iter()
        .find(|m| m.hmonitor == hmon.0 as isize)
        .ok_or("монітор вікна не знайдено")?;
    let held = wgc::hold(wgc::Target::Window(w.handle()), border)?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    let grabbed = dxgi::capture(m.handle());
    drop(held);
    let (frame, _) = grabbed?;
    let rgba = crate::tone::map_cpu(&frame, m.white());
    let (ox, oy) = (m.rect[0], m.rect[1]);
    let b = w.bounds;
    let (mut yellow, mut ring) = (0usize, 0usize);
    for y in (b[1] - 4)..(b[3] + 4) {
        for x in (b[0] - 4)..(b[2] + 4) {
            let near_edge = (x - b[0]).abs() <= 3
                || (x - (b[2] - 1)).abs() <= 3
                || (y - b[1]).abs() <= 3
                || (y - (b[3] - 1)).abs() <= 3;
            let (px, py) = (x - ox, y - oy);
            if !near_edge
                || px < 0
                || py < 0
                || px >= frame.width as i32
                || py >= frame.height as i32
            {
                continue;
            }
            ring += 1;
            let i = (py as usize * frame.width as usize + px as usize) * 4;
            let (r, g, bl) = (rgba[i], rgba[i + 1], rgba[i + 2]);
            if r >= 180 && g >= 140 && bl <= 90 {
                yellow += 1;
            }
        }
    }
    print_json(&serde_json::json!({
        "window": w.title, "dwm_bounds": b, "border_requested": border, "borderless_access": wgc::request_borderless(),
        "ring_pixels": ring, "yellow_pixels": yellow, "yellow_share": (yellow as f64 / ring.max(1) as f64 * 1000.0).round() / 1000.0,
    }))
}
