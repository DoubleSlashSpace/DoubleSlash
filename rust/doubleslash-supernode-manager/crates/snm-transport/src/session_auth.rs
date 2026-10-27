//! Credentials and host-key trust supplied by an embedding application.
//!
//! The CLI and TUI authenticate from `~/.ssh`, environment variables, and a
//! terminal prompt, and trust a new host key on first sight. An application
//! that drives the manager from its own UI (the DoubleSlash desktop client)
//! has none of those: it holds its own key, asks for a password in a dialog,
//! and pins the host key the user confirmed. [`SessionAuth`] carries exactly
//! that, and an embedded transport built with it never reads the environment,
//! never prompts, and refuses any host key other than the pinned one.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client;
use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{decode_secret_key, HashAlg, PrivateKey, PublicKey, PublicKeyOrCertificate};
use zeroize::Zeroizing;

use crate::traits::TransportError;

/// Explicit credentials for one SSH login.
#[derive(Clone)]
pub struct SessionAuth {
    pub(crate) keys: Vec<Arc<PrivateKey>>,
    pub(crate) password: Option<Zeroizing<String>>,
    pub(crate) default_identity_files: bool,
    pub(crate) host_key: PublicKey,
}

impl SessionAuth {
    /// Credentials that trust only `host_key`. Add at least one key or a
    /// password before connecting.
    pub fn pinned(host_key: PublicKey) -> Self {
        Self {
            keys: Vec::new(),
            password: None,
            default_identity_files: false,
            host_key,
        }
    }

    pub fn with_key(mut self, key: PrivateKey) -> Self {
        self.keys.push(Arc::new(key));
        self
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        let password = password.into();
        self.password = (!password.is_empty()).then(|| Zeroizing::new(password));
        self
    }

    /// Also try the user's own `~/.ssh/id_*` keys, for "bring your own key".
    pub fn with_default_identity_files(mut self) -> Self {
        self.default_identity_files = true;
        self
    }

    pub fn host_key(&self) -> &PublicKey {
        &self.host_key
    }

    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }
}

impl fmt::Debug for SessionAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionAuth")
            .field("keys", &self.keys.len())
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("default_identity_files", &self.default_identity_files)
            .field("host_key", &host_key_fingerprint(&self.host_key))
            .finish()
    }
}

/// A freshly generated login key, in OpenSSH encodings.
pub struct GeneratedKey {
    /// `-----BEGIN OPENSSH PRIVATE KEY-----` block, unencrypted. The caller
    /// is responsible for storing it encrypted.
    pub private_openssh: Zeroizing<String>,
    /// One `ssh-ed25519 AAAA… comment` line for `authorized_keys`.
    pub public_openssh: String,
}

/// Generate an ed25519 login key labelled `comment`.
pub fn generate_ed25519_key(comment: &str) -> Result<GeneratedKey, TransportError> {
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::getrandom(seed.as_mut())
        .map_err(|e| TransportError::Other(format!("generate key seed: {e}")))?;
    let mut key = PrivateKey::from(Ed25519Keypair::from_seed(&seed));
    key.set_comment(comment);
    let private_openssh = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| TransportError::Other(format!("encode private key: {e}")))?;
    let public_openssh = key
        .public_key()
        .to_openssh()
        .map_err(|e| TransportError::Other(format!("encode public key: {e}")))?;
    Ok(GeneratedKey {
        private_openssh,
        public_openssh,
    })
}

/// Parse an OpenSSH or PEM private key, decrypting it with `passphrase`.
pub fn parse_private_key(
    text: &str,
    passphrase: Option<&str>,
) -> Result<PrivateKey, TransportError> {
    decode_secret_key(text, passphrase)
        .map_err(|e| TransportError::Other(format!("parse private key: {e}")))
}

/// Parse one `ssh-ed25519 AAAA…` line, as stored for a pinned host key.
pub fn parse_public_key(line: &str) -> Result<PublicKey, TransportError> {
    PublicKey::from_openssh(line.trim())
        .map_err(|e| TransportError::Other(format!("parse public key: {e}")))
}

/// The `SHA256:…` form OpenSSH prints, for a user to compare with the
/// fingerprint their hosting provider shows.
pub fn host_key_fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// Same key material; the comment is not part of a host's identity.
pub(crate) fn host_key_matches(pinned: &PublicKey, offered: &PublicKey) -> bool {
    pinned.key_data() == offered.key_data()
}

/// Connect to `host:port` only far enough to learn its host key, without
/// authenticating. The caller shows the fingerprint, and the user's
/// confirmation turns it into [`SessionAuth::pinned`].
pub async fn fetch_host_key(host: &str, port: u16) -> Result<PublicKey, TransportError> {
    let seen = Arc::new(Mutex::new(None));
    let handler = CaptureHostKey { seen: seen.clone() };
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(15)),
        ..<_>::default()
    });
    let connect = client::connect(config, (host, port), handler);
    // Rejecting the key is how the handshake stops; the error that produces
    // is expected, and only a missing key is a failure.
    let outcome = tokio::time::timeout(Duration::from_secs(20), connect)
        .await
        .map_err(|_| TransportError::Other(format!("ssh connect to {host}:{port} timed out")))?;
    let captured = seen.lock().ok().and_then(|mut slot| slot.take());
    match (captured, outcome) {
        (Some(key), _) => Ok(key),
        (None, Err(e)) => Err(TransportError::Other(format!("ssh connect: {e}"))),
        (None, Ok(_)) => Err(TransportError::Other(format!(
            "{host}:{port} completed a handshake without presenting a host key"
        ))),
    }
}

struct CaptureHostKey {
    seen: Arc<Mutex<Option<PublicKey>>>,
}

impl client::Handler for CaptureHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        if let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key {
            if let Ok(mut slot) = self.seen.lock() {
                *slot = Some(key.clone());
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_round_trips_and_is_ed25519() {
        let generated = generate_ed25519_key("doubleslash@test").unwrap();
        assert!(generated.public_openssh.starts_with("ssh-ed25519 "));
        assert!(generated.public_openssh.ends_with(" doubleslash@test"));
        let parsed = parse_private_key(&generated.private_openssh, None).unwrap();
        assert_eq!(
            parsed.public_key().to_openssh().unwrap(),
            generated.public_openssh
        );
    }

    #[test]
    fn two_generated_keys_differ() {
        let a = generate_ed25519_key("a").unwrap();
        let b = generate_ed25519_key("a").unwrap();
        assert_ne!(a.public_openssh, b.public_openssh);
    }

    #[test]
    fn host_key_match_ignores_comment_but_not_material() {
        let a = generate_ed25519_key("one").unwrap();
        let b = generate_ed25519_key("two").unwrap();
        let a_key = parse_public_key(&a.public_openssh).unwrap();
        let a_renamed = parse_public_key(&a.public_openssh.replace(" one", " other")).unwrap();
        let b_key = parse_public_key(&b.public_openssh).unwrap();
        assert!(host_key_matches(&a_key, &a_renamed));
        assert!(!host_key_matches(&a_key, &b_key));
    }

    #[test]
    fn fingerprint_uses_openssh_sha256_form() {
        let a = generate_ed25519_key("x").unwrap();
        let key = parse_public_key(&a.public_openssh).unwrap();
        assert!(host_key_fingerprint(&key).starts_with("SHA256:"));
    }

    #[test]
    fn debug_redacts_password() {
        let a = generate_ed25519_key("x").unwrap();
        let key = parse_public_key(&a.public_openssh).unwrap();
        let auth = SessionAuth::pinned(key).with_password("hunter2");
        let shown = format!("{auth:?}");
        assert!(!shown.contains("hunter2"));
        assert!(shown.contains("<redacted>"));
    }
}
