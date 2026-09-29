//! The real shell path (CI only — it registers a shell extension for the current user): the DLL
//! registered per user, a `.znimok` file on disk, and the shell asked for its thumbnail through
//! `IShellItemImageFactory` — the same call Explorer makes; the handler runs in the shell's
//! isolated surrogate process.
#![cfg(windows)]

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify,
    SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
};
use windows::core::HSTRING;

#[test]
fn explorer_shows_the_stored_thumbnail() {
    if std::env::var_os("CI").is_none() && std::env::var_os("ZNIMOK_LIVE_SHELL").is_none() {
        eprintln!("skipped: registers a shell extension; runs on CI or with ZNIMOK_LIVE_SHELL=1");
        return;
    }
    // `cargo test` leaves the cdylib in target/<profile>/deps; `cargo build` in target/<profile>.
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let dll = [deps, deps.parent().unwrap()]
        .iter()
        .map(|d| d.join("znimok_thumbnail.dll"))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("znimok_thumbnail.dll not built next to {}", exe.display()));
    // COM ignores per-user class registrations in an elevated process (CI runners are): there
    // the test registers for the machine; a normal Explorer reads the per-user one.
    // SAFETY: plain query.
    let scope = if unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool() {
        znimok_thumbnail::Scope::Machine
    } else {
        znimok_thumbnail::Scope::User
    };
    znimok_thumbnail::register_in(scope, &dll.display().to_string(), r"Software\Classes").unwrap();
    // SAFETY: documented broadcast.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };

    let dir = std::env::temp_dir().join(format!("zk-thumb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Знімок.znimok");
    let doc = znimok_core::Document::from_raster(
        "t",
        znimok_core::Raster::new(64, 40, [255u8; 4].repeat(64 * 40)),
    );
    let thumb = znimok_core::Raster::new(160, 100, [20u8, 90, 220, 255].repeat(160 * 100));
    std::fs::write(
        &file,
        znimok_format::write(
            &doc,
            &znimok_format::WriteOptions {
                thumbnail: Some(thumb),
                ..Default::default()
            },
        ),
    )
    .unwrap();

    // SAFETY: COM on this thread; the shell item and bitmap are released below.
    let result = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let item: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(file.as_os_str()), None).unwrap();
        item.GetImage(SIZE { cx: 96, cy: 96 }, SIIGBF_THUMBNAILONLY)
            .map(|bmp| {
                let mut info = BITMAP::default();
                GetObjectW(
                    bmp.into(),
                    size_of::<BITMAP>() as i32,
                    Some((&mut info as *mut BITMAP).cast()),
                );
                let (w, h) = (info.bmWidth, info.bmHeight.abs());
                // Middle pixel, BGRA (the shell may return a bottom-up copy; the colour is uniform).
                let row = (h / 2) as usize * info.bmWidthBytes as usize;
                let px = std::slice::from_raw_parts(
                    info.bmBits.cast::<u8>().add(row + (w / 2) as usize * 4),
                    4,
                )
                .to_vec();
                let _ = DeleteObject(bmp.into());
                (w, h, px)
            })
    };
    let _ = znimok_thumbnail::unregister_in(scope, r"Software\Classes");
    // SAFETY: documented broadcast.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    let _ = std::fs::remove_dir_all(&dir);

    let (w, h, px) = result.expect("the shell gave no thumbnail");
    assert_eq!(w.max(h), 96, "{w}×{h}");
    let close = |a: u8, b: u8| a.abs_diff(b) <= 3;
    assert!(
        close(px[0], 220) && close(px[1], 90) && close(px[2], 20),
        "BGRA {px:?}"
    );
}
