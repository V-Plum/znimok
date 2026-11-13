//! Shared by the shell tests: where cargo left the cdylib the shell is to load.
#![cfg(windows)]

use std::path::PathBuf;

/// `znimok_thumbnail.dll` as this test build sees it. `cargo test` leaves the cdylib in
/// target/<profile>/deps and `cargo build` in target/<profile>; cargo 1.100's build-dir layout
/// keeps each unit apart — the test in build/znimok-thumbnail/<hash>/out, the cdylib in a sibling
/// <hash2>/out (the newest is ours: an older hash is a build of another configuration).
pub fn thumbnail_dll() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let mut found: Vec<PathBuf> = [dir, dir.parent().unwrap()]
        .iter()
        .map(|d| d.join("znimok_thumbnail.dll"))
        .filter(|p| p.exists())
        .collect();
    if found.is_empty()
        && let Some(units) = dir.parent().and_then(|h| h.parent())
        && let Ok(rd) = std::fs::read_dir(units)
    {
        found = rd
            .flatten()
            .map(|e| e.path().join("out").join("znimok_thumbnail.dll"))
            .filter(|p| p.exists())
            .collect();
        found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        found.reverse();
    }
    found
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("znimok_thumbnail.dll not built next to {}", exe.display()))
}
