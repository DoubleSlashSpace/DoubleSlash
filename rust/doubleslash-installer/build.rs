include!("../build_version.rs");

fn main() {
    let product_version = stamped_version();
    println!("cargo:rustc-env=DOUBLESLASH_VERSION={product_version}");

    #[cfg(windows)]
    {
        // Embed an explicit "asInvoker" execution-level manifest so that Windows
        // UAC heuristics (which auto-elevate binaries whose names contain words
        // like "install", "setup", or "update") do not trigger a UAC prompt.
        // The installer writes only to %LOCALAPPDATA%\DoubleSlash and user-owned
        // shortcuts, so no elevation is required or desired.
        const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>"#;

        let packed = file_version_u64(&product_version);
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/doubleslash.ico");
        res.set("ProductName", "DoubleSlash");
        res.set("FileDescription", "DoubleSlash Installer / Updater");
        res.set("LegalCopyright", "DoubleSlash Project");
        res.set("ProductVersion", &product_version);
        res.set("FileVersion", &product_version);
        res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
        res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
        res.set_manifest(MANIFEST);
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to set Windows resource: {e}");
        }
    }
}
