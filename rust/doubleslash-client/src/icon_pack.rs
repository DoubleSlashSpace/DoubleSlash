//! Desktop icon packs: a folder of icons that replace the packaged set.
//!
//! The UI asks for icons by the names in `icons.qrc`. A pack supplies those
//! file names as SVG, or the same stem as PNG (`phone.png` for `phone.svg`).
//! When one folder has both, the SVG is used. Anything left out, and anything
//! this loader will not accept, stays the packaged icon.
//!
//! `scripts/export_icon_pack.mjs` writes a starter folder. Settings stores
//! the folder path and asks [`describe`] for the file URLs QML should load.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

/// Largest icon file this loader will hand to the UI.
const MAX_ICON_BYTES: usize = 256 * 1024;

/// Largest width or height of a pack PNG. Bounds the decoded picture.
const MAX_PNG_PX: u32 = 2048;

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// What QML applies. `icons` maps a built-in file name to a `file:` URL.
#[derive(Debug, Serialize)]
struct PackReport {
    ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    name: String,
    count: usize,
    builtin: usize,
    icons: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    note: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<String>,
}

/// JSON for `Icons.applyDescribed`. An empty path is the built-in set.
pub fn describe(path: &str) -> String {
    match serde_json::to_string(&report(path.trim())) {
        Ok(json) => json,
        Err(_) => r#"{"ok":false,"error":"Could not describe that icon pack."}"#.to_string(),
    }
}

fn report(path: &str) -> PackReport {
    let names = builtin_icon_names();
    let builtin = names.len();
    if path.is_empty() {
        return PackReport {
            ok: true,
            error: String::new(),
            name: String::new(),
            count: 0,
            builtin,
            icons: BTreeMap::new(),
            note: String::new(),
            skipped: Vec::new(),
        };
    }

    let dir = Path::new(path);
    if !dir.exists() {
        return failed(builtin, "That folder is not there.");
    }
    if !dir.is_dir() {
        return failed(builtin, "Choose the folder that contains the icons.");
    }

    let manifest = match read_manifest(dir) {
        Ok(manifest) => manifest,
        Err(error) => return failed(builtin, &error),
    };
    let name = manifest
        .as_ref()
        .map(|m| clean_pack_name(&m.name, ""))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| dir_fallback_name(dir));

    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut skipped = Vec::new();
    if let Err(error) = collect_dir(dir, names, &mut found, &mut skipped) {
        return failed(builtin, &error);
    }
    let nested = dir.join("icons");
    if is_real_dir(&nested) {
        // The folder itself wins when the same name is in both places.
        let _ = collect_dir(&nested, names, &mut found, &mut skipped);
    }

    if found.is_empty() {
        let error = if skipped.is_empty() {
            "That folder has none of the built-in icon names."
        } else {
            "Those icons could not be used."
        };
        return failed(builtin, error);
    }

    let count = found.len();
    let icons = found
        .into_iter()
        .map(|(name, path)| (name, path_to_file_url(&path)))
        .collect();
    let note = if skipped.is_empty() {
        String::new()
    } else {
        "Some files were left out.".to_string()
    };
    PackReport {
        ok: true,
        error: String::new(),
        name,
        count,
        builtin,
        icons,
        note,
        skipped,
    }
}

fn failed(builtin: usize, error: &str) -> PackReport {
    PackReport {
        ok: false,
        error: error.to_string(),
        name: String::new(),
        count: 0,
        builtin,
        icons: BTreeMap::new(),
        note: String::new(),
        skipped: Vec::new(),
    }
}

struct Manifest {
    name: String,
}

fn read_manifest(dir: &Path) -> Result<Option<Manifest>, String> {
    let path = dir.join("pack.json");
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|_| "pack.json could not be read.".to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "pack.json could not be read.".to_string())?;
    let Some(obj) = value.as_object() else {
        return Err("pack.json could not be read.".to_string());
    };
    if let Some(version) = obj.get("v") {
        match version.as_u64() {
            Some(1) => {}
            _ => {
                return Err("This icon pack is not a version this DoubleSlash reads.".to_string());
            }
        }
    }
    let name = obj
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    Ok(Some(Manifest { name }))
}

#[derive(Clone, Copy)]
enum IconFile {
    Svg,
    Png,
}

/// `phone.svg` is the built-in name. `phone.png` fills that same name.
fn builtin_key(file_name: &str, names: &BTreeSet<String>) -> Option<(String, IconFile)> {
    let lower = file_name.to_ascii_lowercase();
    if names.contains(&lower) {
        return Some((lower, IconFile::Svg));
    }
    let stem = lower.strip_suffix(".png")?;
    let svg_name = format!("{stem}.svg");
    if names.contains(&svg_name) {
        Some((svg_name, IconFile::Png))
    } else {
        None
    }
}

fn collect_dir(
    dir: &Path,
    names: &BTreeSet<String>,
    found: &mut BTreeMap<String, PathBuf>,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|_| "That folder could not be read.".to_string())?;
    // SVG wins over PNG in this folder. A name already chosen by the caller
    // (the pack folder, before its `icons` subdirectory) stays chosen.
    let mut svg_here: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut png_here: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        // A link could point outside the folder the user chose.
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((key, kind)) = builtin_key(file_name, names) else {
            continue;
        };
        let slot = match kind {
            IconFile::Svg => &mut svg_here,
            IconFile::Png => &mut png_here,
        };
        if slot.contains_key(&key) {
            continue;
        }
        match read_icon(&path, meta.len(), kind) {
            Ok(()) => {
                slot.insert(key, path);
            }
            Err(why) => {
                if skipped.len() < 12 {
                    skipped.push(format!("{} ({why})", file_name.to_ascii_lowercase()));
                }
            }
        }
    }
    for (key, path) in svg_here {
        found.entry(key).or_insert(path);
    }
    for (key, path) in png_here {
        found.entry(key).or_insert(path);
    }
    Ok(())
}

fn read_icon(path: &Path, len: u64, kind: IconFile) -> Result<(), &'static str> {
    if len > MAX_ICON_BYTES as u64 {
        return Err("too large");
    }
    let bytes = fs::read(path).map_err(|_| "could not be read")?;
    let why = match kind {
        IconFile::Svg => svg_rejection(&bytes),
        IconFile::Png => png_rejection(&bytes),
    };
    if let Some(why) = why {
        Err(why)
    } else {
        Ok(())
    }
}

fn svg_rejection(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_ICON_BYTES || bytes.contains(&0) {
        return Some("not an svg");
    }
    let text = String::from_utf8_lossy(bytes);
    let lower = text.to_ascii_lowercase();
    let head = &lower[..lower.len().min(1024)];
    if !head.contains("<svg") {
        return Some("not an svg");
    }
    const BANNED: &[&str] = &[
        "<script",
        "javascript:",
        "<foreignobject",
        "<iframe",
        "<embed",
        "<handler",
        "<!doctype",
        "<!entity",
        "<image",
        "<animate",
        "<set ",
        "<set>",
        "<set/",
        "<?xml-stylesheet",
        "href=\"http",
        "href='http",
        "xlink:href=\"http",
        "xlink:href='http",
    ];
    if BANNED.iter().any(|needle| lower.contains(needle)) {
        Some("not allowed")
    } else {
        None
    }
}

fn png_rejection(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_ICON_BYTES
        || bytes.len() < PNG_SIG.len() + 25
        || bytes[..PNG_SIG.len()] != PNG_SIG
    {
        return Some("not a png");
    }
    let mut pos = PNG_SIG.len();
    let mut seen_idat = false;
    let mut seen_iend = false;
    let mut first = true;
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        let kind = [
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ];
        let data_at = pos + 8;
        let Some(after_data) = data_at.checked_add(len as usize) else {
            return Some("not a png");
        };
        let Some(next) = after_data.checked_add(4) else {
            return Some("not a png");
        };
        if next > bytes.len() {
            return Some("not a png");
        }
        if first {
            if kind != *b"IHDR" || len != 13 {
                return Some("not a png");
            }
            let width = u32::from_be_bytes([
                bytes[data_at],
                bytes[data_at + 1],
                bytes[data_at + 2],
                bytes[data_at + 3],
            ]);
            let height = u32::from_be_bytes([
                bytes[data_at + 4],
                bytes[data_at + 5],
                bytes[data_at + 6],
                bytes[data_at + 7],
            ]);
            if width == 0 || height == 0 {
                return Some("not a png");
            }
            if width > MAX_PNG_PX || height > MAX_PNG_PX {
                return Some("too large");
            }
            let depth = bytes[data_at + 8];
            let color = bytes[data_at + 9];
            if !png_ihdr_ok(depth, color)
                || bytes[data_at + 10] != 0
                || bytes[data_at + 11] != 0
                || bytes[data_at + 12] > 1
            {
                return Some("not a png");
            }
            first = false;
        } else if kind == *b"IDAT" {
            if len > 0 {
                seen_idat = true;
            }
        } else if kind == *b"IEND" {
            if len != 0 || next != bytes.len() {
                return Some("not a png");
            }
            seen_iend = true;
            break;
        }
        pos = next;
    }
    if seen_idat && seen_iend {
        None
    } else {
        Some("not a png")
    }
}

fn png_ihdr_ok(depth: u8, color: u8) -> bool {
    match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => false,
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        .unwrap_or(false)
}

fn dir_fallback_name(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    clean_pack_name(&raw, "Icon pack")
}

fn clean_pack_name(raw: &str, fallback: &str) -> String {
    let mut cleaned = String::new();
    for c in raw.chars() {
        if c.is_control() || is_bidi(c) {
            continue;
        }
        if c.is_whitespace() {
            if !cleaned.is_empty() && !cleaned.ends_with(' ') {
                cleaned.push(' ');
            }
            continue;
        }
        if cleaned.chars().count() >= 80 {
            break;
        }
        cleaned.push(c);
    }
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn is_bidi(c: char) -> bool {
    matches!(
        c,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

fn builtin_icon_names() -> &'static BTreeSet<String> {
    static NAMES: OnceLock<BTreeSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = BTreeSet::new();
        for line in include_str!("../icons.qrc").lines() {
            let Some(rest) = line.trim().strip_prefix("<file alias=\"icons/") else {
                continue;
            };
            let Some((name, _)) = rest.split_once('"') else {
                continue;
            };
            if name.ends_with(".svg") {
                names.insert(name.to_string());
            }
        }
        names
    })
}

fn path_to_file_url(path: &Path) -> String {
    let abs = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut text = abs.to_string_lossy().replace('\\', "/");
    if let Some(rest) = text.strip_prefix("//?/UNC/") {
        text = format!("//{rest}");
    } else if let Some(rest) = text.strip_prefix("//?/") {
        text = rest.to_string();
    }
    let encoded = percent_encode_path(&text);
    if encoded.starts_with("//") {
        format!("file:{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn percent_encode_path(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><path d="M1 1h2" fill="#C8C8C8"/></svg>"##;

    fn parsed(path: &str) -> serde_json::Value {
        serde_json::from_str(&describe(path)).expect("describe returns json")
    }

    #[test]
    fn builtin_names_are_the_packaged_svgs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qml/icons");
        let names = builtin_icon_names();
        assert!(names.contains("phone.svg"));
        assert!(names.contains("phone-hangup.svg"));
        for name in names {
            assert!(root.join(name).is_file(), "{name} is listed but missing");
        }
    }

    #[test]
    fn the_shipped_icons_all_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qml/icons");
        let report = parsed(&dir.to_string_lossy());
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["count"], report["builtin"]);
        assert_eq!(report["name"], "icons");
        let url = report["icons"]["phone.svg"].as_str().expect("phone.svg");
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.ends_with("/phone.svg"), "{url}");
        assert!(!url.contains('\\'), "{url}");
    }

    #[test]
    fn an_empty_path_is_the_built_in_set() {
        let report = parsed("   ");
        assert_eq!(report["ok"], true);
        assert_eq!(report["count"], 0);
        assert!(report["name"].as_str().unwrap_or("x").is_empty());
        assert!(report
            .get("icons")
            .and_then(|v| v.as_object())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let report = parsed("Z:/no/such/icon-pack-folder");
        assert_eq!(report["ok"], false);
        assert_eq!(report["error"], "That folder is not there.");
    }

    #[test]
    fn a_partial_pack_keeps_its_name_and_skips_the_rest() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            dir.path().join("pack.json"),
            r#"{"v":1,"name":"Hello\u202eEvil"}"#,
        )
        .expect("pack.json");
        fs::write(dir.path().join("Phone.svg"), SAFE).expect("phone");
        fs::write(
            dir.path().join("mic.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>nope</script></svg>"#,
        )
        .expect("mic");
        fs::write(dir.path().join("not-an-icon.svg"), SAFE).expect("extra");
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).expect("nested");
        fs::write(nested.join("gear.svg"), SAFE).expect("nested gear");
        let icons = dir.path().join("icons");
        fs::create_dir(&icons).expect("icons");
        fs::write(icons.join("gear.svg"), SAFE).expect("gear");
        fs::write(icons.join("phone.svg"), SAFE).expect("nested phone");

        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["name"], "HelloEvil");
        assert_eq!(report["count"], 2);
        assert!(report["icons"].get("phone.svg").is_some());
        assert!(report["icons"].get("gear.svg").is_some());
        assert!(report["icons"].get("mic.svg").is_none());
        assert!(report["icons"].get("not-an-icon.svg").is_none());
        let phone = report["icons"]["phone.svg"].as_str().unwrap();
        assert_eq!(phone, path_to_file_url(&dir.path().join("Phone.svg")));
        let gear = report["icons"]["gear.svg"].as_str().unwrap();
        assert_eq!(gear, path_to_file_url(&icons.join("gear.svg")));
        assert_eq!(report["note"], "Some files were left out.");
        let skipped = report["skipped"].as_array().expect("skipped");
        assert!(skipped
            .iter()
            .any(|v| v.as_str().unwrap_or("").contains("mic.svg")));
    }

    #[test]
    fn a_newer_manifest_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("pack.json"), r#"{"v":2,"name":"Later"}"#).expect("pack.json");
        fs::write(dir.path().join("phone.svg"), SAFE).expect("phone");
        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], false);
        assert_eq!(
            report["error"],
            "This icon pack is not a version this DoubleSlash reads."
        );
    }

    #[test]
    fn a_broken_manifest_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("pack.json"), "{").expect("pack.json");
        fs::write(dir.path().join("phone.svg"), SAFE).expect("phone");
        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], false);
        assert_eq!(report["error"], "pack.json could not be read.");
    }

    #[test]
    fn an_oversized_icon_does_not_load() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut body = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
        body.push_str(&"a".repeat(MAX_ICON_BYTES));
        body.push_str("</svg>");
        fs::write(dir.path().join("phone.svg"), body).expect("huge");
        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], false, "{report}");
        assert_eq!(report["error"], "Those icons could not be used.");
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("phone.svg");
        fs::write(&file, SAFE).expect("phone");
        let report = parsed(&file.to_string_lossy());
        assert_eq!(report["ok"], false);
        assert_eq!(
            report["error"],
            "Choose the folder that contains the icons."
        );
    }

    /// 1×1 RGBA PNG.
    const PNG_1X1: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I',
        b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn a_png_fills_the_matching_icon_name() {
        assert!(png_rejection(PNG_1X1).is_none());
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("Phone.PNG"), PNG_1X1).expect("png");
        fs::write(dir.path().join("gear.png"), b"not a png").expect("bad gear");
        fs::write(dir.path().join("not-an-icon.png"), PNG_1X1).expect("extra");
        let nested = dir.path().join("icons");
        fs::create_dir(&nested).expect("icons");
        // The pack folder already chose phone, so this SVG does not replace it.
        fs::write(nested.join("phone.svg"), SAFE).expect("nested svg");
        fs::write(nested.join("mic.png"), PNG_1X1).expect("mic png");

        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["count"], 2);
        assert_eq!(
            report["icons"]["phone.svg"].as_str().unwrap(),
            path_to_file_url(&dir.path().join("Phone.PNG"))
        );
        assert_eq!(
            report["icons"]["mic.svg"].as_str().unwrap(),
            path_to_file_url(&nested.join("mic.png"))
        );
        assert!(report["icons"].get("gear.svg").is_none());
        assert!(report["icons"].get("not-an-icon.svg").is_none());
        assert_eq!(report["note"], "Some files were left out.");
        let skipped = report["skipped"].as_array().expect("skipped");
        assert!(skipped
            .iter()
            .any(|v| v.as_str().unwrap_or("") == "gear.png (not a png)"));
    }

    #[test]
    fn an_svg_wins_when_the_folder_also_has_a_png() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("phone.png"), PNG_1X1).expect("png");
        fs::write(dir.path().join("phone.svg"), SAFE).expect("svg");
        let report = parsed(&dir.path().to_string_lossy());
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(report["count"], 1);
        assert_eq!(
            report["icons"]["phone.svg"].as_str().unwrap(),
            path_to_file_url(&dir.path().join("phone.svg"))
        );
        assert!(report.get("note").is_none());
        assert!(report.get("skipped").is_none());
    }

    #[test]
    fn a_png_wider_than_the_limit_is_refused() {
        let mut wide = PNG_1X1.to_vec();
        wide[16..20].copy_from_slice(&3000u32.to_be_bytes());
        assert_eq!(png_rejection(&wide), Some("too large"));
        assert_eq!(png_rejection(b"not a png"), Some("not a png"));
        let mut empty = PNG_1X1.to_vec();
        empty[16..20].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(png_rejection(&empty), Some("not a png"));
    }
}
