//! Durable accepted device registries. SQLite serializes competing writers;
//! authenticated encryption hides registry contents and binds each lookup key.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::{invalid, DeviceId, DeviceRegistry, MAX_REGISTRY_BYTES};
use crate::crypto::{decrypt_blob, encrypt_blob};
use crate::error::Result;
use crate::identity::Identity;

const STORE_LABEL: &str = "doubleslash-store/device-trust/v1";
pub const DEVICE_TRUST_FILE: &str = "device-trust.db";
const STORE_MARKER: &[u8] = b"doubleslash-device-trust-v1";
const MAX_IDENTITIES: usize = 4096;
const MAX_ENVELOPE: usize = MAX_REGISTRY_BYTES + 64;

/// Accepted registries, encrypted with a profile-specific storage subkey.
///
/// Callers must independently trust `expected_identity` before accepting a
/// registry. A valid signature does not grant contact trust. Reads consult the
/// database rather than a stale in-memory cache; successful updates are committed
/// before returning. Restoring an older whole profile still requires catching up
/// with trusted devices: this is not hardware-backed protection against disk rollback.
pub struct DeviceTrustStore {
    connection: Connection,
    key: Zeroizing<[u8; 32]>,
}

impl DeviceTrustStore {
    pub fn open(identity: &Identity, path: &Path) -> Result<Self> {
        let key = Zeroizing::new(identity.derive_store_key(STORE_LABEL)?);
        Self::open_with_key(&key, path)
    }

    /// Backup reads must not create an empty store if the source disappeared,
    /// or initialize a malformed empty database supplied in an archive.
    pub(crate) fn read_existing(identity: &Identity, path: &Path) -> Result<Self> {
        let key = Zeroizing::new(identity.derive_store_key(STORE_LABEL)?);
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "trusted_schema", false)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 1 {
            return Err(invalid("Unsupported device trust database version"));
        }
        validate_marker(&connection, &key)?;
        Ok(Self { connection, key })
    }

    /// Opens without requiring identity signing authority. Use the device-trust
    /// subkey, never a chat/peer-store key. An existing unreadable store is an
    /// error and must not be replaced by an empty trust set.
    pub fn open_with_key(key: &[u8; 32], path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "trusted_schema", false)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                let tables: i64 = transaction.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                    [], |row| row.get(0),
                )?;
                if tables != 0 {
                    return Err(invalid("Unrecognized device trust database"));
                }
                transaction.execute_batch(
                    "CREATE TABLE metadata (id INTEGER PRIMARY KEY CHECK(id=1), envelope BLOB NOT NULL);
                     CREATE TABLE registries (lookup BLOB PRIMARY KEY CHECK(length(lookup)=32), envelope BLOB NOT NULL);
                     PRAGMA user_version=1;",
                )?;
                transaction.execute(
                    "INSERT INTO metadata VALUES(1, ?1)",
                    [encrypt_blob(key, STORE_MARKER)?],
                )?;
            }
            1 => {
                validate_marker(&transaction, key)?;
            }
            _ => return Err(invalid("Unsupported device trust database version")),
        }
        transaction.commit()?;
        Ok(Self {
            connection,
            key: Zeroizing::new(*key),
        })
    }

    /// Commit an authenticated successor after checking the latest persisted
    /// version under a write transaction. No caller-visible state advances when
    /// signature validation or persistence fails.
    pub fn accept(&mut self, expected_identity: &[u8; 32], bytes: &[u8]) -> Result<bool> {
        // Bound and verify untrusted input before acquiring the database writer.
        let candidate = DeviceRegistry::verify(bytes, expected_identity, None)?;
        let lookup = self.lookup(expected_identity)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = read_registry(&transaction, &self.key, &lookup, expected_identity)?;
        let next = DeviceRegistry::verify(bytes, expected_identity, previous.as_ref())?;
        if previous
            .as_ref()
            .is_some_and(|old| old.version() == next.version())
        {
            return Ok(false);
        }
        if previous.is_none() {
            let count: usize =
                transaction.query_row("SELECT count(*) FROM registries", [], |row| row.get(0))?;
            if count >= MAX_IDENTITIES {
                return Err(invalid("Device trust database identity limit reached"));
            }
        }
        let envelope = encrypt_blob(self.key.as_ref(), &candidate.to_bytes()?)?;
        transaction.execute(
            "INSERT INTO registries(lookup, envelope) VALUES(?1, ?2)
             ON CONFLICT(lookup) DO UPDATE SET envelope=excluded.envelope",
            params![lookup.as_slice(), envelope],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn get(&self, expected_identity: &[u8; 32]) -> Result<Option<DeviceRegistry>> {
        read_registry(
            &self.connection,
            &self.key,
            &self.lookup(expected_identity)?,
            expected_identity,
        )
    }

    /// Capture committed rows with SQLite's online backup API, including a
    /// source database using WAL. The destination is then fully authenticated.
    pub fn backup_snapshot(&self, destination: &Path) -> Result<()> {
        self.connection
            .backup(rusqlite::DatabaseName::Main, destination, None)?;
        Self::open_with_key(&self.key, destination)?.validate_all()
    }

    /// Validate every stored signature and encrypted identity binding before a
    /// staged backup is offered for restore. This does not grant contact trust.
    pub fn validate_all(&self) -> Result<()> {
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(invalid("Device trust database failed its integrity check"));
        }
        let mut statement = self.connection.prepare(
            "SELECT CASE WHEN length(lookup)=32 THEN lookup END,
             CASE WHEN length(envelope)<=?1 THEN envelope END FROM registries",
        )?;
        let mut rows = statement.query([MAX_ENVELOPE])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            count += 1;
            if count > MAX_IDENTITIES {
                return Err(invalid("Device trust database identity limit reached"));
            }
            let lookup: Option<Vec<u8>> = row.get(0)?;
            let lookup = lookup.ok_or_else(|| invalid("Invalid device trust lookup size"))?;
            let envelope: Option<Vec<u8>> = row.get(1)?;
            let envelope =
                envelope.ok_or_else(|| invalid("Device trust registry exceeds size limit"))?;
            let plaintext = Zeroizing::new(decrypt_blob(self.key.as_ref(), &envelope)?);
            let signed: super::SignedRegistry = serde_json::from_slice(&plaintext)?;
            let identity = signed.body.identity;
            DeviceRegistry::verify(&plaintext, &identity, None)?;
            if lookup != self.lookup(&identity)? {
                return Err(invalid("Device trust registry identity binding rejected"));
            }
        }
        Ok(())
    }

    /// Verify possession against the latest committed authorization, including
    /// revocations written by another process since this handle was opened.
    pub fn verify_proof(
        &self,
        expected_identity: &[u8; 32],
        device: DeviceId,
        challenge: &[u8; 32],
        transcript: &[u8; 32],
        signature: &[u8],
    ) -> Result<bool> {
        Ok(self.get(expected_identity)?.is_some_and(|registry| {
            registry.verify_proof(device, challenge, transcript, signature)
        }))
    }

    fn lookup(&self, identity: &[u8; 32]) -> Result<[u8; 32]> {
        let mut lookup = [0; 32];
        hkdf::Hkdf::<Sha256>::new(Some(self.key.as_ref()), identity)
            .expand(b"device-trust-lookup/v1", &mut lookup)
            .map_err(|_| invalid("Unable to derive device trust lookup"))?;
        Ok(lookup)
    }
}

fn validate_marker(connection: &Connection, key: &[u8; 32]) -> Result<()> {
    let marker: Vec<u8> = connection.query_row(
        "SELECT envelope FROM metadata WHERE id=1 AND length(envelope)<=128",
        [],
        |row| row.get(0),
    )?;
    if decrypt_blob(key, &marker)? != STORE_MARKER {
        return Err(invalid("Device trust database belongs to another profile"));
    }
    Ok(())
}

fn read_registry(
    connection: &Connection,
    key: &[u8; 32],
    lookup: &[u8; 32],
    identity: &[u8; 32],
) -> Result<Option<DeviceRegistry>> {
    // Read size before the blob so a damaged/malicious database cannot force
    // allocation of an arbitrarily large envelope. Never treat an oversized
    // existing row as an absent predecessor.
    let stored: Option<(usize, Option<Vec<u8>>)> = connection
        .query_row(
            "SELECT length(envelope), CASE WHEN length(envelope)<=?2 THEN envelope END
         FROM registries WHERE lookup=?1",
            params![lookup.as_slice(), MAX_ENVELOPE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((size, envelope)) = stored else {
        return Ok(None);
    };
    if size > MAX_ENVELOPE {
        return Err(invalid("Device trust registry exceeds size limit"));
    }
    let envelope = envelope.ok_or_else(|| invalid("Missing device trust envelope"))?;
    let plaintext = Zeroizing::new(decrypt_blob(key, &envelope)?);
    Ok(Some(DeviceRegistry::verify(&plaintext, identity, None)?))
}

#[cfg(test)]
mod tests;
