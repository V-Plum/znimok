//! The icon handler through COM, as Explorer uses it (ZK-150; CI only — it registers a shell
//! extension): the class is created by its CLSID, loads a `.znimok` file by path and names the
//! `.ico` of its kind — a screenshot, a video, a video with a DevTools log.
#![cfg(windows)]

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::UI::Shell::{GIL_PERINSTANCE, IExtractIconW};
use windows::core::{GUID, HSTRING, Interface};

fn write_samples(dir: &std::path::Path) -> [std::path::PathBuf; 3] {
    let raster = znimok_core::Raster::new(64, 40, vec![255; 64 * 40 * 4]);
    let doc = znimok_core::Document::from_raster("t", raster);
    let info = znimok_format::VideoInfo {
        width: 64,
        height: 40,
        fps_milli: 30_000,
        frames: 30,
        duration_hns: 10_000_000,
        codec: *b"avc1",
    };
    let mut video = znimok_format::Video::new(info);
    let mp4 = vec![0u8; 64];
    let shot = dir.join("shot.znimok");
    std::fs::write(&shot, znimok_format::write(&doc, &Default::default())).unwrap();
    let clip = dir.join("clip.znimok");
    std::fs::write(
        &clip,
        znimok_format::write_video(&doc, &video, &mp4, &Default::default()),
    )
    .unwrap();
    video.devlog = Some(znimok_format::DevLog {
        wall0_ms: 1,
        events: Vec::new(),
    });
    let report = dir.join("report.znimok");
    std::fs::write(
        &report,
        znimok_format::write_video(&doc, &video, &mp4, &Default::default()),
    )
    .unwrap();
    [shot, clip, report]
}

#[test]
fn each_kind_gets_its_icon() {
    if std::env::var_os("CI").is_none() && std::env::var_os("ZNIMOK_LIVE_SHELL").is_none() {
        eprintln!("skipped: registers a shell extension; runs on CI or with ZNIMOK_LIVE_SHELL=1");
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let dll = [deps, deps.parent().unwrap()]
        .iter()
        .map(|d| d.join("znimok_thumbnail.dll"))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("znimok_thumbnail.dll not built next to {}", exe.display()));
    // SAFETY: plain query.
    let scope = if unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool() {
        znimok_thumbnail::Scope::Machine
    } else {
        znimok_thumbnail::Scope::User
    };
    znimok_thumbnail::register_in(scope, &dll.display().to_string(), r"Software\Classes").unwrap();

    let dir = std::env::temp_dir().join(format!("znimok-icons-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = write_samples(&dir);
    // The same GUID as ICON_CLSID_STR.
    let clsid = GUID::from_u128(0x0f5c6e12_2a39_4aac_9c34_3e2c9305e05a);
    assert_eq!(format!("{{{clsid:?}}}"), znimok_thumbnail::ICON_CLSID_STR);
    let mut got = Vec::new();
    // SAFETY: COM on this thread; interfaces used while the object lives.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        for f in &files {
            let pf: IPersistFile = CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER).unwrap();
            pf.Load(&HSTRING::from(f.as_os_str()), STGM_READ).unwrap();
            let ex: IExtractIconW = pf.cast().unwrap();
            let mut buf = [0u16; 1024];
            let (mut index, mut flags) = (0i32, 0u32);
            ex.GetIconLocation(0, &mut buf, &mut index, &mut flags)
                .unwrap();
            let n = buf.iter().position(|&c| c == 0).unwrap();
            let path = String::from_utf16_lossy(&buf[..n]);
            assert_eq!(index, 0);
            assert_eq!(flags & GIL_PERINSTANCE, GIL_PERINSTANCE);
            got.push(path);
        }
    }
    let _ = znimok_thumbnail::unregister_in(scope, r"Software\Classes");
    let _ = std::fs::remove_dir_all(&dir);
    let names: Vec<&str> = got.iter().map(|p| p.rsplit('\\').next().unwrap()).collect();
    assert_eq!(
        names,
        ["doc-image.ico", "doc-video.ico", "doc-report.ico"],
        "{got:?}"
    );
    assert!(got.iter().all(|p| p.contains(r"\icons\")), "{got:?}");
}
