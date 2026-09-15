//! Opens profiles written before the store-label rename.
//!
//! Commit 8d496f7 renamed the at-rest subkey labels from `conquerd-store/*` to
//! `doubleslash-store/*`. Each key is HKDF(identity, label), so profiles written
//! earlier stopped decrypting and looked empty. The stores call in here as they
//! open: data that reads only under the old label is re-encrypted under the
//! current one, so the fallback does real work at most once per profile.
//!
//! Pre-release only; delete once no pre-rename profile remains.

use std::io::Write;
use std::path::Path;

use tracing::{info, warn};
use zeroize::Zeroizing;

use crate::crypto::{decrypt_blob, encrypt_blob};
use crate::error::Result;
use crate::identity::Identity;

/// Current label, and the label the same store used before the rename.
const LEGACY_LABELS: &[(&str, &str)] = &[
    (
        crate::peer_store::PEER_STORE_LABEL,
        "conquerd-store/peers/v1",
    ),
    (
        crate::room_store::ROOM_STORE_LABEL,
        "conquerd-store/rooms/v1",
    ),
    (
        crate::chat_store::CHAT_STORE_LABEL,
        "conquerd-store/chat/v1",
    ),
    (crate::device::KEY_LABEL, "conquerd-store/device-key/v1"),
];

/// The pre-rename key for `current_label`, if that store existed before it.
pub(crate) fn legacy_key(
    identity: &Identity,
    current_label: &str,
) -> Result<Option<Zeroizing<[u8; 32]>>> {
    LEGACY_LABELS
        .iter()
        .find(|(current, _)| *current == current_label)
        .map(|(_, legacy)| identity.derive_store_key(legacy).map(Zeroizing::new))
        .transpose()
}

/// `blob` re-encrypted under `current` when it opens only under `legacy`;
/// `None` when it already opens under `current`, or under neither.
pub(crate) fn reencrypt(
    blob: &[u8],
    current: &[u8; 32],
    legacy: &[u8; 32],
) -> Result<Option<Vec<u8>>> {
    if decrypt_blob(current, blob).is_ok() {
        return Ok(None);
    }
    match decrypt_blob(legacy, blob) {
        Ok(plaintext) => encrypt_blob(current, &Zeroizing::new(plaintext)).map(Some),
        Err(_) => Ok(None),
    }
}

/// Re-encrypt a whole-file store in place if it opens only under the legacy
/// label. Never fails the caller: a store that still does not open reports
/// that itself.
pub(crate) fn upgrade_file(identity: &Identity, path: &Path, current_label: &str) -> bool {
    match try_upgrade_file(identity, path, current_label) {
        Ok(rewritten) => rewritten,
        Err(e) => {
            warn!(
                "could not re-encrypt {} under {current_label}: {e}",
                path.display()
            );
            false
        }
    }
}

fn try_upgrade_file(identity: &Identity, path: &Path, current_label: &str) -> Result<bool> {
    let Some(legacy) = legacy_key(identity, current_label)? else {
        return Ok(false);
    };
    let envelope = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let current = Zeroizing::new(identity.derive_store_key(current_label)?);
    let Some(upgraded) = reencrypt(&envelope, &current, &legacy)? else {
        return Ok(false);
    };
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(dir)?;
    staged.write_all(&upgraded)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|e| e.error)?;
    info!("re-encrypted {} under {current_label}", path.display());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_store::{ChatStore, CHAT_STORE_LABEL};
    use crate::device::DeviceKey;
    use crate::peer_store::PEER_STORE_LABEL;
    use crate::room_store::{RoomEntry, RoomStore, ROOM_STORE_LABEL};
    use tempfile::tempdir;

    fn legacy(identity: &Identity, current_label: &str) -> Zeroizing<[u8; 32]> {
        legacy_key(identity, current_label)
            .unwrap()
            .expect("store predates the rename")
    }

    #[test]
    fn pre_rename_file_is_reencrypted_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("peers.dat");
        let identity = Identity::generate();
        let plaintext = br#"{"version":1,"peers":[]}"#;
        let old = legacy(&identity, PEER_STORE_LABEL);
        std::fs::write(&path, encrypt_blob(&old[..], plaintext).unwrap()).unwrap();

        assert!(upgrade_file(&identity, &path, PEER_STORE_LABEL));
        let current = identity.derive_store_key(PEER_STORE_LABEL).unwrap();
        let envelope = std::fs::read(&path).unwrap();
        assert_eq!(decrypt_blob(&current, &envelope).unwrap(), plaintext);
        assert!(
            !upgrade_file(&identity, &path, PEER_STORE_LABEL),
            "a current file is left alone"
        );
    }

    #[test]
    fn missing_and_unreadable_files_are_untouched() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("peers.dat");
        let identity = Identity::generate();
        assert!(!upgrade_file(&identity, &path, PEER_STORE_LABEL));
        assert!(!path.exists());

        std::fs::write(&path, b"not an envelope").unwrap();
        assert!(!upgrade_file(&identity, &path, PEER_STORE_LABEL));
        assert_eq!(std::fs::read(&path).unwrap(), b"not an envelope");
    }

    #[test]
    fn pre_rename_room_store_opens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("my_rooms.dat");
        let identity = Identity::generate();
        RoomStore::open_with_key(&legacy(&identity, ROOM_STORE_LABEL), &path)
            .unwrap()
            .add(RoomEntry::new("room-id", "Kept room").with_supernode("host"))
            .unwrap();

        let rooms = RoomStore::open(&identity, Some(&path)).unwrap();
        assert_eq!(rooms.list().len(), 1);
    }

    #[test]
    fn pre_rename_device_key_loads() {
        // A device key file that does not decrypt fails closed, so without
        // the upgrade this profile could not start at all.
        let dir = tempdir().unwrap();
        let identity = Identity::generate();
        let mut plaintext = identity.public_key_bytes().to_vec();
        plaintext.extend([7u8; 32]);
        let old = legacy(&identity, crate::device::KEY_LABEL);
        let envelope = encrypt_blob(&old[..], &plaintext).unwrap();
        std::fs::write(dir.path().join("device-key.dat"), envelope).unwrap();

        assert!(DeviceKey::load_or_create(&identity, dir.path()).is_ok());
    }

    #[test]
    fn pre_rename_chat_rows_read_after_open() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("chat.db");
        let identity = Identity::generate();
        let old = legacy(&identity, CHAT_STORE_LABEL);
        drop(ChatStore::open_with_key(&old, &db).unwrap()); // creates the schema

        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO messages (id, peer_id, sender, recipient, body, timestamp, sender_handle)
             VALUES ('m1', 'peer-1', 'a', 'b', ?1, 1.0, ?2)",
            rusqlite::params![
                encrypt_blob(&old[..], b"written before the rename").unwrap(),
                encrypt_blob(&old[..], b"alice").unwrap()
            ],
        )
        .unwrap();
        drop(conn);

        let store = ChatStore::open(&identity, Some(&db)).unwrap();
        let history = store.get_history("peer-1", 0).unwrap();
        assert_eq!(history[0].body, "written before the rename");
        assert_eq!(history[0].sender_handle, "alice");
    }
}
