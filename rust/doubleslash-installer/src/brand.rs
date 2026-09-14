//! User-facing product names for the installer.
//!
//! Crate names stay `doubleslash-installer`; only the Windows bundle, shortcuts,
//! and GUI strings use DoubleSlash.

pub const PRODUCT_NAME: &str = "DoubleSlash";
pub const WINDOWS_EXE: &str = "DoubleSlash.exe";
pub const WINDOWS_INSTALL_DIR: &str = "DoubleSlash";

/// True when `dir` contains a DoubleSlash client exe.
pub fn exe_in(dir: &std::path::Path) -> bool {
    dir.join(WINDOWS_EXE).is_file()
}
