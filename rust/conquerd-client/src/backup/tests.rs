use super::*;
use crate::chat_store::{ChatMessage, MessageKind, MessageStatus};
use crate::peer_store::PeerRecord;
use crate::room_store::RoomEntry;

const PASSWORD: &[u8] = b"backup password independent of login";
const LOCAL_PASSWORD: &[u8] = b"new local password for restore";

fn fixture(root: &Path, include_attachments: bool) -> Result<BackupSnapshot> {
    let identity = Identity::from_seed(&[7; 32])?;
    identity.save_encrypted(b"original password plus unavailable keyfile", root)?;
    let mut peers = PeerStore::open(&identity, Some(&root.join("peers.dat")))?;
    peers.upsert(PeerRecord {
        peer_id: "friend".into(),
        identity_pub: Identity::from_seed(&[8; 32])?.public_id(),
        blocked: true,
        handle: "My friend".into(),
        ..Default::default()
    });
    peers.save()?;
    let mut rooms = RoomStore::open(&identity, Some(&root.join("my_rooms.dat")))?;
    rooms.add(RoomEntry::new("room-id", "Private room").with_supernode("host"))?;
    rooms.hide_from_sidebar("host", "room-id")?;
    let attachment = root.join("picture.bin");
    fs::write(&attachment, vec![42u8; CHUNK + 17])?;
    let chat = ChatStore::open(&identity, Some(&root.join("chat_history.db")))?;
    chat.insert(&ChatMessage {
        id: "message-1".into(),
        peer_id: "friend".into(),
        sender: "friend".into(),
        recipient: identity.public_id(),
        body: "a private message that must not appear in the archive".into(),
        timestamp: 10.0,
        is_self: false,
        status: MessageStatus::Delivered,
        kind: MessageKind::File,
        attachment_name: "picture.bin".into(),
        attachment_path: attachment.to_string_lossy().into_owned(),
        size_str: "1 MiB".into(),
        status_note: String::new(),
        sender_handle: "My friend".into(),
    })?;
    fs::write(root.join("settings.json"), br#"{"local_handle":"My name","avatar_config_json":"{}","input_device":"old microphone","ollama_auto_respond_direct":true,"plugin_path":"old.dll"}"#)?;
    BackupSnapshot::capture(
        BackupSource {
            identity: &identity,
            peers: &peers,
            rooms: &rooms,
            chat: &chat,
            directory: root,
        },
        include_attachments,
    )
}

#[test]
fn backup_restores_identity_wal_history_hidden_rooms_and_attachments() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let destination = tempfile::tempdir()?;
    let archive = source.path().join("backup.dbackup");
    let summary = fixture(source.path(), true)?.write(&archive, PASSWORD)?;
    assert_eq!(summary.messages, 1);
    assert_eq!(summary.attachments, 1);
    let bytes = fs::read(&archive)?;
    assert!(!bytes
        .windows(b"a private message".len())
        .any(|w| w == b"a private message"));
    let prepared = PreparedRestore::inspect(&archive, PASSWORD, destination.path())?;
    let restored = destination.path().join("restored");
    prepared.commit(&restored, LOCAL_PASSWORD)?;
    let identity = Identity::load_with_passphrase(LOCAL_PASSWORD, &restored)?;
    assert_eq!(identity.public_id(), summary.public_id);
    assert!(Identity::load_with_passphrase(PASSWORD, &restored).is_err());
    let peers = PeerStore::open(&identity, Some(&restored.join("peers.dat")))?;
    assert!(peers.get("friend").is_some_and(|p| p.blocked));
    let rooms = RoomStore::open(&identity, Some(&restored.join("my_rooms.dat")))?;
    assert!(rooms.is_hidden_from_sidebar("host", "room-id"));
    let chat = ChatStore::open(&identity, Some(&restored.join("chat_history.db")))?;
    let message = chat
        .get_by_id("message-1")?
        .ok_or_else(|| invalid("Missing restored message"))?;
    assert_eq!(
        message.body,
        "a private message that must not appear in the archive"
    );
    assert_eq!(fs::read(&message.attachment_path)?, vec![42u8; CHUNK + 17]);
    assert!(Path::new(&message.attachment_path).starts_with(&restored));
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(restored.join("settings.json"))?)?;
    assert_eq!(settings["local_handle"], "My name");
    assert!(settings.get("input_device").is_none());
    assert!(settings.get("ollama_auto_respond_direct").is_none());
    assert!(settings.get("plugin_path").is_none());
    assert_eq!(settings["video_enabled"], false);
    assert_eq!(settings["video_overlays_json"], "[]");
    Ok(())
}

#[test]
fn wrong_password_corruption_truncation_and_trailing_bytes_are_rejected() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let staging = tempfile::tempdir()?;
    let archive = source.path().join("backup.dbackup");
    fixture(source.path(), false)?.write(&archive, PASSWORD)?;
    assert!(PreparedRestore::inspect(&archive, b"wrong password", staging.path()).is_err());
    let bytes = fs::read(&archive)?;
    let broken = source.path().join("broken.dbackup");
    let mut tampered = bytes.clone();
    let offset = tampered.len() / 2;
    tampered[offset] ^= 0x80;
    fs::write(&broken, tampered)?;
    assert!(PreparedRestore::inspect(&broken, PASSWORD, staging.path()).is_err());
    fs::write(&broken, &bytes[..bytes.len() - 1])?;
    assert!(PreparedRestore::inspect(&broken, PASSWORD, staging.path()).is_err());
    let mut appended = bytes;
    appended.push(0);
    fs::write(&broken, appended)?;
    assert!(PreparedRestore::inspect(&broken, PASSWORD, staging.path()).is_err());
    assert_eq!(
        fs::read_dir(staging.path())?.count(),
        0,
        "failed imports must clean up staging"
    );
    Ok(())
}

#[test]
fn frames_cannot_be_reordered_or_read_with_unbounded_lengths() -> anyhow::Result<()> {
    let key = [9u8; 32];
    let header = [7u8; 40];
    let mut first = Vec::new();
    let mut index = 0;
    write_frame(&mut first, &key, &header, &mut index, b"first")?;
    assert!(read_frame(&mut first.as_slice(), &key, &header, &mut 1).is_err());
    let length = u32::MAX.to_be_bytes();
    assert!(read_frame(&mut length.as_slice(), &key, &header, &mut 0).is_err());
    Ok(())
}

#[test]
fn manifest_rejects_traversal_duplicates_and_missing_stores() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let snapshot = fixture(source.path(), false)?;
    let mut manifest = snapshot.manifest;
    manifest.entries[0].name = "../identity.dat".into();
    assert!(validate_manifest(&manifest).is_err());
    manifest.entries[0].name = "C:/identity.dat".into();
    assert!(validate_manifest(&manifest).is_err());
    manifest.entries[0].name = "my_rooms.dat".into();
    assert!(validate_manifest(&manifest).is_err());
    manifest.entries[0].name = "attachments/0".into();
    assert!(validate_manifest(&manifest).is_err());
    manifest.entries[0].name = "peers.dat".into();
    manifest.entries[0].size = (16 * CHUNK + 1) as u64;
    manifest.summary.bytes = manifest.entries.iter().map(|entry| entry.size).sum();
    assert!(validate_manifest(&manifest).is_err());
    Ok(())
}

#[test]
fn restore_preserves_existing_profiles_and_requires_preview_token() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let root = tempfile::tempdir()?;
    let archive = source.path().join("backup.dbackup");
    fixture(source.path(), false)?.write(&archive, PASSWORD)?;
    let original = Identity::from_seed(&[3; 32])?;
    original.save_encrypted(LOCAL_PASSWORD, root.path())?;
    let old_bytes = fs::read(root.path().join("identity.dat"))?;
    let service = BackupService::default();
    let run = |request: serde_json::Value, running| {
        service.run(&request.to_string(), root.path(), running, |_| {
            Err(invalid("No session"))
        })
    };
    let inspect_request = serde_json::json!({"cmd":"backup.inspect", "path":archive, "password":String::from_utf8_lossy(PASSWORD)});
    assert_eq!(run(inspect_request.clone(), true)["ok"], false);
    let preview = run(inspect_request, false);
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(
        run(
            serde_json::json!({"cmd":"backup.restore", "token":"wrong", "local_password":String::from_utf8_lossy(LOCAL_PASSWORD)}),
            false
        )["ok"],
        false
    );
    let reply = run(
        serde_json::json!({"cmd":"backup.restore", "token":preview["token"], "local_password":String::from_utf8_lossy(LOCAL_PASSWORD)}),
        false,
    );
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(fs::read(root.path().join("identity.dat"))?, old_bytes);
    let restored = selected_profile(root.path())?;
    assert_ne!(restored, root.path());
    let identity = Identity::load_with_passphrase(LOCAL_PASSWORD, &restored)?;
    let chat = ChatStore::open(&identity, Some(&restored.join("chat_history.db")))?;
    assert!(chat
        .get_by_id("message-1")?
        .is_some_and(|m| m.attachment_path.is_empty()));
    let reply = run(
        serde_json::json!({"cmd":"profile.select", "profile":"original"}),
        false,
    );
    assert_eq!(reply["ok"], true);
    assert_eq!(selected_profile(root.path())?, root.path());
    Ok(())
}

#[test]
fn export_does_not_overwrite_and_cancel_removes_prepared_restore() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let staging = tempfile::tempdir()?;
    let archive = source.path().join("backup.dbackup");
    let snapshot = fixture(source.path(), false)?;
    snapshot.write(&archive, PASSWORD)?;
    let original = fs::read(&archive)?;
    let second_source = tempfile::tempdir()?;
    assert!(fixture(second_source.path(), false)?
        .write(&archive, PASSWORD)
        .is_err());
    assert_eq!(fs::read(&archive)?, original);
    let prepared = PreparedRestore::inspect(&archive, PASSWORD, staging.path())?;
    assert_eq!(fs::read_dir(staging.path())?.count(), 1);
    drop(prepared);
    assert_eq!(fs::read_dir(staging.path())?.count(), 0);
    Ok(())
}

#[test]
fn changing_an_attachment_after_snapshot_prevents_publication() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let snapshot = fixture(source.path(), true)?;
    fs::write(source.path().join("picture.bin"), b"shortened")?;
    let archive = source.path().join("backup.dbackup");
    assert!(snapshot.write(&archive, PASSWORD).is_err());
    assert!(!archive.exists());
    Ok(())
}
