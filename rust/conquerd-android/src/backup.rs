//! Backup commands also work before nativeStart (handle zero), through the
//! existing JSON boundary. Long exports own store Arcs rather than a session
//! pointer, allowing Disconnect to stop transport and capture immediately.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use conquerd_client::backup::{BackupService, BackupSnapshot, BackupSource};
use conquerd_client::{
    chat_store::ChatStore, identity::Identity, peer_store::PeerStore, room_store::RoomStore,
};
use parking_lot::RwLock;
use serde_json::{json, Value};

pub struct Context {
    identity: Arc<Identity>,
    peers: Arc<RwLock<PeerStore>>,
    rooms: Arc<RwLock<RoomStore>>,
    chat: Arc<ChatStore>,
    directory: PathBuf,
}

impl Context {
    pub fn capture(session: &crate::session::Session) -> Self {
        Self {
            identity: Arc::clone(&session.identity),
            peers: Arc::clone(&session.peer_store),
            rooms: Arc::clone(&session.room_store),
            chat: Arc::clone(&session.chat_store),
            directory: Identity::default_key_dir(),
        }
    }
}

pub fn dispatch(request: &str, context: Option<Context>) -> Value {
    static SERVICE: OnceLock<BackupService> = OnceLock::new();
    let Ok(value) = serde_json::from_str::<Value>(request) else {
        return json!({"ok": false, "error": "Invalid backup command"});
    };
    let Some(root) = value["home_dir"].as_str() else {
        return json!({"ok": false, "error": "Missing profile directory"});
    };
    SERVICE.get_or_init(BackupService::default).run(
        request,
        std::path::Path::new(root),
        context.is_some(),
        |attachments| {
            let context = context.ok_or_else(|| {
                conquerd_client::error::ClientError::Identity(
                    "Unlock your identity before creating a backup".into(),
                )
            })?;
            if let Some(settings) = value.get("android_settings").filter(|v| v.is_object()) {
                std::fs::write(
                    context.directory.join("android-settings.json"),
                    serde_json::to_vec(settings)?,
                )?;
            }
            let peers = context.peers.read();
            let rooms = context.rooms.read();
            BackupSnapshot::capture(
                BackupSource {
                    identity: &context.identity,
                    peers: &peers,
                    rooms: &rooms,
                    chat: &context.chat,
                    directory: &context.directory,
                },
                attachments,
            )
        },
    )
}
