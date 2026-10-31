// Product version shared by the client, installer, supernode, and Android build scripts.
//
// The stamped version is `{major}.{minor}.{build}.0`.
// `major` and `minor` come from this crate's Cargo.toml.
// `build` is `DOUBLESLASH_BUILD_NUMBER`, or `git rev-list --count HEAD` on a full clone.
// The fourth component stays 0. That is the Microsoft Store package identity, and every
// other surface reports the same string. A shallow clone with no
// `DOUBLESLASH_BUILD_NUMBER` stamps build 0.

use std::process::Command;

pub fn stamped_version() -> String {
    println!("cargo:rerun-if-env-changed=DOUBLESLASH_BUILD_NUMBER");
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_else(|_| "0".into());
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_else(|_| "0".into());
    let build = build_number();
    format!("{major}.{minor}.{build}.0")
}

fn build_number() -> u64 {
    if let Ok(raw) = std::env::var("DOUBLESLASH_BUILD_NUMBER") {
        let raw = raw.trim();
        if !raw.is_empty() {
            return parse_build(raw);
        }
    }
    if shallow_repo() {
        println!(
            "cargo:warning=shallow git checkout has no build number; stamping 0. Set DOUBLESLASH_BUILD_NUMBER."
        );
        return 0;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    match Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output()
    {
        Ok(out) if out.status.success() => parse_build(String::from_utf8_lossy(&out.stdout).trim()),
        _ => 0,
    }
}

fn parse_build(raw: &str) -> u64 {
    let n: u64 = raw.parse().unwrap_or_else(|_| {
        panic!("build number must be an integer, got {raw:?}");
    });
    if n > 65535 {
        panic!("build number {n} exceeds the 65535 limit of Windows and Microsoft Store versions");
    }
    n
}

fn shallow_repo() -> bool {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    Command::new("git")
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|text| text.trim() == "true")
}

/// Four 16-bit Windows version words packed the way `winresource` expects.
#[cfg(windows)]
pub fn file_version_u64(stamp: &str) -> u64 {
    let mut words = [0u64; 4];
    for (index, part) in stamp.split('.').take(4).enumerate() {
        words[index] = part.parse().unwrap_or(0);
    }
    (words[0] << 48) | (words[1] << 32) | (words[2] << 16) | words[3]
}
