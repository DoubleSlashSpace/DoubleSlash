//! DoubleSlash client framework and desktop application services.
//!
//! The executable in `main.rs` is one consumer of these modules. The Qt/QML
//! frontend and headless integration mode select different parts of this
//! library through Cargo features.

pub mod aec;
pub mod agent_voice;
pub mod audio_devices;
pub mod avatar_config;
pub mod backup;
pub mod call_controller;
pub mod chat_store;
pub mod cluster;
pub mod connection_fallback;
pub mod connection_manager;
pub mod content_audio;
pub mod content_capture;
pub mod content_playout;
pub mod content_sender;
pub mod crypto;
pub mod device;
pub mod error;
pub mod feature_trust;
pub mod file_transfer;
pub mod github_updater;
pub mod group_key;
pub mod identity;
pub mod logging;
pub mod media_clock;
pub mod media_sync;
pub mod ollama_module;
pub mod ollama_share;
pub mod ollama_tools;
pub mod peer_store;
pub mod platform;
#[cfg(feature = "qt-ui")]
pub mod plugin_manager;
#[cfg(feature = "qt-ui")]
pub mod plugin_runtime;
pub mod protocol;
pub mod quic_relay_client;
pub mod quic_tls;
pub mod room_store;
pub mod session_state;
pub mod sfu_client;
pub mod space;
pub mod store_migration;
#[cfg(feature = "qt-ui")]
pub mod taskbar_badge;
#[cfg(feature = "qt-ui")]
pub mod ui;
pub mod upnp;
pub mod uri_scheme;
pub mod video;
pub mod web_app_client;

#[cfg(test)]
mod packaged_icons {
    use std::fs;
    use std::path::Path;

    /// Icons the QML names with a `qrc:` URL. The packaged binary loads
    /// `icons.qrc` and does not fall back to `qml/icons/` on disk, so a name
    /// that is only a file shows up as a blank button.
    #[test]
    fn every_qml_icon_is_packaged() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let qrc = fs::read_to_string(root.join("icons.qrc")).expect("read icons.qrc");
        let mut packaged = std::collections::BTreeSet::new();
        for line in qrc.lines() {
            let Some(rest) = line.trim().strip_prefix("<file alias=\"icons/") else {
                continue;
            };
            let Some((name, rest)) = rest.split_once("\">") else {
                continue;
            };
            let Some(path) = rest.strip_suffix("</file>") else {
                continue;
            };
            assert!(
                root.join(path).is_file(),
                "icons.qrc lists {name} at {path}, but that file is missing"
            );
            packaged.insert(name.to_string());
        }
        assert!(!packaged.is_empty(), "icons.qrc listed no icons");

        let marker = "qrc:/qt/qml/DoubleSlash/Client/icons/";
        let mut missing = Vec::new();
        for entry in fs::read_dir(root.join("qml")).expect("read qml/") {
            let path = entry.expect("qml entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("qml") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("read qml");
            let mut search = text.as_str();
            while let Some(at) = search.find(marker) {
                let after = &search[at + marker.len()..];
                let end = after
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.')
                    .unwrap_or(after.len());
                let name = &after[..end];
                if name.ends_with(".svg") && !packaged.contains(name) {
                    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    missing.push(format!("{file}: {name}"));
                }
                search = &after[end..];
            }
        }
        assert!(
            missing.is_empty(),
            "QML icons missing from icons.qrc:\n{}",
            missing.join("\n")
        );
    }
}
