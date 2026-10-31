//! Windows: embeds the app icon (Explorer, the taskbar, the tray) and version information.
fn main() {
    println!("cargo:rerun-if-changed=assets/dropcopy.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let v = std::env::var("CARGO_PKG_VERSION").unwrap();
    let n: Vec<u32> = v.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let (a, b, c) = (n[0], n[1], n[2]);
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("dropcopy.ico");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "DropCopy"
      VALUE "FileDescription", "DropCopy - share on your phone, Ctrl+V on this PC"
      VALUE "FileVersion", "{v}"
      VALUE "InternalName", "dropcopy"
      VALUE "OriginalFilename", "dropcopy.exe"
      VALUE "ProductName", "DropCopy"
      VALUE "ProductVersion", "{v}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/")
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dropcopy.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
