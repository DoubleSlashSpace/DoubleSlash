//! Room certificates this client holds, and the secrets they commit to.
//!
//! A supernode keeps rooms in memory, so it forgets them when it restarts or
//! a room sits idle. Any member, not only the owner, brings a room back by
//! presenting its owner-signed certificate
//! ([`doubleslash_features::room_cert`]); the secret the certificate commits
//! to then admits that member to a private room. The manager attaches both to
//! every create, join, subscribe and invite it sends for the room, so the
//! room returns whichever of those reaches a node first.
//!
//! The room store is the durable copy. The clients hand the manager what it
//! holds on each reconnect replay (`CreateRoom`); the manager reports what it
//! learns or signs with [`ConnectionEvent::RoomCertificateLearned`].

use std::collections::BTreeMap;

use doubleslash_features::RoomCertificate;
use serde_json::Value;
use tracing::{debug, info};

use super::super::events::ConnectionEvent;
use super::ConnectionManager;

/// A room's certificate with the secret it commits to, when this client
/// knows it. Public rooms, and private rooms admitted by Space grant, have no
/// secret here.
#[derive(Debug, Clone)]
pub(super) struct RoomCredential {
    pub cert: RoomCertificate,
    pub secret: String,
}

/// A fresh join secret for a private room this client creates.
fn new_room_secret() -> String {
    crate::crypto::generate_nonce_hex(16)
}

impl ConnectionManager {
    /// Hold `cert` for its room, with `secret` when the certificate commits to
    /// it. A certificate that does not verify, or is not newer than the one
    /// held from the same owner, is ignored. Returns whether `cert` is now
    /// the held certificate and differs from what was held before.
    pub(super) fn learn_room_cert(&mut self, cert: RoomCertificate, secret: &str) -> bool {
        if cert.verify().is_err() {
            debug!(
                "Ignoring a room certificate for {} that does not verify",
                &cert.room_id[..12.min(cert.room_id.len())]
            );
            return false;
        }
        match self.room_creds.get_mut(&cert.room_id) {
            Some(held) if held.cert == cert => {
                if held.secret.is_empty() && cert.admits_secret(secret) {
                    held.secret = secret.to_owned();
                }
                false
            }
            Some(held) if !cert.supersedes(&held.cert) => false,
            held => {
                // A rotated certificate may still commit to the secret we hold.
                let kept = held.map(|h| h.secret.clone()).unwrap_or_default();
                let secret = [secret, kept.as_str()]
                    .into_iter()
                    .find(|s| cert.admits_secret(s))
                    .unwrap_or_default()
                    .to_owned();
                self.room_creds
                    .insert(cert.room_id.clone(), RoomCredential { cert, secret });
                true
            }
        }
    }

    /// Record `secret` for `room_id` when the held certificate commits to it.
    pub(super) fn learn_room_secret(&mut self, room_id: &str, secret: &str) {
        if let Some(held) = self.room_creds.get_mut(room_id) {
            if held.secret.is_empty() && held.cert.admits_secret(secret) {
                held.secret = secret.to_owned();
            }
        }
    }

    /// Hold the certificates a node's room list carries. The room store
    /// keeps its own copy from the same list, so nothing is reported.
    pub(super) fn learn_room_certs_from_list(&mut self, rooms: Option<&Value>) {
        let Some(rooms) = rooms.and_then(Value::as_array) else {
            return;
        };
        for room in rooms {
            if let Some(cert) = room
                .get("room_cert")
                .and_then(RoomCertificate::from_value_verified)
            {
                self.learn_room_cert(cert, "");
            }
        }
    }

    /// Put `room_id`'s certificate in `payload`, and, when `with_secret` and
    /// the payload carries no token yet, the secret it commits to.
    pub(super) fn attach_room_credentials(
        &self,
        room_id: &str,
        payload: &mut BTreeMap<String, Value>,
        with_secret: bool,
    ) {
        let Some(held) = self.room_creds.get(room_id) else {
            return;
        };
        payload.insert("room_cert".to_owned(), held.cert.to_value());
        let has_token = payload
            .get("invite_token")
            .and_then(Value::as_str)
            .is_some_and(|t| !t.is_empty());
        if with_secret && !has_token && !held.secret.is_empty() {
            payload.insert(
                "invite_token".to_owned(),
                Value::String(held.secret.clone()),
            );
        }
    }

    /// Sign a certificate for a room this client creates, or replays as its
    /// owner, when none is held for it yet. A private room commits to
    /// `secret`, or to a fresh one when `secret` is empty. Returns `None` when
    /// `room_id` is not the id this identity and `room_name` derive: a
    /// temporary call room, the built-in room, or someone else's.
    pub(super) fn sign_own_room_cert(
        &mut self,
        room_id: Option<&str>,
        room_name: &str,
        room_type: &str,
        invite_policy: &str,
        secret: &str,
    ) -> Option<RoomCertificate> {
        let me = self.identity.public_id();
        let own_id = doubleslash_features::room_cert::derive_room_id(&me, room_name);
        if room_id.is_some_and(|id| id != own_id) {
            return None;
        }
        if let Some(held) = self.room_creds.get(&own_id) {
            return Some(held.cert.clone());
        }
        let secret = match (room_type == "private", secret.is_empty()) {
            (true, true) => new_room_secret(),
            (true, false) => secret.to_owned(),
            (false, _) => String::new(),
        };
        let issued_at = super::unix_now_secs();
        let identity = std::sync::Arc::clone(&self.identity);
        let cert =
            RoomCertificate::new(&me, room_name, room_type, invite_policy, &secret, issued_at)
                .signed(|bytes| identity.sign(bytes));
        info!(
            "Signed a certificate for our room '{room_name}' ({})",
            &own_id[..12.min(own_id.len())]
        );
        self.learn_room_cert(cert.clone(), &secret);
        self.unpersisted_room_certs.insert(own_id);
        Some(cert)
    }

    /// Report `room_id`'s certificate, with its secret, for the room store.
    pub(super) fn emit_room_cert(&self, room_id: &str) {
        let Some(held) = self.room_creds.get(room_id) else {
            return;
        };
        self.emit_event(ConnectionEvent::RoomCertificateLearned {
            room_id: room_id.to_owned(),
            room_cert: held.cert.to_json(),
            invite_token: held.secret.clone(),
        });
    }

    /// Report a certificate this client signed once the node has the room,
    /// so the room store records it after the room itself.
    pub(super) fn emit_signed_room_cert(&mut self, room_id: &str) {
        if self.unpersisted_room_certs.remove(room_id) {
            self.emit_room_cert(room_id);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Any member brings a forgotten room back: what the manager signs, keeps
    //! and attaches so a node can recreate a room from its certificate.

    use super::super::room_session::RoomCreateRequest;
    use super::*;
    use crate::identity::Identity;
    use crate::peer_store::{PeerRecord, PeerStore};
    use crate::protocol::{MessageType, SignalingMessage};
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    struct Client {
        manager: ConnectionManager,
        identity: Arc<Identity>,
        node: Identity,
        outgoing: mpsc::Receiver<Message>,
        events: mpsc::Receiver<ConnectionEvent>,
        _profile: tempfile::TempDir,
    }

    fn client() -> Client {
        let profile = tempfile::tempdir().unwrap();
        let identity = Arc::new(Identity::generate());
        let store = PeerStore::open(&identity, Some(&profile.path().join("peers.dat"))).unwrap();
        let store = Arc::new(RwLock::new(store));
        let node = Identity::generate();
        store.write().upsert(PeerRecord {
            peer_id: node.peer_id(),
            identity_pub: node.public_id(),
            is_supernode: true,
            ..Default::default()
        });
        let (mut manager, events) =
            ConnectionManager::new_for_test(Arc::clone(&identity), Arc::clone(&store));
        let outgoing = manager.test_add_supernode_session(&node.public_id());
        Client {
            manager,
            identity,
            node,
            outgoing,
            events,
            _profile: profile,
        }
    }

    fn sent(client: &mut Client) -> Vec<SignalingMessage> {
        let mut out = Vec::new();
        while let Ok(Message::Text(raw)) = client.outgoing.try_recv() {
            out.push(SignalingMessage::from_json(&raw).unwrap());
        }
        out
    }

    fn events(client: &mut Client) -> Vec<ConnectionEvent> {
        let mut out = Vec::new();
        while let Ok(event) = client.events.try_recv() {
            out.push(event);
        }
        out
    }

    fn create<'a>(
        node: &'a str,
        name: &'a str,
        room_id: Option<&'a str>,
        creator: Option<&'a str>,
        token: &'a str,
        cert: &'a str,
    ) -> RoomCreateRequest<'a> {
        RoomCreateRequest {
            supernode_id: node,
            room_name: name,
            room_type: "private",
            room_id,
            creator_id: creator,
            materialize_only: room_id.is_some(),
            invite_policy: "owner",
            invite_token: token,
            room_cert: cert,
        }
    }

    fn owners_cert(owner: &Identity, name: &str, secret: &str) -> RoomCertificate {
        RoomCertificate::new(&owner.public_id(), name, "private", "owner", secret, 7)
            .signed(|b| owner.sign(b))
    }

    fn sign(identity: &Identity, msg: &mut SignalingMessage) {
        use base64::Engine;
        let canonical = msg.canonical_bytes().unwrap();
        msg.signature =
            Some(base64::engine::general_purpose::URL_SAFE.encode(identity.sign(&canonical)));
    }

    #[tokio::test]
    async fn creating_a_room_signs_its_certificate_and_reports_it_after_the_room() {
        let mut c = client();
        let node = c.node.public_id();
        c.manager
            .send_room_create(create(&node, "Den", None, None, "", ""))
            .await;
        let out = sent(&mut c);
        let msg = out
            .iter()
            .find(|m| m.msg_type == MessageType::SfuRoomCreate)
            .expect("SfuRoomCreate");
        let cert = RoomCertificate::from_value_verified(&msg.payload["room_cert"])
            .expect("a certificate that verifies");
        assert!(cert.is_owner(&c.identity.public_id()));
        assert!(cert.is_private());
        let secret = msg.payload["invite_token"].as_str().unwrap().to_owned();
        assert!(cert.admits_secret(&secret), "the create carries our secret");

        let mut reply = SignalingMessage::new(MessageType::SfuRoomCreated, node.clone());
        for (k, v) in [
            ("room_id", cert.room_id.as_str()),
            ("room_name", "Den"),
            ("room_type", "private"),
            ("invite_token", secret.as_str()),
        ] {
            reply
                .payload
                .insert(k.to_owned(), Value::String(v.to_owned()));
        }
        sign(&c.node, &mut reply);
        c.manager.handle_inbound_from_supernode(node, reply).await;

        let order: Vec<&str> = events(&mut c)
            .iter()
            .filter_map(|e| match e {
                ConnectionEvent::RoomCreated { .. } => Some("created"),
                ConnectionEvent::RoomCertificateLearned {
                    room_cert,
                    invite_token,
                    ..
                } => {
                    assert_eq!(invite_token, &secret);
                    assert_eq!(
                        RoomCertificate::from_json_verified(room_cert).as_ref(),
                        Some(&cert)
                    );
                    Some("cert")
                }
                _ => None,
            })
            .collect();
        assert_eq!(order, vec!["created", "cert"]);
    }

    /// A member's replay, and every later join and subscribe, carries the
    /// owner's certificate and our secret, so any of them brings the room
    /// back on a node that forgot it.
    #[tokio::test]
    async fn a_member_carries_the_owners_certificate_to_every_room_message() {
        let mut c = client();
        let node = c.node.public_id();
        let owner = Identity::generate();
        let cert = owners_cert(&owner, "Den", "sec");
        let owner_pub = owner.public_id();
        let cert_json = cert.to_json();
        c.manager
            .send_room_create(create(
                &node,
                "Den",
                Some(&cert.room_id),
                Some(&owner_pub),
                "sec",
                &cert_json,
            ))
            .await;
        c.manager.send_room_join(&node, &cert.room_id).await;
        c.manager.send_room_subscribe(&node, &cert.room_id).await;
        let out = sent(&mut c);
        for kind in [
            MessageType::SfuRoomCreate,
            MessageType::SfuJoin,
            MessageType::SfuSubscribe,
        ] {
            let msg = out
                .iter()
                .find(|m| m.msg_type == kind)
                .unwrap_or_else(|| panic!("{kind:?} sent"));
            assert_eq!(
                RoomCertificate::from_value_verified(&msg.payload["room_cert"]).as_ref(),
                Some(&cert),
                "{kind:?} carries the owner's certificate"
            );
            assert_eq!(
                msg.payload["invite_token"], "sec",
                "{kind:?} carries the secret"
            );
        }
        assert!(
            !events(&mut c)
                .iter()
                .any(|e| matches!(e, ConnectionEvent::RoomCertificateLearned { .. })),
            "a certificate from the store is not reported back to it"
        );
    }

    /// Replaying someone else's room never signs a certificate of our own for
    /// it, and a token the certificate does not commit to is not attached.
    #[tokio::test]
    async fn a_member_never_signs_for_another_owners_room() {
        let mut c = client();
        let node = c.node.public_id();
        let owner = Identity::generate();
        let owner_pub = owner.public_id();
        let room_id = doubleslash_features::room_cert::derive_room_id(&owner_pub, "Den");
        c.manager
            .send_room_create(create(
                &node,
                "Den",
                Some(&room_id),
                Some(&owner_pub),
                "",
                "",
            ))
            .await;
        // A creator left empty in the store falls back to our own id; the
        // room id still says the room is not ours.
        let me = c.identity.public_id();
        c.manager
            .send_room_create(create(&node, "Den", Some(&room_id), Some(&me), "", ""))
            .await;
        for msg in sent(&mut c) {
            assert!(
                !msg.payload.contains_key("room_cert"),
                "no certificate for a room we do not own"
            );
        }

        let cert = owners_cert(&owner, "Den", "real");
        assert!(c.manager.learn_room_cert(cert.clone(), "guess"));
        c.manager.send_room_join(&node, &cert.room_id).await;
        let join = sent(&mut c).pop().unwrap();
        assert!(join.payload.contains_key("room_cert"));
        assert!(!join.payload.contains_key("invite_token"));
    }

    #[tokio::test]
    async fn a_later_certificate_replaces_an_earlier_one_and_a_forgery_does_not() {
        let mut c = client();
        let owner = Identity::generate();
        let first = owners_cert(&owner, "Den", "a");
        assert!(c.manager.learn_room_cert(first.clone(), "a"));
        assert!(!c.manager.learn_room_cert(first.clone(), "a"), "same one");

        let mut forged = first.clone();
        forged.room_type = "public".into();
        forged.secret_hash.clear();
        assert!(!c.manager.learn_room_cert(forged, ""));

        let second = RoomCertificate::new(&owner.public_id(), "Den", "private", "owner", "a", 8)
            .signed(|b| owner.sign(b));
        assert!(c.manager.learn_room_cert(second.clone(), ""));
        let held = c.manager.room_creds.get(&first.room_id).unwrap();
        assert_eq!(held.cert, second);
        assert_eq!(
            held.secret, "a",
            "a secret the new certificate still commits to stays"
        );
        assert!(
            !c.manager.learn_room_cert(first, "a"),
            "an older one never returns"
        );
    }

    /// Room invite links carry the certificate, whoever makes them.
    #[tokio::test]
    async fn invite_links_carry_the_certificate() {
        let mut c = client();
        let node = c.node.public_id();
        let owner = Identity::generate();
        let cert = owners_cert(&owner, "Den", "sec");
        c.manager.learn_room_cert(cert.clone(), "sec");
        c.manager.supernodes.get_mut(&node).expect("session").ws_url =
            "ws://203.0.113.5:9000".to_owned();
        let url = c
            .manager
            .generate_room_invite_url(&node, &cert.room_id, "Den", "private", "sec", "", "", "")
            .expect("url");
        let fragment = url.split_once('#').unwrap().1;
        let parsed = super::super::invite::parse_room_invite(fragment).unwrap();
        assert_eq!(
            RoomCertificate::from_json_verified(&parsed.room_cert),
            Some(cert)
        );
    }
}
