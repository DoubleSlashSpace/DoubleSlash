// DoubleSlash supernode — access.rs
// Access control: trait + built-in implementations (open, TOS, ad, code).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;

use crate::config::AccessMode;

/// Persisted set of peers that have passed the access gate.
///
/// Grants used to live in a bare in-memory `HashSet`, so every supernode
/// restart silently revoked everyone who had accepted the gate and a room
/// guest had to accept again before their relay traffic (notably
/// `room.audio.sfu`) was admitted. Writes go through to disk immediately —
/// a grant is a durable statement about a person, not session state.
pub struct GrantStore {
    granted: parking_lot::RwLock<HashSet<String>>,
    /// `None` in tests and for controllers built without a data directory,
    /// which keeps the store in-memory exactly as before.
    path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GrantsFile {
    version: u32,
    updated_at: f64,
    granted: Vec<String>,
}

impl GrantStore {
    /// In-memory only; nothing is written or read.
    pub fn ephemeral() -> Self {
        Self {
            granted: parking_lot::RwLock::new(HashSet::new()),
            path: None,
        }
    }

    /// Backed by `path`, loading any previously persisted grants.
    pub fn new(path: &Path) -> Self {
        let store = Self {
            granted: parking_lot::RwLock::new(HashSet::new()),
            path: Some(path.to_path_buf()),
        };
        store.load();
        store
    }

    fn load(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if !path.exists() {
            return;
        }
        // A missing or corrupt file must not take the node down; it degrades to
        // "nobody is granted yet", which is the same state a fresh node is in.
        let Ok(data) = std::fs::read_to_string(path) else {
            warn!(
                "[access] could not read {}; starting with no grants",
                path.display()
            );
            return;
        };
        let Ok(file) = serde_json::from_str::<GrantsFile>(&data) else {
            warn!(
                "[access] could not parse {}; starting with no grants",
                path.display()
            );
            return;
        };
        let mut granted = self.granted.write();
        for peer in file.granted {
            granted.insert(peer);
        }
    }

    fn save(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let file = GrantsFile {
            version: 1,
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            granted: self.granted.read().iter().cloned().collect(),
        };
        let write = serde_json::to_string_pretty(&file)
            .map_err(std::io::Error::other)
            .and_then(|json| {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, json)
            });
        if let Err(error) = write {
            // Losing the write means this grant reverts on restart, which is
            // the old behaviour — worth a warning, not a failed grant.
            warn!(
                "[access] could not persist grants to {}: {error}",
                path.display()
            );
        }
    }

    pub fn contains(&self, peer_id: &str) -> bool {
        self.granted.read().contains(peer_id)
    }

    /// Record a grant and persist it. Idempotent: re-granting an already
    /// granted peer does not rewrite the file.
    pub fn insert(&self, peer_id: &str) {
        if !self.granted.write().insert(peer_id.to_string()) {
            return;
        }
        self.save();
    }
}

/// Access controller determines whether a peer gets relay access immediately
/// or must visit the web portal first.
///
/// **Open mode semantics** (enforced in `SupernodeState::check_peer_access`):
/// peers who completed a **direct supernode invite** (handshake transcript)
/// are granted immediately. Everyone else (room-invite guests, etc.) must
/// accept the presented TOS via the access portal before full relay access.
pub trait AccessController: Send + Sync {
    /// Return true → grant relay immediately. False → redirect to portal.
    fn check_access(&self, peer_id: &str) -> bool;

    /// Called after access is granted via portal.
    fn on_peer_granted(&self, _peer_id: &str) {}

    /// The portal entry path for this access mode.
    fn portal_entry_path(&self) -> &str {
        "/access.html"
    }

    /// Display name for stats.
    fn mode_name(&self) -> &str;
}

/// Open mode: the controller only tracks **guest** TOS accepts. Direct-invite
/// peers bypass this controller in `SupernodeState::check_peer_access`.
pub struct OpenAccessController {
    /// Room-invite / non-handshake peers who accepted the open-mode TOS.
    guest_accepted: GrantStore,
}

impl OpenAccessController {
    /// In-memory only (tests).
    pub fn new() -> Self {
        Self {
            guest_accepted: GrantStore::ephemeral(),
        }
    }

    /// Persist grants to `path` and load any already recorded there.
    pub fn with_store(path: &Path) -> Self {
        Self {
            guest_accepted: GrantStore::new(path),
        }
    }
}

impl Default for OpenAccessController {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessController for OpenAccessController {
    fn check_access(&self, peer_id: &str) -> bool {
        self.guest_accepted.contains(peer_id)
    }

    fn on_peer_granted(&self, peer_id: &str) {
        self.guest_accepted.insert(peer_id);
    }

    fn portal_entry_path(&self) -> &str {
        "/access.html"
    }

    fn mode_name(&self) -> &str {
        "open"
    }
}

/// Requires TOS acceptance via web portal (all peers, including direct invite).
pub struct TOSAccessController {
    accepted: GrantStore,
}

impl TOSAccessController {
    /// In-memory only (tests); production goes through [`Self::with_store`].
    #[cfg_attr(not(test), expect(dead_code, reason = "exercised by unit tests only"))]
    pub fn new() -> Self {
        Self {
            accepted: GrantStore::ephemeral(),
        }
    }

    /// Persist grants to `path` and load any already recorded there.
    pub fn with_store(path: &Path) -> Self {
        Self {
            accepted: GrantStore::new(path),
        }
    }
}

impl AccessController for TOSAccessController {
    fn check_access(&self, peer_id: &str) -> bool {
        self.accepted.contains(peer_id)
    }

    fn on_peer_granted(&self, peer_id: &str) {
        self.accepted.insert(peer_id);
    }

    fn portal_entry_path(&self) -> &str {
        "/access.html"
    }

    fn mode_name(&self) -> &str {
        "tos"
    }
}

/// Requires watching an ad/timer via web portal.
pub struct AdGateAccessController {
    granted: GrantStore,
}

impl AdGateAccessController {
    /// In-memory only (tests); production goes through [`Self::with_store`].
    #[cfg_attr(not(test), expect(dead_code, reason = "exercised by unit tests only"))]
    pub fn new() -> Self {
        Self {
            granted: GrantStore::ephemeral(),
        }
    }

    /// Persist grants to `path` and load any already recorded there.
    pub fn with_store(path: &Path) -> Self {
        Self {
            granted: GrantStore::new(path),
        }
    }
}

impl AccessController for AdGateAccessController {
    fn check_access(&self, peer_id: &str) -> bool {
        self.granted.contains(peer_id)
    }

    fn on_peer_granted(&self, peer_id: &str) {
        self.granted.insert(peer_id);
    }

    fn portal_entry_path(&self) -> &str {
        "/access.html"
    }

    fn mode_name(&self) -> &str {
        "ad"
    }
}

/// Requires entering an access code via web portal.
pub struct CodeGateAccessController {
    #[cfg_attr(not(test), expect(dead_code, reason = "exercised by unit tests only"))]
    code: String,
    granted: GrantStore,
}

impl CodeGateAccessController {
    /// In-memory only (tests); production goes through [`Self::with_store`].
    #[cfg_attr(not(test), expect(dead_code, reason = "exercised by unit tests only"))]
    pub fn new(code: String) -> Self {
        Self {
            code,
            granted: GrantStore::ephemeral(),
        }
    }

    /// Persist grants to `path` and load any already recorded there.
    pub fn with_store(code: String, path: &Path) -> Self {
        Self {
            code,
            granted: GrantStore::new(path),
        }
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "exercised by unit tests only"))]
    pub fn check_code(&self, submitted: &str) -> bool {
        submitted == self.code
    }
}

impl AccessController for CodeGateAccessController {
    fn check_access(&self, peer_id: &str) -> bool {
        self.granted.contains(peer_id)
    }

    fn on_peer_granted(&self, peer_id: &str) {
        self.granted.insert(peer_id);
    }

    fn portal_entry_path(&self) -> &str {
        "/access.html"
    }

    fn mode_name(&self) -> &str {
        "code"
    }
}

/// Create the appropriate access controller from config, persisting grants
/// under `data_dir` so they survive a restart.
///
/// Each mode keeps its own file: switching modes must not silently carry a
/// grant earned under different terms.
pub fn create_access_controller(
    mode: AccessMode,
    code: &str,
    data_dir: &Path,
) -> Box<dyn AccessController> {
    let path = |name: &str| data_dir.join(name);
    match mode {
        AccessMode::Open => Box::new(OpenAccessController::with_store(&path(
            "access_grants_open.json",
        ))),
        AccessMode::Tos => Box::new(TOSAccessController::with_store(&path(
            "access_grants_tos.json",
        ))),
        AccessMode::Ad => Box::new(AdGateAccessController::with_store(&path(
            "access_grants_ad.json",
        ))),
        AccessMode::Code => Box::new(CodeGateAccessController::with_store(
            code.to_string(),
            &path("access_grants_code.json"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── OpenAccessController ────────────────────────────────────────────────

    #[test]
    fn open_denies_guests_until_tos_accepted() {
        let ac = OpenAccessController::new();
        assert!(!ac.check_access("peer-abc"));
        assert!(!ac.check_access(""));
    }

    #[test]
    fn open_grants_guest_after_on_peer_granted() {
        let ac = OpenAccessController::new();
        ac.on_peer_granted("guest-1");
        assert!(ac.check_access("guest-1"));
        assert!(!ac.check_access("guest-2"));
    }

    #[test]
    fn open_mode_name() {
        assert_eq!(OpenAccessController::new().mode_name(), "open");
    }

    #[test]
    fn open_portal_entry_path_is_access_html() {
        assert_eq!(
            OpenAccessController::new().portal_entry_path(),
            "/access.html"
        );
    }

    // ── TOSAccessController ─────────────────────────────────────────────────

    #[test]
    fn tos_denies_before_grant() {
        let ac = TOSAccessController::new();
        assert!(!ac.check_access("peer-1"));
    }

    #[test]
    fn tos_grants_after_on_peer_granted() {
        let ac = TOSAccessController::new();
        ac.on_peer_granted("peer-1");
        assert!(ac.check_access("peer-1"));
        assert!(!ac.check_access("peer-2"));
    }

    #[test]
    fn tos_portal_path_and_mode_name() {
        let ac = TOSAccessController::new();
        assert_eq!(ac.portal_entry_path(), "/access.html");
        assert_eq!(ac.mode_name(), "tos");
    }

    // ── AdGateAccessController ──────────────────────────────────────────────

    #[test]
    fn ad_gate_denies_before_grant() {
        let ac = AdGateAccessController::new();
        assert!(!ac.check_access("peer-x"));
    }

    #[test]
    fn ad_gate_grants_after_on_peer_granted() {
        let ac = AdGateAccessController::new();
        ac.on_peer_granted("peer-x");
        assert!(ac.check_access("peer-x"));
        assert!(!ac.check_access("peer-y"));
    }

    #[test]
    fn ad_gate_portal_path_and_mode_name() {
        let ac = AdGateAccessController::new();
        assert_eq!(ac.portal_entry_path(), "/access.html");
        assert_eq!(ac.mode_name(), "ad");
    }

    // ── CodeGateAccessController ────────────────────────────────────────────

    #[test]
    fn code_gate_denies_before_grant() {
        let ac = CodeGateAccessController::new("secret".into());
        assert!(!ac.check_access("peer-z"));
    }

    #[test]
    fn code_gate_check_code_correct() {
        let ac = CodeGateAccessController::new("secret".into());
        assert!(ac.check_code("secret"));
        assert!(!ac.check_code("wrong"));
        assert!(!ac.check_code(""));
    }

    #[test]
    fn code_gate_grants_after_on_peer_granted() {
        let ac = CodeGateAccessController::new("secret".into());
        ac.on_peer_granted("peer-z");
        assert!(ac.check_access("peer-z"));
        assert!(!ac.check_access("peer-w"));
    }

    #[test]
    fn code_gate_portal_path_and_mode_name() {
        let ac = CodeGateAccessController::new("x".into());
        assert_eq!(ac.portal_entry_path(), "/access.html");
        assert_eq!(ac.mode_name(), "code");
    }

    // ── factory ─────────────────────────────────────────────────────────────

    /// Unique empty directory per call: these controllers now persist, so a
    /// grant from a previous run must not leak into the next assertion.
    fn fresh_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ds-access-{tag}-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn grants_survive_a_restart() {
        // The bug this guards: grants lived in memory only, so every restart
        // silently revoked everyone who had accepted the gate and a room guest
        // was refused relay (and therefore room audio) until they re-accepted.
        let dir = fresh_dir("restart");
        let path = dir.join("access_grants_open.json");
        {
            let ac = OpenAccessController::with_store(&path);
            assert!(!ac.check_access("guest-1"));
            ac.on_peer_granted("guest-1");
            assert!(ac.check_access("guest-1"));
        }
        // Same path, new process.
        let reloaded = OpenAccessController::with_store(&path);
        assert!(reloaded.check_access("guest-1"));
        assert!(!reloaded.check_access("never-granted"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ephemeral_store_persists_nothing() {
        let ac = OpenAccessController::new();
        ac.on_peer_granted("guest-1");
        assert!(ac.check_access("guest-1"));
    }

    #[test]
    fn unreadable_grant_file_degrades_to_no_grants() {
        // A corrupt file must not take the node down; it reads as "nobody is
        // granted yet", which is just a fresh node.
        let dir = fresh_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("access_grants_open.json");
        std::fs::write(&path, "{ not json").unwrap();
        let ac = OpenAccessController::with_store(&path);
        assert!(!ac.check_access("guest-1"));
        // And it recovers: a new grant overwrites the bad file.
        ac.on_peer_granted("guest-1");
        assert!(OpenAccessController::with_store(&path).check_access("guest-1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_mode_keeps_its_own_grant_file() {
        // Switching gate mode must not carry over a grant earned under
        // different terms.
        let dir = fresh_dir("modes");
        let open = create_access_controller(AccessMode::Open, "", &dir);
        open.on_peer_granted("guest-1");
        assert!(open.check_access("guest-1"));
        let tos = create_access_controller(AccessMode::Tos, "", &dir);
        assert!(!tos.check_access("guest-1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn factory_open_denies_until_guest_tos() {
        let dir = fresh_dir("open");
        let ac = create_access_controller(AccessMode::Open, "irrelevant", &dir);
        assert!(!ac.check_access("anyone"));
        assert_eq!(ac.mode_name(), "open");
        ac.on_peer_granted("anyone");
        assert!(ac.check_access("anyone"));
    }

    #[test]
    fn factory_tos_denies_initially() {
        let dir = fresh_dir("tos");
        let ac = create_access_controller(AccessMode::Tos, "irrelevant", &dir);
        assert!(!ac.check_access("anyone"));
        assert_eq!(ac.mode_name(), "tos");
    }

    #[test]
    fn factory_ad_denies_initially() {
        let dir = fresh_dir("ad");
        let ac = create_access_controller(AccessMode::Ad, "irrelevant", &dir);
        assert!(!ac.check_access("anyone"));
        assert_eq!(ac.mode_name(), "ad");
    }

    #[test]
    fn factory_code_denies_initially() {
        let dir = fresh_dir("code");
        let ac = create_access_controller(AccessMode::Code, "mycode", &dir);
        assert!(!ac.check_access("anyone"));
        assert_eq!(ac.mode_name(), "code");
    }

    #[test]
    fn concurrent_grant_and_check_tos() {
        use std::sync::Arc;
        let ac = Arc::new(TOSAccessController::new());
        let ac2 = ac.clone();
        let handle = std::thread::spawn(move || {
            ac2.on_peer_granted("peer-concurrent");
        });
        handle.join().unwrap();
        assert!(ac.check_access("peer-concurrent"));
    }
}
