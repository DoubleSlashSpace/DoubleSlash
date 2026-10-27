//! The supernodes this profile set up and manages, encrypted on disk.
//!
//! Holds the profile's SSH login key, each server's pinned host key and
//! address, and the inventory the manager library works from. It never holds
//! a server password: those are used once during setup and dropped.
//!
//! The file is not part of portable backups (`backup.rs` copies named stores
//! only). A restored or second device sets up its own access to a server.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snm_transport::{generate_ed25519_key, parse_private_key, PrivateKey};
use tracing::warn;
use zeroize::Zeroizing;

use crate::crypto::{decrypt_blob, encrypt_blob};
use crate::error::{ClientError, Result};
use crate::identity::Identity;

pub const MANAGED_STORE_FILE: &str = "managed_supernodes.dat";
pub const MANAGED_STORE_LABEL: &str = "doubleslash-store/managed-supernodes/v1";

/// One logical supernode: a single server, or several clustered together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedNode {
    pub id: String,
    pub name: String,
    /// `nightly` until a stable release exists.
    pub channel: String,
    /// `server-timer` or `client` (`snm_supernode::UpdateMode`).
    pub update_mode: String,
    /// Set once a second server joins.
    #[serde(default)]
    pub cluster_id: Option<String>,
    /// The invite the client accepted for the first server.
    pub invite_url: String,
    pub created_at: u64,
    /// Every server, first one first, with an inventory host per entry.
    pub hosts: Vec<ManagedHost>,
    /// The manager's description of the deployment.
    pub inventory: snm_core::Inventory,
}

impl ManagedNode {
    pub fn host(&self, host_name: &str) -> Option<&ManagedHost> {
        self.hosts.iter().find(|h| h.host_name == host_name)
    }
}

/// How to reach one server and what it is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedHost {
    /// `[[host]].name` in the node's inventory.
    pub host_name: String,
    /// `user@host[:port]`.
    pub ssh_target: String,
    /// The host key the user confirmed, as one OpenSSH public-key line.
    pub host_key: String,
    /// `snm_supernode::SshAccessMode` as a string.
    pub access: String,
    pub public_host: String,
    pub platform: String,
    /// The node's public identity (`identity.json` `public_key`).
    pub node_identity: String,
    #[serde(default)]
    pub os: String,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreData {
    #[serde(default = "default_schema")]
    schema: u32,
    /// Unix seconds when the responsibility notice was accepted.
    #[serde(default)]
    notice_accepted_at: Option<u64>,
    /// OpenSSH private key, generated on first use.
    #[serde(default)]
    client_key: Option<String>,
    #[serde(default)]
    nodes: Vec<ManagedNode>,
}

fn default_schema() -> u32 {
    1
}

pub struct ManagedNodeStore {
    path: PathBuf,
    key: [u8; 32],
    data: StoreData,
}

impl ManagedNodeStore {
    pub fn open(identity: &Identity, directory: &Path) -> Result<Self> {
        let key = identity.derive_store_key(MANAGED_STORE_LABEL)?;
        Self::open_with_key(&key, &directory.join(MANAGED_STORE_FILE))
    }

    pub fn open_with_key(key: &[u8; 32], path: &Path) -> Result<Self> {
        let data = if path.exists() {
            let envelope = std::fs::read(path)?;
            let plaintext = Zeroizing::new(decrypt_blob(key, &envelope)?);
            let data: StoreData = serde_json::from_slice(&plaintext)
                .map_err(|e| ClientError::Store(format!("managed supernodes: {e}")))?;
            if data.schema != 1 {
                warn!(
                    "managed supernode store schema {} is newer than supported (1)",
                    data.schema
                );
            }
            data
        } else {
            StoreData {
                schema: 1,
                ..StoreData::default()
            }
        };
        Ok(Self {
            path: path.to_owned(),
            key: *key,
            data,
        })
    }

    fn save(&self) -> Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&self.data).map_err(|e| ClientError::Store(e.to_string()))?,
        );
        let envelope = encrypt_blob(&self.key, &plaintext)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = self.path.with_extension("dat.tmp");
        std::fs::write(&staging, envelope)?;
        std::fs::rename(&staging, &self.path)?;
        Ok(())
    }

    pub fn notice_accepted(&self) -> bool {
        self.data.notice_accepted_at.is_some()
    }

    pub fn accept_notice(&mut self, now: u64) -> Result<()> {
        self.data.notice_accepted_at.get_or_insert(now);
        self.save()
    }

    /// The profile's SSH login key, generated and saved on first use.
    pub fn client_key(&mut self, comment: &str) -> Result<PrivateKey> {
        if let Some(text) = &self.data.client_key {
            return parse_private_key(text, None)
                .map_err(|e| ClientError::Store(format!("stored SSH key: {e}")));
        }
        let generated = generate_ed25519_key(comment)
            .map_err(|e| ClientError::Store(format!("generate SSH key: {e}")))?;
        let key = parse_private_key(&generated.private_openssh, None)
            .map_err(|e| ClientError::Store(e.to_string()))?;
        self.data.client_key = Some(generated.private_openssh.to_string());
        self.save()?;
        Ok(key)
    }

    /// The stored key, without generating one.
    pub fn existing_client_key(&self) -> Option<PrivateKey> {
        parse_private_key(self.data.client_key.as_deref()?, None).ok()
    }

    pub fn nodes(&self) -> &[ManagedNode] {
        &self.data.nodes
    }

    pub fn node(&self, id: &str) -> Option<&ManagedNode> {
        self.data.nodes.iter().find(|n| n.id == id)
    }

    pub fn upsert(&mut self, node: ManagedNode) -> Result<()> {
        match self.data.nodes.iter_mut().find(|n| n.id == node.id) {
            Some(existing) => *existing = node,
            None => self.data.nodes.push(node),
        }
        self.save()
    }

    /// Forget a node locally. The server is not touched.
    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let before = self.data.nodes.len();
        self.data.nodes.retain(|n| n.id != id);
        let removed = self.data.nodes.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ds-managed-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(MANAGED_STORE_FILE)
    }

    fn node(id: &str) -> ManagedNode {
        ManagedNode {
            id: id.into(),
            name: "My node".into(),
            channel: "nightly".into(),
            update_mode: "server-timer".into(),
            cluster_id: None,
            invite_url: "https://doubleslash.space/i#x".into(),
            created_at: 1,
            hosts: vec![ManagedHost {
                host_name: "my-node".into(),
                ssh_target: "root@198.51.100.7".into(),
                host_key: "ssh-ed25519 AAAA".into(),
                access: "client-key-only".into(),
                public_host: "198.51.100.7".into(),
                platform: "linux-x86_64".into(),
                node_identity: "abc".into(),
                os: "Debian GNU/Linux 12 (bookworm)".into(),
            }],
            inventory: snm_core::Inventory {
                defaults: snm_core::Defaults::default(),
                host: Vec::new(),
                clusters: Vec::new(),
            },
        }
    }

    #[test]
    fn round_trips_encrypted_and_keeps_the_client_key() {
        let path = temp_path("roundtrip");
        let key = [7u8; 32];
        let public = {
            let mut store = ManagedNodeStore::open_with_key(&key, &path).unwrap();
            assert!(!store.notice_accepted());
            store.accept_notice(42).unwrap();
            store.upsert(node("n1")).unwrap();
            let k = store.client_key("test").unwrap();
            k.public_key().to_openssh().unwrap()
        };
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("198.51.100.7"));
        assert!(!String::from_utf8_lossy(&raw).contains("PRIVATE KEY"));

        let mut store = ManagedNodeStore::open_with_key(&key, &path).unwrap();
        assert!(store.notice_accepted());
        assert_eq!(store.nodes().len(), 1);
        assert_eq!(
            store
                .client_key("ignored")
                .unwrap()
                .public_key()
                .to_openssh()
                .unwrap(),
            public
        );
        assert!(ManagedNodeStore::open_with_key(&[8u8; 32], &path).is_err());
    }

    #[test]
    fn upsert_replaces_and_remove_forgets() {
        let path = temp_path("upsert");
        let mut store = ManagedNodeStore::open_with_key(&[1u8; 32], &path).unwrap();
        store.upsert(node("n1")).unwrap();
        let mut renamed = node("n1");
        renamed.name = "Renamed".into();
        store.upsert(renamed).unwrap();
        assert_eq!(store.nodes().len(), 1);
        assert_eq!(store.node("n1").unwrap().name, "Renamed");
        assert!(store.remove("n1").unwrap());
        assert!(!store.remove("n1").unwrap());
        assert!(store.nodes().is_empty());
    }

    #[test]
    fn accepting_the_notice_twice_keeps_the_first_time() {
        let path = temp_path("notice");
        let mut store = ManagedNodeStore::open_with_key(&[2u8; 32], &path).unwrap();
        store.accept_notice(10).unwrap();
        store.accept_notice(20).unwrap();
        assert_eq!(store.data.notice_accepted_at, Some(10));
    }
}
