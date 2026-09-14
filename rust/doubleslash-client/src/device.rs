//! Identity-authorized device credentials. These are the local foundation for
//! device addressing; legacy sessions still use the identity key on the wire.
//!
//! A device key proves possession only. Authorization always requires a verified
//! registry pinned to the expected contact identity, including its latest version.

mod store;
pub use store::{DeviceTrustStore, DEVICE_TRUST_FILE};

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::{decrypt_blob, encrypt_blob};
use crate::error::{ClientError, Result};
use crate::identity::Identity;

const REGISTRY_DOMAIN: &[u8] = b"doubleslash/device-registry/v1\0";
const PROOF_DOMAIN: &[u8] = b"doubleslash/device-proof/v1\0";
const KEY_LABEL: &str = "conquerd-store/device-key/v1";
const KEY_FILE: &str = "device-key.dat";
const MAX_REGISTRY_BYTES: usize = 64 * 1024;
const MAX_DEVICES: usize = 64;

fn invalid(reason: &str) -> ClientError {
    ClientError::Identity(reason.into())
}

pub use doubleslash_features::DeviceId;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEntry {
    pub id: DeviceId,
    pub name: String,
    /// Revoked entries are permanent tombstones: an old key cannot be re-added.
    pub revoked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryBody {
    schema: u32,
    identity: [u8; 32],
    version: u64,
    devices: Vec<DeviceEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedRegistry {
    body: RegistryBody,
    signature: Vec<u8>,
}

/// Only verified documents inhabit this type. Deserialization must go through
/// `verify`, supplying an independently trusted identity and accepted predecessor.
pub struct DeviceRegistry {
    signed: SignedRegistry,
}

impl DeviceRegistry {
    pub fn create(identity: &Identity, first: DeviceEntry) -> Result<Self> {
        Self::sign(
            identity,
            RegistryBody {
                schema: 1,
                identity: identity.public_key_bytes(),
                version: 1,
                devices: vec![first],
            },
        )
    }

    pub fn version(&self) -> u64 {
        self.signed.body.version
    }

    pub fn entries(&self) -> &[DeviceEntry] {
        &self.signed.body.devices
    }

    pub fn authorizes(&self, device: DeviceId) -> bool {
        self.entries().iter().any(|d| d.id == device && !d.revoked)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.signed)?)
    }

    /// Author a successor locally. Remote updates use `verify` instead: neither
    /// a device key nor a relay may authorize devices or undo a revocation.
    pub fn update(&self, identity: &Identity, entry: DeviceEntry) -> Result<Self> {
        if identity.public_key_bytes() != self.signed.body.identity {
            return Err(invalid("Device registry belongs to another identity"));
        }
        let mut body = self.signed.body.clone();
        body.version = body
            .version
            .checked_add(1)
            .ok_or_else(|| invalid("Device registry version exhausted"))?;
        match body.devices.iter_mut().find(|d| d.id == entry.id) {
            Some(old) => {
                if old.revoked && !entry.revoked {
                    return Err(invalid("A revoked device key cannot be authorized again"));
                }
                *old = entry;
            }
            None => body.devices.push(entry),
        }
        Self::sign(identity, body)
    }

    pub fn verify(
        bytes: &[u8],
        expected_identity: &[u8; 32],
        previous: Option<&Self>,
    ) -> Result<Self> {
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(invalid("Device registry exceeds size limit"));
        }
        let signed: SignedRegistry = serde_json::from_slice(bytes)?;
        validate_body(&signed.body)?;
        if &signed.body.identity != expected_identity {
            return Err(invalid("Device registry identity mismatch"));
        }
        let key = VerifyingKey::from_bytes(expected_identity)
            .map_err(|_| invalid("Invalid identity key"))?;
        let signature = Signature::from_slice(&signed.signature)
            .map_err(|_| invalid("Invalid registry signature"))?;
        key.verify_strict(&registry_bytes(&signed.body)?, &signature)
            .map_err(|_| invalid("Device registry signature rejected"))?;
        if let Some(previous) = previous {
            if previous.signed.body.identity != *expected_identity {
                return Err(invalid("Previous registry belongs to another identity"));
            }
            if signed.body.version < previous.version() {
                return Err(invalid("Device registry rollback rejected"));
            }
            if signed.body.version == previous.version() && signed.body != previous.signed.body {
                return Err(invalid("Conflicting device registries at the same version"));
            }
            for old in previous.entries() {
                let next = signed.body.devices.iter().find(|d| d.id == old.id);
                if next.is_none() || (old.revoked && next.is_some_and(|d| !d.revoked)) {
                    return Err(invalid(
                        "Device registry removed a tombstone or reauthorized a revoked key",
                    ));
                }
            }
        }
        Ok(Self { signed })
    }

    fn sign(identity: &Identity, mut body: RegistryBody) -> Result<Self> {
        body.devices.sort_by_key(|entry| entry.id);
        validate_body(&body)?;
        let signature = identity.sign(&registry_bytes(&body)?);
        Ok(Self {
            signed: SignedRegistry { body, signature },
        })
    }

    /// Verify endpoint possession against a caller-supplied fresh challenge and
    /// handshake transcript hash. Freshness/one-use of the challenge belongs to
    /// the handshake state machine, not to the signature primitive.
    pub fn verify_proof(
        &self,
        device: DeviceId,
        challenge: &[u8; 32],
        transcript: &[u8; 32],
        signature: &[u8],
    ) -> bool {
        if !self.authorizes(device) {
            return false;
        }
        let Ok(key) = VerifyingKey::from_bytes(&device.0) else {
            return false;
        };
        let Ok(signature) = Signature::from_slice(signature) else {
            return false;
        };
        key.verify_strict(
            &proof_bytes(
                &self.signed.body.identity,
                self.version(),
                device,
                challenge,
                transcript,
            ),
            &signature,
        )
        .is_ok()
    }
}

fn registry_bytes(body: &RegistryBody) -> Result<Vec<u8>> {
    let mut bytes = REGISTRY_DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(body)?);
    Ok(bytes)
}

fn validate_body(body: &RegistryBody) -> Result<()> {
    if body.schema != 1
        || body.version == 0
        || body.devices.is_empty()
        || body.devices.len() > MAX_DEVICES
    {
        return Err(invalid("Invalid device registry version or device count"));
    }
    let mut previous = None;
    for device in &body.devices {
        let key =
            VerifyingKey::from_bytes(&device.id.0).map_err(|_| invalid("Invalid device key"))?;
        if key.is_weak()
            || device.id.0 == body.identity
            || previous.is_some_and(|id| id >= device.id)
        {
            return Err(invalid("Duplicate, unsorted, weak, or identity device key"));
        }
        if device.name.trim().is_empty()
            || device.name.len() > 128
            || device.name.chars().any(char::is_control)
        {
            return Err(invalid("Invalid device name"));
        }
        previous = Some(device.id);
    }
    Ok(())
}

/// Device-local secret. It never belongs in a portable identity backup: restoring
/// a profile must create a new endpoint instead of cloning this credential.
pub struct DeviceKey {
    signing: SigningKey,
}

impl DeviceKey {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn id(&self) -> DeviceId {
        DeviceId(self.signing.verifying_key().to_bytes())
    }

    pub fn entry(&self, name: &str) -> DeviceEntry {
        DeviceEntry {
            id: self.id(),
            name: name.into(),
            revoked: false,
        }
    }

    pub fn prove(
        &self,
        registry: &DeviceRegistry,
        challenge: &[u8; 32],
        transcript: &[u8; 32],
    ) -> Result<[u8; 64]> {
        if !registry.authorizes(self.id()) {
            return Err(invalid("Device is not authorized by this registry"));
        }
        Ok(self
            .signing
            .sign(&proof_bytes(
                &registry.signed.body.identity,
                registry.version(),
                self.id(),
                challenge,
                transcript,
            ))
            .to_bytes())
    }

    /// Owner-profile bootstrap. Private material is encrypted under a dedicated
    /// storage subkey. Atomic create ensures competing starts load the same key;
    /// corrupt existing material fails closed instead of silently replacing it.
    pub fn load_or_create(identity: &Identity, directory: &Path) -> Result<Self> {
        let path = directory.join(KEY_FILE);
        let storage_key = Zeroizing::new(identity.derive_store_key(KEY_LABEL)?);
        match std::fs::File::open(&path) {
            Ok(file) => return Self::read(file, &storage_key, identity),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        std::fs::create_dir_all(directory)?;
        let device = Self::generate();
        let mut plaintext = Zeroizing::new(Vec::with_capacity(64));
        plaintext.extend(identity.public_key_bytes());
        plaintext.extend(Zeroizing::new(device.signing.to_bytes()).iter());
        let envelope = encrypt_blob(storage_key.as_ref(), &plaintext)?;
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        staged.write_all(&envelope)?;
        staged.as_file().sync_all()?;
        match staged.persist_noclobber(&path) {
            Ok(_) => Ok(device),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::read(std::fs::File::open(&path)?, &storage_key, identity)
            }
            Err(error) => Err(error.error.into()),
        }
    }

    fn read(file: std::fs::File, storage_key: &[u8; 32], identity: &Identity) -> Result<Self> {
        let mut envelope = Vec::new();
        file.take(1025).read_to_end(&mut envelope)?;
        if envelope.len() > 1024 {
            return Err(invalid("Invalid device key file size"));
        }
        let plaintext = Zeroizing::new(decrypt_blob(storage_key, &envelope)?);
        if plaintext.len() != 64 || plaintext[..32] != identity.public_key_bytes() {
            return Err(invalid("Device key belongs to another identity"));
        }
        let mut seed = Zeroizing::new([0; 32]);
        seed.copy_from_slice(&plaintext[32..]);
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }
}

fn proof_bytes(
    identity: &[u8; 32],
    version: u64,
    device: DeviceId,
    challenge: &[u8; 32],
    transcript: &[u8; 32],
) -> Vec<u8> {
    let mut bytes = PROOF_DOMAIN.to_vec();
    bytes.extend(identity);
    bytes.extend(version.to_be_bytes());
    bytes.extend(device.0);
    bytes.extend(challenge);
    bytes.extend(transcript);
    bytes
}

/// Pins accepted registry versions per identity. Persist `snapshots()` alongside
/// trust before acting on an update; loading that snapshot retains revocations.
#[derive(Default)]
pub struct DeviceRegistryStore {
    registries: BTreeMap<[u8; 32], DeviceRegistry>,
}

impl DeviceRegistryStore {
    pub fn accept(&mut self, expected_identity: &[u8; 32], bytes: &[u8]) -> Result<bool> {
        let previous = self.registries.get(expected_identity);
        let next = DeviceRegistry::verify(bytes, expected_identity, previous)?;
        let changed = previous.is_none_or(|old| old.version() != next.version());
        self.registries.insert(*expected_identity, next);
        Ok(changed)
    }

    pub fn get(&self, identity: &[u8; 32]) -> Option<&DeviceRegistry> {
        self.registries.get(identity)
    }

    pub fn snapshots(&self) -> Result<Vec<([u8; 32], Vec<u8>)>> {
        self.registries
            .iter()
            .map(|(id, registry)| Ok((*id, registry.to_bytes()?)))
            .collect()
    }
}

/// Stable registry digest for later capability exchange, containing no secrets.
pub fn registry_digest(registry: &DeviceRegistry) -> Result<[u8; 32]> {
    Ok(Sha256::digest(registry_bytes(&registry.signed.body)?).into())
}

#[cfg(test)]
mod tests;
