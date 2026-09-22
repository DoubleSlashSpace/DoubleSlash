//! Portable, authenticated profile backups. No OS keyring or original unlock
//! credential is required to restore. Each bounded frame authenticates the
//! format header and its position; a final authenticated marker detects truncation.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use zeroize::Zeroizing;

use crate::chat_store::ChatStore;
use crate::crypto::{aesgcm_decrypt, aesgcm_encrypt, argon2id_kdf, decrypt_blob};
use crate::device::{DeviceTrustStore, DEVICE_TRUST_FILE};
use crate::error::{ClientError, Result};
use crate::identity::Identity;
use crate::peer_store::PeerStore;
use crate::room_store::RoomStore;

mod service;
pub use service::{selected_profile, BackupService};

const MAGIC: &[u8; 8] = b"DSSHBA01";
const CHUNK: usize = 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024 * 1024;
const END: &[u8] = b"DoubleSlash backup complete";

fn invalid(message: &str) -> ClientError {
    ClientError::Store(message.to_owned())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupSummary {
    pub public_id: String,
    pub created_at: u64,
    pub files: usize,
    pub bytes: u64,
    pub messages: u64,
    pub attachments: usize,
    pub missing_attachments: usize,
    pub includes_attachments: bool,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    size: u64,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    summary: BackupSummary,
    entries: Vec<Entry>,
}

/// Borrowed live stores. Callers hold peer and room read locks while taking
/// the snapshot, then release them before encryption or attachment copying.
pub struct BackupSource<'a> {
    pub identity: &'a Identity,
    pub peers: &'a PeerStore,
    pub rooms: &'a RoomStore,
    pub chat: &'a ChatStore,
    pub directory: &'a Path,
}

/// Frozen store snapshot; attachments are streamed from their source files.
pub struct BackupSnapshot {
    directory: TempDir,
    identity_seed: Zeroizing<[u8; 32]>,
    manifest: Manifest,
    sources: Vec<PathBuf>,
}

impl BackupSnapshot {
    pub fn capture(source: BackupSource<'_>, include_attachments: bool) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("doubleslash-backup-")
            .tempdir_in(source.directory)?;
        fs::write(
            directory.path().join("peers.dat"),
            source.peers.backup_snapshot()?,
        )?;
        fs::write(
            directory.path().join("my_rooms.dat"),
            source.rooms.backup_snapshot()?,
        )?;
        let db_path = directory.path().join("chat_history.db");
        source.chat.backup_snapshot(&db_path)?;
        let db = rusqlite::Connection::open(&db_path)?;
        let messages = db.query_row("SELECT count(*) FROM messages", [], |r| r.get(0))?;
        let paths: Vec<String> = db
            .prepare("SELECT DISTINCT attachment_path FROM messages WHERE attachment_path != ''")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut attachments = Vec::new();
        let mut missing = 0;
        for path in paths {
            let candidate = PathBuf::from(&path);
            let metadata = fs::symlink_metadata(&candidate).ok();
            let available = metadata.as_ref().is_some_and(|m| m.is_file());
            let replacement = if include_attachments && available {
                let name = format!("attachments/{}", attachments.len());
                attachments.push((name.clone(), candidate));
                name
            } else {
                if !available {
                    missing += 1;
                }
                String::new()
            };
            db.execute(
                "UPDATE messages SET attachment_path=?1 WHERE attachment_path=?2",
                [&replacement, &path],
            )?;
        }
        db.close().map_err(|(_, e)| e)?;
        let mut names = vec![
            "peers.dat".to_owned(),
            "my_rooms.dat".to_owned(),
            "chat_history.db".to_owned(),
        ];
        let device_trust = source.directory.join(DEVICE_TRUST_FILE);
        if device_trust.try_exists()? {
            DeviceTrustStore::read_existing(source.identity, &device_trust)?
                .backup_snapshot(&directory.path().join(DEVICE_TRUST_FILE))?;
            names.push(DEVICE_TRUST_FILE.to_owned());
        }
        for name in ["settings.json", "android-settings.json"] {
            let path = source.directory.join(name);
            if path.exists() || name == "settings.json" {
                let mut value: serde_json::Value = if path.exists() {
                    serde_json::from_slice(&read_small(&path)?)?
                } else {
                    serde_json::json!({})
                };
                // Android keeps the current public profile on its own peer
                // record. Carry it into desktop settings even when there was
                // never a Qt settings file on this device.
                if name == "settings.json" {
                    let map = value
                        .as_object_mut()
                        .ok_or_else(|| invalid("Invalid settings document"))?;
                    if let Some(own) = source.peers.get_by_identity(&source.identity.public_id()) {
                        map.insert("local_handle".into(), own.handle.clone().into());
                        if let Some(avatar) = &own.avatar_config {
                            map.insert(
                                "avatar_config_json".into(),
                                serde_json::to_string(avatar)?.into(),
                            );
                        }
                    }
                }
                fs::write(directory.path().join(name), serde_json::to_vec(&value)?)?;
                names.push(name.to_owned());
            }
        }
        let mut sources: Vec<_> = names.iter().map(|n| directory.path().join(n)).collect();
        let attachment_count = attachments.len();
        for (name, path) in attachments {
            names.push(name);
            sources.push(path);
        }
        let entries: Vec<_> = names
            .into_iter()
            .zip(&sources)
            .map(|(name, path)| {
                Ok(Entry {
                    name,
                    size: fs::metadata(path)?.len(),
                })
            })
            .collect::<Result<_>>()?;
        let bytes = entries.iter().try_fold(0u64, |total, e| {
            total
                .checked_add(e.size)
                .ok_or_else(|| invalid("Backup is too large"))
        })?;
        let manifest = Manifest {
            summary: BackupSummary {
                public_id: source.identity.public_id(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                files: entries.len(),
                bytes,
                messages,
                attachments: attachment_count,
                missing_attachments: missing,
                includes_attachments: include_attachments,
            },
            entries,
        };
        validate_manifest(&manifest)?;
        Ok(Self {
            directory,
            identity_seed: source.identity.private_key_bytes(),
            manifest,
            sources,
        })
    }

    /// Publish only after re-reading and authenticating the entire encrypted file.
    /// Existing destination files are never overwritten.
    pub fn write(self, destination: &Path, password: &[u8]) -> Result<BackupSummary> {
        check_password(password)?;
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| invalid("Choose an absolute backup path"))?;
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        let mut header = [0u8; 40];
        header[..8].copy_from_slice(MAGIC);
        OsRng.fill_bytes(&mut header[8..]);
        output.write_all(&header)?;
        let key = backup_key(password, &header)?;
        let mut index = 0u64;
        write_frame(
            &mut output,
            &key[..],
            &header,
            &mut index,
            &serde_json::to_vec(&self.manifest)?,
        )?;
        write_frame(
            &mut output,
            &key[..],
            &header,
            &mut index,
            self.identity_seed.as_ref(),
        )?;
        let mut buffer = Zeroizing::new(vec![0u8; CHUNK]);
        for (entry, path) in self.manifest.entries.iter().zip(&self.sources) {
            let mut input = File::open(path)?;
            let before = input.metadata()?;
            let mut remaining = entry.size;
            while remaining > 0 {
                let size = remaining.min(CHUNK as u64) as usize;
                input.read_exact(&mut buffer[..size])?;
                write_frame(&mut output, &key[..], &header, &mut index, &buffer[..size])?;
                remaining -= size as u64;
            }
            if input.read(&mut [0u8; 1])? != 0
                || before.modified().ok() != input.metadata()?.modified().ok()
            {
                return Err(invalid("A file changed during backup. Please try again."));
            }
        }
        write_frame(&mut output, &key[..], &header, &mut index, END)?;
        output.as_file().sync_all()?;
        // Re-open all frames without extracting a second copy of attachments.
        read_archive(output.path(), password, None)?;
        output
            .persist_noclobber(destination)
            .map_err(|e| ClientError::Io(e.error))?;
        // Keep the temporary snapshot alive until all sources have been read.
        drop(self.directory);
        Ok(self.manifest.summary)
    }
}

fn check_password(password: &[u8]) -> Result<()> {
    if password.len() < 12 || password.len() > 4096 {
        return Err(invalid(
            "Use a backup password of at least 12 characters (maximum 4096 bytes).",
        ));
    }
    Ok(())
}

fn backup_key(password: &[u8], header: &[u8; 40]) -> Result<Zeroizing<[u8; 32]>> {
    // Fixed v1 parameters bound resource use even for an attacker-crafted header.
    Ok(Zeroizing::new(argon2id_kdf(
        password,
        &header[8..],
        3,
        65536,
        4,
    )?))
}

fn aad(header: &[u8; 40], index: u64) -> [u8; 48] {
    let mut aad = [0u8; 48];
    aad[..40].copy_from_slice(header);
    aad[40..].copy_from_slice(&index.to_be_bytes());
    aad
}

fn write_frame(
    output: &mut impl Write,
    key: &[u8],
    header: &[u8; 40],
    index: &mut u64,
    bytes: &[u8],
) -> Result<()> {
    if bytes.len() > CHUNK {
        return Err(invalid("Backup manifest exceeds the supported size"));
    }
    let (nonce, ciphertext) = aesgcm_encrypt(key, bytes, &aad(header, *index))?;
    output.write_all(&((nonce.len() + ciphertext.len()) as u32).to_be_bytes())?;
    output.write_all(&nonce)?;
    output.write_all(&ciphertext)?;
    *index += 1;
    Ok(())
}

fn read_frame(
    input: &mut impl Read,
    key: &[u8],
    header: &[u8; 40],
    index: &mut u64,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if !(28..=CHUNK + 28).contains(&length) {
        return Err(invalid("Invalid backup frame length"));
    }
    let mut frame = vec![0u8; length];
    input.read_exact(&mut frame)?;
    let plaintext = aesgcm_decrypt(key, &frame[..12], &frame[12..], &aad(header, *index))
        .map_err(|_| invalid("Incorrect backup password or damaged backup"))?;
    *index += 1;
    Ok(Zeroizing::new(plaintext))
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.entries.len() > MAX_FILES {
        return Err(invalid("Too many backup files"));
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for entry in &manifest.entries {
        if entry.name == DEVICE_TRUST_FILE && entry.size > 512 * 1024 * 1024 {
            return Err(invalid("Device trust database is too large"));
        }
        if entry.name != "chat_history.db"
            && entry.name != DEVICE_TRUST_FILE
            && !entry.name.starts_with("attachments/")
            && entry.size > (16 * CHUNK) as u64
        {
            return Err(invalid("Profile metadata is too large"));
        }
        let valid = matches!(
            entry.name.as_str(),
            "peers.dat"
                | "my_rooms.dat"
                | "chat_history.db"
                | "settings.json"
                | "android-settings.json"
                | DEVICE_TRUST_FILE
        ) || entry.name.strip_prefix("attachments/").is_some_and(|n| {
            !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit())
        });
        if !valid || !seen.insert(entry.name.as_str()) {
            return Err(invalid("Invalid or duplicate backup entry"));
        }
        total = total
            .checked_add(entry.size)
            .filter(|n| *n <= MAX_TOTAL)
            .ok_or_else(|| invalid("Backup exceeds the supported size"))?;
    }
    if total != manifest.summary.bytes
        || seen.len() != manifest.summary.files
        || !["peers.dat", "my_rooms.dat", "chat_history.db"]
            .iter()
            .all(|n| seen.contains(n))
    {
        return Err(invalid("Incomplete backup manifest"));
    }
    Ok(())
}

fn read_archive(
    path: &Path,
    password: &[u8],
    destination: Option<&Path>,
) -> Result<(Manifest, Identity)> {
    if password.len() > 4096 {
        return Err(invalid("Backup password is too long"));
    }
    let mut input = File::open(path)?;
    let mut header = [0u8; 40];
    input.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid("Unsupported backup format or version"));
    }
    let key = backup_key(password, &header)?;
    let mut index = 0;
    let manifest: Manifest =
        serde_json::from_slice(&read_frame(&mut input, &key[..], &header, &mut index)?)?;
    validate_manifest(&manifest)?;
    let seed = read_frame(&mut input, &key[..], &header, &mut index)?;
    let identity = Identity::from_seed(&seed)?;
    if identity.public_id() != manifest.summary.public_id {
        return Err(invalid("Backup identity does not match its manifest"));
    }
    // During export verification, retain just the optional trust database so
    // its actual archived bytes can be validated without duplicating attachments.
    let trust_copy = if destination.is_none()
        && manifest
            .entries
            .iter()
            .any(|entry| entry.name == DEVICE_TRUST_FILE)
    {
        Some(
            tempfile::Builder::new()
                .prefix(".verify-trust-")
                .tempdir_in(
                    path.parent()
                        .ok_or_else(|| invalid("Backup path has no parent"))?,
                )?,
        )
    } else {
        None
    };
    for entry in &manifest.entries {
        let extract_to = destination.or_else(|| {
            (entry.name == DEVICE_TRUST_FILE)
                .then(|| trust_copy.as_ref().map(TempDir::path))
                .flatten()
        });
        let mut output = if let Some(directory) = extract_to {
            let path = directory.join(&entry.name);
            if entry.name.starts_with("attachments/") {
                fs::create_dir_all(directory.join("attachments"))?;
            }
            Some(File::options().write(true).create_new(true).open(path)?)
        } else {
            None
        };
        let mut remaining = entry.size;
        while remaining > 0 {
            let frame = read_frame(&mut input, &key[..], &header, &mut index)?;
            if frame.len() as u64 != remaining.min(CHUNK as u64) {
                return Err(invalid("Backup file size mismatch"));
            }
            if let Some(output) = output.as_mut() {
                output.write_all(&frame)?;
            }
            remaining -= frame.len() as u64;
        }
        if let Some(output) = output {
            output.sync_all()?;
        }
    }
    if read_frame(&mut input, &key[..], &header, &mut index)?.as_slice() != END
        || input.read(&mut [0u8; 1])? != 0
    {
        return Err(invalid("Incomplete backup or unexpected trailing data"));
    }
    if let Some(copy) = trust_copy {
        DeviceTrustStore::read_existing(&identity, &copy.path().join(DEVICE_TRUST_FILE))?
            .validate_all()?;
    }
    Ok((manifest, identity))
}

fn read_small(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((16 * CHUNK + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * CHUNK {
        return Err(invalid("Profile metadata is too large"));
    }
    Ok(bytes)
}

/// Authenticated, validated import waiting for the user's preview confirmation.
/// Dropping it (including cancellation) removes its private staging directory.
pub struct PreparedRestore {
    directory: TempDir,
    identity: Identity,
    pub summary: BackupSummary,
}

impl PreparedRestore {
    pub fn inspect(path: &Path, password: &[u8], staging_parent: &Path) -> Result<Self> {
        fs::create_dir_all(staging_parent)?;
        let directory = tempfile::Builder::new()
            .prefix(".restore-")
            .tempdir_in(staging_parent)?;
        let (manifest, identity) = read_archive(path, password, Some(directory.path()))?;
        // PeerStore's ordinary loader tolerates damaged entries; restore must not.
        let peers = Zeroizing::new(decrypt_blob(
            &identity.derive_store_key(crate::peer_store::PEER_STORE_LABEL)?,
            &read_small(&directory.path().join("peers.dat"))?,
        )?);
        let peers: serde_json::Value = serde_json::from_slice(&peers)?;
        if peers["version"] != 1 {
            return Err(invalid("Unsupported peer store version"));
        }
        let entries = peers["peers"]
            .as_array()
            .ok_or_else(|| invalid("Missing peer records"))?;
        for entry in entries {
            let _: crate::peer_store::PeerRecord = serde_json::from_value(entry.clone())?;
        }
        let rooms = Zeroizing::new(decrypt_blob(
            &identity.derive_store_key(crate::room_store::ROOM_STORE_LABEL)?,
            &read_small(&directory.path().join("my_rooms.dat"))?,
        )?);
        let rooms: serde_json::Value = serde_json::from_slice(&rooms)?;
        if rooms.get("schema").is_some_and(|schema| schema != 1) {
            return Err(invalid("Unsupported room store version"));
        }
        let mut room_ids = HashSet::new();
        for room in rooms["rooms"]
            .as_array()
            .ok_or_else(|| invalid("Missing room definitions"))?
        {
            let room: crate::room_store::RoomEntry = serde_json::from_value(room.clone())?;
            if room.supernode_id.is_empty()
                || room.room_id.is_empty()
                || !room_ids.insert((room.supernode_id, room.room_id))
            {
                return Err(invalid("Invalid or duplicate room definition"));
            }
        }
        RoomStore::open(&identity, Some(&directory.path().join("my_rooms.dat")))?;
        let device_trust = directory.path().join(DEVICE_TRUST_FILE);
        if device_trust.try_exists()? {
            DeviceTrustStore::read_existing(&identity, &device_trust)?.validate_all()?;
        }
        for name in ["settings.json", "android-settings.json"] {
            let path = directory.path().join(name);
            if path.exists() {
                let value: serde_json::Value = serde_json::from_slice(&read_small(&path)?)?;
                if !value.is_object() {
                    return Err(invalid("Invalid settings document"));
                }
            }
        }
        let db = rusqlite::Connection::open(directory.path().join("chat_history.db"))?;
        db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")?;
        let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(invalid("Backup chat database failed its integrity check"));
        }
        let key = identity.derive_store_key(crate::chat_store::CHAT_STORE_LABEL)?;
        let mut statement =
            db.prepare("SELECT body, sender_handle, attachment_path FROM messages")?;
        let mut rows = statement.query([])?;
        let mut messages = 0;
        while let Some(row) = rows.next()? {
            let body: Vec<u8> = row.get(0)?;
            let _ = Zeroizing::new(decrypt_blob(&key, &body)?);
            // Legacy empty handles are stored as an empty blob.
            let handle: Vec<u8> = row.get(1)?;
            if !handle.is_empty() {
                let _ = Zeroizing::new(decrypt_blob(&key, &handle)?);
            }
            let attachment: String = row.get(2)?;
            if !attachment.is_empty()
                && !manifest
                    .entries
                    .iter()
                    .any(|e| e.name.starts_with("attachments/") && e.name == attachment)
            {
                return Err(invalid("Backup contains an invalid attachment reference"));
            }
            messages += 1;
        }
        if messages != manifest.summary.messages {
            return Err(invalid("Backup message count mismatch"));
        }
        Ok(Self {
            directory,
            identity,
            summary: manifest.summary,
        })
    }

    /// Restore to a new directory only. The caller activates it after success.
    pub fn commit(self, destination: &Path, local_password: &[u8]) -> Result<BackupSummary> {
        check_password(local_password)?;
        if destination.exists() {
            return Err(invalid(
                "Restore destination already exists; choose a new profile",
            ));
        }
        let directory = self.directory.path();
        self.identity.save_encrypted(local_password, directory)?;
        let db = rusqlite::Connection::open(directory.join("chat_history.db"))?;
        db.execute_batch("PRAGMA trusted_schema=OFF;")?;
        let paths: Vec<String> = db
            .prepare("SELECT DISTINCT attachment_path FROM messages WHERE attachment_path != ''")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for path in paths {
            let local = destination.join(&path).to_string_lossy().into_owned();
            db.execute(
                "UPDATE messages SET attachment_path=?1 WHERE attachment_path=?2",
                [&local, &path],
            )?;
        }
        db.close().map_err(|(_, e)| e)?;
        sanitize_settings(directory)?;
        // Same-filesystem rename publishes the entire profile at once. The
        // destination is generated by the caller, never taken from the archive.
        fs::rename(directory, destination)?;
        Ok(self.summary)
    }
}

fn sanitize_settings(directory: &Path) -> Result<()> {
    let path = directory.join("settings.json");
    if !path.exists() {
        return Ok(());
    }
    let mut settings: serde_json::Value = serde_json::from_slice(&read_small(&path)?)?;
    let map = settings
        .as_object_mut()
        .ok_or_else(|| invalid("Invalid settings document"))?;
    // A restored profile must never automatically capture, load native plugins,
    // or reuse machine-specific paths from a different installation.
    map.retain(|key, _| {
        !key.contains("device")
            && !key.contains("path")
            && !key.contains("source")
            && !key.contains("plugin")
            && !key.contains("auto_respond")
            && !key.contains("ollama_tools")
            && !key.contains("ollama_voice")
            && !key.contains("ollama_stt")
            && !key.contains("ollama_file_sharing")
            && !key.contains("ollama_share_folder")
            && !key.contains("auto_start")
    });
    map.insert("onboarding_complete".into(), false.into());
    map.insert("video_enabled".into(), false.into());
    map.insert("video_overlays_json".into(), "[]".into());
    fs::write(path, serde_json::to_vec_pretty(&settings)?)?;
    Ok(())
}

#[cfg(test)]
mod tests;
