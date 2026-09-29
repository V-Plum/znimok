// Windows resources for Znimok's executables (included by the build scripts of znimok-app and
// znimok-cli): the icon and a VERSIONINFO block. Explorer, the taskbar, Alt+Tab and the Start
// menu show the icon; the version block is part of the "legitimate program" profile that keeps
// antivirus heuristics calm (the Little Helpers lesson: an exe without it drew Wacatac.B!ml).

/// Compiles the resources into the binary being built — only when the target is Windows.
#[allow(dead_code)]
fn windows_resources(description: &str, original_name: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = dir
        .join("../znimok-app/icons/znimok.ico")
        .canonicalize()
        .expect("znimok-app/icons/znimok.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let (a, b, c) = (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
        nums.get(2).copied().unwrap_or(0),
    );
    let ico_path = ico.display().to_string().replace('\\', "\\\\");
    // The \\?\ prefix from canonicalize() confuses rc.exe.
    let ico_path = ico_path.trim_start_matches("\\\\\\\\?\\\\").to_string();
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{ico_path}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Vadym Slyva"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{original_name}"
      VALUE "LegalCopyright", "(c) 2026 Vadym Slyva"
      VALUE "OriginalFilename", "{original_name}"
      VALUE "ProductName", "Znimok"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("znimok.rc");
    std::fs::write(&out, rc).expect("write znimok.rc");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .expect("Windows resources (rc.exe)");
}
