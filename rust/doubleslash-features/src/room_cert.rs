//! Owner-signed room certificates.
//!
//! A supernode keeps rooms in memory only, so a room disappears when the node
//! restarts or the room sits idle. The certificate is what brings it back: the
//! room's owner signs its description once, every member keeps a copy, and any
//! member can present it to any node. The node recreates the room from the
//! certificate, never from what the request claims, so a member can restore a
//! room but cannot change its type, policy or owner.
//!
//! A private room's certificate also commits to the room's join secret (the
//! invite token members share) by hash. A node admits whoever presents a
//! secret matching that hash, on any node and after any restart, without
//! keeping token state of its own. A string the owner never committed to
//! admits no one.
//!
//! # Canonical signing payload
//!
//! ```text
//! "doubleslash-room-cert-v1|" ‖ serde_json::to_vec(BTreeMap {
//!   "invite_policy", "issued_at", "owner_pub", "room_id",
//!   "room_name", "room_type", "schema", "secret_hash",
//! })
//! ```
//!
//! Only strings and unsigned integers are signed, so the bytes do not depend
//! on how a workspace configures `serde_json`.

use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Domain-separation label for the certificate signature.
pub const ROOM_CERT_LABEL: &str = "doubleslash-room-cert-v1";
/// Domain-separation label for a room's join-secret commitment.
pub const ROOM_SECRET_LABEL: &str = "doubleslash-room-secret-v1";
/// Schema version carried by [`RoomCertificate::schema`].
pub const ROOM_CERT_SCHEMA: u32 = 1;
/// Longest room name a certificate may carry, in bytes.
pub const MAX_ROOM_NAME_BYTES: usize = 256;

/// Why a certificate failed [`RoomCertificate::verify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomCertError {
    /// A schema this build does not understand.
    Schema,
    /// `room_type` or `invite_policy` outside their vocabulary, or a field
    /// over its length limit.
    Field,
    /// `room_id` is not the id the owner's key and the room name derive.
    RoomId,
    /// A public room committing to a secret, or a malformed hash.
    SecretHash,
    /// The signature does not verify under `owner_pub`.
    Signature,
}

impl std::fmt::Display for RoomCertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            Self::Schema => "unsupported room certificate schema",
            Self::Field => "room certificate field out of range",
            Self::RoomId => "room id does not match the owner and name",
            Self::SecretHash => "room certificate secret commitment is malformed",
            Self::Signature => "room certificate signature does not verify",
        };
        f.write_str(what)
    }
}

impl std::error::Error for RoomCertError {}

/// An owner-signed description of one room. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomCertificate {
    pub schema: u32,
    /// [`derive_room_id`] of `owner_pub` and `room_name`.
    pub room_id: String,
    pub room_name: String,
    /// `"public"` | `"private"`.
    pub room_type: String,
    /// `"owner"` | `"members"`: who may mint node invite tokens.
    pub invite_policy: String,
    /// The owner's Ed25519 public id, exactly as the owner signs messages.
    pub owner_pub: String,
    /// [`room_secret_hash`] of the room's join secret; `""` when none.
    #[serde(default)]
    pub secret_hash: String,
    /// Unix seconds. A later certificate from the same owner replaces an
    /// earlier one, which is how a secret is rotated.
    pub issued_at: u64,
    /// base64url Ed25519 signature over [`RoomCertificate::signing_bytes`].
    #[serde(default)]
    pub signature: String,
}

impl RoomCertificate {
    /// An unsigned certificate for a room `owner_pub` creates. A private room
    /// commits to `secret` when it is non-empty; a public room never does.
    pub fn new(
        owner_pub: &str,
        room_name: &str,
        room_type: &str,
        invite_policy: &str,
        secret: &str,
        issued_at: u64,
    ) -> Self {
        let room_id = derive_room_id(owner_pub, room_name);
        let room_type = if room_type.eq_ignore_ascii_case("private") {
            "private"
        } else {
            "public"
        };
        let secret_hash = if room_type == "private" && !secret.is_empty() {
            room_secret_hash(&room_id, secret)
        } else {
            String::new()
        };
        Self {
            schema: ROOM_CERT_SCHEMA,
            room_id,
            room_name: room_name.to_owned(),
            room_type: room_type.to_owned(),
            invite_policy: if invite_policy == "members" {
                "members".to_owned()
            } else {
                "owner".to_owned()
            },
            owner_pub: owner_pub.to_owned(),
            secret_hash,
            issued_at,
            signature: String::new(),
        }
    }

    /// The bytes the signature covers.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut m: BTreeMap<&str, Value> = BTreeMap::new();
        m.insert("schema", json!(self.schema));
        m.insert("room_id", json!(self.room_id));
        m.insert("room_name", json!(self.room_name));
        m.insert("room_type", json!(self.room_type));
        m.insert("invite_policy", json!(self.invite_policy));
        m.insert("owner_pub", json!(self.owner_pub));
        m.insert("secret_hash", json!(self.secret_hash));
        m.insert("issued_at", json!(self.issued_at));
        let body = serde_json::to_vec(&m).unwrap_or_default();
        let mut out = Vec::with_capacity(ROOM_CERT_LABEL.len() + 1 + body.len());
        out.extend_from_slice(ROOM_CERT_LABEL.as_bytes());
        out.push(b'|');
        out.extend_from_slice(&body);
        out
    }

    /// Sign with the owner's key. `sign` returns the 64-byte signature.
    pub fn signed(mut self, sign: impl FnOnce(&[u8]) -> Vec<u8>) -> Self {
        self.signature = URL_SAFE_NO_PAD.encode(sign(&self.signing_bytes()));
        self
    }

    /// Check the schema, the field vocabulary, the room id's binding to the
    /// owner and name, the secret commitment and the owner's signature.
    pub fn verify(&self) -> Result<(), RoomCertError> {
        if self.schema != ROOM_CERT_SCHEMA {
            return Err(RoomCertError::Schema);
        }
        if !matches!(self.room_type.as_str(), "public" | "private")
            || !matches!(self.invite_policy.as_str(), "owner" | "members")
            || self.room_name.is_empty()
            || self.room_name.len() > MAX_ROOM_NAME_BYTES
        {
            return Err(RoomCertError::Field);
        }
        if !room_id_matches(&self.room_id, &self.owner_pub, &self.room_name) {
            return Err(RoomCertError::RoomId);
        }
        if !self.secret_hash.is_empty()
            && (self.room_type != "private" || !is_sha256_hex(&self.secret_hash))
        {
            return Err(RoomCertError::SecretHash);
        }
        let key = decode_b64url(&self.owner_pub)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .and_then(|b| VerifyingKey::from_bytes(&b).ok())
            .ok_or(RoomCertError::Signature)?;
        let sig = decode_b64url(&self.signature)
            .and_then(|b| <[u8; 64]>::try_from(b).ok())
            .map(|b| Signature::from_bytes(&b))
            .ok_or(RoomCertError::Signature)?;
        key.verify(&self.signing_bytes(), &sig)
            .map_err(|_| RoomCertError::Signature)
    }

    pub fn is_private(&self) -> bool {
        self.room_type == "private"
    }

    /// Whether `peer` is this room's owner, however either spells the padding.
    pub fn is_owner(&self, peer: &str) -> bool {
        !peer.is_empty() && peer.trim_end_matches('=') == self.owner_pub.trim_end_matches('=')
    }

    /// Whether `secret` is the join secret this certificate commits to.
    pub fn admits_secret(&self, secret: &str) -> bool {
        if secret.is_empty() || self.secret_hash.is_empty() {
            return false;
        }
        let presented = room_secret_hash(&self.room_id, secret);
        // Equal-length hex; compare without an early exit.
        presented.len() == self.secret_hash.len()
            && presented
                .bytes()
                .zip(self.secret_hash.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }

    /// Whether `self` should replace `held` for the same room: the same owner,
    /// issued later.
    pub fn supersedes(&self, held: &RoomCertificate) -> bool {
        self.room_id == held.room_id
            && held.is_owner(&self.owner_pub)
            && self.issued_at > held.issued_at
    }

    /// Parse a certificate from a JSON value and verify it.
    pub fn from_value_verified(value: &Value) -> Option<Self> {
        let cert: Self = serde_json::from_value(value.clone()).ok()?;
        cert.verify().ok().map(|()| cert)
    }

    /// Parse a certificate from JSON text and verify it.
    pub fn from_json_verified(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let value: Value = serde_json::from_str(text).ok()?;
        Self::from_value_verified(&value)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// A room's id: the first 16 hex characters of
/// `SHA-256("{creator_pub}:{room_name}")`. Supernodes derive the same id for a
/// room created without one.
pub fn derive_room_id(creator_pub: &str, room_name: &str) -> String {
    let digest = Sha256::digest(format!("{creator_pub}:{room_name}").as_bytes());
    hex(&digest)[..16].to_owned()
}

/// Whether `room_id` derives from `owner_pub` and `room_name`, accepting the
/// owner key with or without base64 padding: a node derived the id from the
/// spelling the owner's messages carried.
pub fn room_id_matches(room_id: &str, owner_pub: &str, room_name: &str) -> bool {
    let bare = owner_pub.trim_end_matches('=');
    let padded = format!("{bare}=");
    [owner_pub, bare, padded.as_str()]
        .iter()
        .any(|owner| derive_room_id(owner, room_name) == room_id)
}

/// The commitment a certificate carries for `secret`.
pub fn room_secret_hash(room_id: &str, secret: &str) -> String {
    let digest = Sha256::digest(format!("{ROOM_SECRET_LABEL}|{room_id}|{secret}").as_bytes());
    hex(&digest)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn decode_b64url(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn owner() -> (SigningKey, String) {
        let key = SigningKey::generate(&mut OsRng);
        // Clients spell their id with padding.
        let id = URL_SAFE.encode(key.verifying_key().as_bytes());
        (key, id)
    }

    fn cert(key: &SigningKey, id: &str, room_type: &str, secret: &str) -> RoomCertificate {
        RoomCertificate::new(id, "General", room_type, "owner", secret, 1_000)
            .signed(|b| key.sign(b).to_bytes().to_vec())
    }

    #[test]
    fn signed_certificate_verifies_and_round_trips() {
        let (key, id) = owner();
        let c = cert(&key, &id, "private", "s3cret");
        assert_eq!(c.verify(), Ok(()));
        assert_eq!(c.room_id, derive_room_id(&id, "General"));
        assert_eq!(RoomCertificate::from_json_verified(&c.to_json()), Some(c));
    }

    #[test]
    fn known_room_id_matches_the_supernode_derivation() {
        // sha256("owner:General")[..16]; the supernode's `derive_room_id`
        // must give the same value.
        assert_eq!(derive_room_id("owner", "General"), "722056d713a3077d");
    }

    #[test]
    fn tampered_fields_fail() {
        let (key, id) = owner();
        let good = cert(&key, &id, "private", "s3cret");

        let mut public = good.clone();
        public.room_type = "public".into();
        assert!(public.verify().is_err(), "type flipped to public");

        let mut renamed = good.clone();
        renamed.room_name = "Other".into();
        assert_eq!(renamed.verify(), Err(RoomCertError::RoomId));

        let mut stolen = good.clone();
        stolen.room_id = derive_room_id("someone-else", "General");
        assert_eq!(stolen.verify(), Err(RoomCertError::RoomId));

        let mut members = good.clone();
        members.invite_policy = "members".into();
        assert_eq!(members.verify(), Err(RoomCertError::Signature));

        let mut secret = good;
        secret.secret_hash = room_secret_hash(&secret.room_id, "guess");
        assert_eq!(secret.verify(), Err(RoomCertError::Signature));
    }

    #[test]
    fn another_key_cannot_sign_for_the_owner() {
        let (_, id) = owner();
        let (attacker, _) = owner();
        let forged = cert(&attacker, &id, "private", "x");
        assert_eq!(forged.verify(), Err(RoomCertError::Signature));
    }

    #[test]
    fn another_owner_cannot_certify_the_same_room_id() {
        let (victim_key, victim) = owner();
        let (attacker_key, attacker) = owner();
        let real = cert(&victim_key, &victim, "private", "x");
        let mut squat = cert(&attacker_key, &attacker, "public", "");
        squat.room_id = real.room_id.clone();
        squat = squat.signed(|b| attacker_key.sign(b).to_bytes().to_vec());
        assert_eq!(squat.verify(), Err(RoomCertError::RoomId));
    }

    #[test]
    fn only_the_committed_secret_admits() {
        let (key, id) = owner();
        let c = cert(&key, &id, "private", "s3cret");
        assert!(c.admits_secret("s3cret"));
        assert!(!c.admits_secret("x"));
        assert!(!c.admits_secret(""));
        let public = cert(&key, &id, "public", "s3cret");
        assert!(public.secret_hash.is_empty());
        assert!(!public.admits_secret("s3cret"));
    }

    #[test]
    fn owner_matches_either_padding() {
        let (key, id) = owner();
        let c = cert(&key, &id, "public", "");
        assert!(c.is_owner(&id));
        assert!(c.is_owner(id.trim_end_matches('=')));
        assert!(!c.is_owner(""));
        assert!(!c.is_owner("someone"));
    }

    #[test]
    fn unpadded_owner_ids_bind_the_same_room() {
        let (key, id) = owner();
        let bare = id.trim_end_matches('=').to_owned();
        let mut c = RoomCertificate::new(&bare, "General", "public", "owner", "", 5);
        // The node derived the id from the padded spelling.
        c.room_id = derive_room_id(&id, "General");
        let c = c.signed(|b| key.sign(b).to_bytes().to_vec());
        assert_eq!(c.verify(), Ok(()));
    }

    #[test]
    fn later_certificate_from_the_owner_supersedes() {
        let (key, id) = owner();
        let first = cert(&key, &id, "private", "a");
        let mut second = RoomCertificate::new(&id, "General", "private", "owner", "b", 2_000);
        second = second.signed(|b| key.sign(b).to_bytes().to_vec());
        assert!(second.supersedes(&first));
        assert!(!first.supersedes(&second));
        assert!(!first.supersedes(&first));
    }
}
