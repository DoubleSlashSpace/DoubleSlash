//! Trust invites between room members.
//!
//! Sharing a room is not trust: room membership lets two people talk in the
//! room, but not message, call or send files to each other. A trust invite is
//! how one member offers the other that relationship without leaving the room
//! to exchange a link.
//!
//! The offer carries an ordinary personal invite, so accepting it is exactly
//! `AcceptInvite` and the signed invite handshake that follows. What this module
//! adds is only the delivery:
//!
//! * **Sealed.** The invite travels inside an `EncryptedSignal` addressed to the
//!   member. A supernode relaying a cleartext invite could redeem it first and
//!   become the trusted peer itself.
//! * **Bound to its sender.** The invite's `inviter_identity_pub` must be the
//!   identity that signed and sealed the offer, so one member cannot forward
//!   someone else's invite under their own name.
//! * **Asked, never applied.** Receiving an offer only surfaces it to the user;
//!   nothing is trusted until they accept.
//! * **Bounded.** Anyone sharing a room can send one, so offers are limited per
//!   sender and in total, and only accepted from a current member of the room
//!   they name.

use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{debug, info, warn};

use crate::protocol::{MessageType, SignalingMessage};

use super::super::events::ConnectionEvent;
use super::room_session::union_members_for_room;
use super::{unix_now_secs, ConnectionManager};

/// Least time between two offers to the same member, or two shown from the
/// same sender.
pub(super) const TRUST_INVITE_INTERVAL: Duration = Duration::from_secs(60);

/// Most distinct senders whose offers are shown within one interval. Past it,
/// further offers are dropped until the window moves on.
pub(super) const TRUST_INVITE_MAX_SENDERS: usize = 8;

/// Longest invite lifetime accepted inside an offer. Personal invites live 15
/// minutes; anything claiming much longer was not minted by this code.
const TRUST_INVITE_MAX_TTL_SECS: u64 = 60 * 60;

/// Upper bound on the invite URL an offer may carry.
const TRUST_INVITE_MAX_URL_LEN: usize = 4096;

/// Longest self-chosen handle carried through to the prompt.
const TRUST_INVITE_MAX_HANDLE_CHARS: usize = 64;

/// Check that `invite_url` is a live personal invite minted by `sender`.
///
/// Returns the inviter's self-chosen handle, stripped of control characters and
/// shortened, for display. Pure so the rules can be tested without a manager.
pub(super) fn check_trust_invite(
    invite_url: &str,
    sender: &str,
    now: u64,
) -> Result<String, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    if invite_url.len() > TRUST_INVITE_MAX_URL_LEN {
        return Err("invite too large".into());
    }
    let rest = doubleslash_features::normalize_app_url(invite_url)
        .ok_or_else(|| "not an invite link".to_owned())?;
    let Some(("invite", encoded)) = rest.split_once('#') else {
        return Err("not a personal invite".into());
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|_| "invite is not base64url".to_owned())?;
    let payload: Value =
        serde_json::from_slice(&bytes).map_err(|_| "invite is not JSON".to_owned())?;
    let field = |k: &str| payload.get(k).and_then(Value::as_str).unwrap_or("");

    let inviter = field("inviter_identity_pub");
    if inviter.is_empty() || inviter.trim_end_matches('=') != sender.trim_end_matches('=') {
        return Err("invite was minted by someone else".into());
    }
    if payload
        .get("is_supernode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("supernode invites are not offered this way".into());
    }
    if field("invite_id").is_empty() || field("inviter_ephemeral_pub").is_empty() {
        return Err("invite is incomplete".into());
    }
    let expires_at = payload
        .get("expires_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| "invite has no expiry".to_owned())?;
    if expires_at < now {
        return Err("invite expired".into());
    }
    if expires_at > now.saturating_add(TRUST_INVITE_MAX_TTL_SECS) {
        return Err("invite lifetime too long".into());
    }
    // Bidi controls are format characters, not control ones, but in a name
    // shown on a trust prompt they are how "evil" is made to read as "live".
    Ok(field("inviter_handle")
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(TRUST_INVITE_MAX_HANDLE_CHARS)
        .collect())
}

/// The room member in `members` who is `id`, in the spelling membership uses.
fn member_spelling(members: &std::collections::HashSet<String>, id: &str) -> Option<String> {
    let bare = id.trim_end_matches('=');
    members
        .iter()
        .find(|m| m.trim_end_matches('=') == bare)
        .cloned()
}

impl ConnectionManager {
    /// Offer `member_public_id`, a member of `room_id`, a personal invite.
    pub(super) async fn send_trust_invite(&mut self, room_id: &str, member_public_id: &str) {
        let error = match self.try_send_trust_invite(room_id, member_public_id).await {
            Ok(()) => String::new(),
            Err(e) => {
                debug!(
                    "[trust-invite] not sent to {}: {e}",
                    &member_public_id[..8.min(member_public_id.len())]
                );
                e
            }
        };
        self.emit_event(ConnectionEvent::TrustInviteResult {
            member_public_id: member_public_id.to_owned(),
            error,
        });
    }

    async fn try_send_trust_invite(
        &mut self,
        room_id: &str,
        member_public_id: &str,
    ) -> Result<(), String> {
        let me = self.identity.public_id();
        let bare = member_public_id.trim_end_matches('=').to_owned();
        if bare.is_empty() || room_id.is_empty() {
            return Err("no member selected".into());
        }
        if bare == me.trim_end_matches('=') {
            return Err("that is you".into());
        }
        if let Some(rec) = self.peer_store.read().find_identity(member_public_id) {
            return Err(if rec.blocked || rec.revoked {
                "you have blocked this peer".into()
            } else {
                "already a trusted peer".into()
            });
        }
        // Only to someone the room's authoritative membership holds — the same
        // set its key goes to — so this reaches nobody the room did not already
        // put in front of us.
        let members = union_members_for_room(&self.room_group_members, room_id);
        let Some(target) = member_spelling(&members, member_public_id) else {
            return Err("they are no longer in this room".into());
        };
        if self
            .trust_invites_sent
            .get(&bare)
            .is_some_and(|at| at.elapsed() < TRUST_INVITE_INTERVAL)
        {
            return Err("invite already sent — give them a minute".into());
        }

        let invite_url = self
            .generate_invite_url()
            .ok_or_else(|| "could not create an invite".to_owned())?;
        let mut inner = SignalingMessage::new(MessageType::TrustRequest, me);
        inner.source_device = self.device_id;
        inner
            .payload
            .insert("room_id".to_owned(), Value::String(room_id.to_owned()));
        inner
            .payload
            .insert("invite_url".to_owned(), Value::String(invite_url));
        let canonical = inner
            .canonical_bytes()
            .map_err(|_| "could not sign the invite".to_owned())?;
        let sig = self.identity.sign(&canonical);
        use base64::Engine;
        inner.signature = Some(base64::engine::general_purpose::URL_SAFE.encode(sig));
        let env = self
            .seal_signal_to_member(&inner, &target)
            .ok_or_else(|| "could not seal the invite".to_owned())?;
        self.dispatch_outbound(env).await;

        self.trust_invites_sent
            .retain(|_, at| at.elapsed() < TRUST_INVITE_INTERVAL);
        self.trust_invites_sent.insert(bare, Instant::now());
        info!(
            "[trust-invite] offered to {} in room {}",
            &target[..8.min(target.len())],
            &room_id[..8.min(room_id.len())]
        );
        Ok(())
    }

    /// A sealed `TrustRequest` arrived. Surface it if it is well-formed, from a
    /// current room member we hold no record of, and within the rate limits.
    ///
    /// Callers must only pass messages that came out of an `EncryptedSignal`:
    /// a cleartext offer has crossed a supernode that could redeem its invite.
    pub(super) fn handle_trust_request(&mut self, msg: &SignalingMessage) {
        let sender = msg.sender.as_str();
        let short = &sender[..8.min(sender.len())];
        let bare = sender.trim_end_matches('=').to_owned();

        // Any record at all — trusted, blocked, revoked, or a supernode — means
        // there is nothing to ask: either they are trusted already or the user
        // has decided against them.
        if self.peer_store.read().find_identity(sender).is_some() {
            debug!("[trust-invite] from {short}: already known — ignoring");
            return;
        }
        let room_id = msg
            .payload
            .get("room_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if room_id.is_empty() {
            return;
        }
        let members = union_members_for_room(&self.room_group_members, room_id);
        if member_spelling(&members, sender).is_none() {
            debug!(
                "[trust-invite] from {short}: not a member of room {} — ignoring",
                &room_id[..8.min(room_id.len())]
            );
            return;
        }
        let invite_url = msg
            .payload
            .get("invite_url")
            .and_then(Value::as_str)
            .unwrap_or("");
        let handle = match check_trust_invite(invite_url, sender, unix_now_secs()) {
            Ok(h) => h,
            Err(e) => {
                warn!("[trust-invite] from {short}: rejected — {e}");
                return;
            }
        };

        self.trust_invites_received
            .retain(|_, at| at.elapsed() < TRUST_INVITE_INTERVAL);
        if self.trust_invites_received.contains_key(&bare) {
            debug!("[trust-invite] from {short}: rate limited");
            return;
        }
        if self.trust_invites_received.len() >= TRUST_INVITE_MAX_SENDERS {
            warn!("[trust-invite] from {short}: too many offers this minute — dropped");
            return;
        }
        self.trust_invites_received.insert(bare, Instant::now());

        info!(
            "[trust-invite] offer from {short} in room {}",
            &room_id[..8.min(room_id.len())]
        );
        self.emit_event(ConnectionEvent::TrustInviteReceived {
            sender_public_id: sender.to_owned(),
            handle,
            room_id: room_id.to_owned(),
            invite_url: invite_url.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    const SENDER: &str = "c2VuZGVyLWlkZW50aXR5LWtleS1ieXRlcy0wMTIzNDU2Nzg=";
    const NOW: u64 = 1_800_000_000;

    fn invite(fields: serde_json::Value) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(fields.to_string().as_bytes());
        doubleslash_features::mint_invite_https("invite", &encoded)
    }

    fn good() -> serde_json::Value {
        serde_json::json!({
            "inviter_peer_id": "abcd",
            "inviter_identity_pub": SENDER,
            "invite_id": "id-1",
            "expires_at": NOW + 900,
            "inviter_ephemeral_pub": "eph",
            "inviter_handle": "alice",
        })
    }

    #[test]
    fn accepts_a_live_invite_from_its_sender() {
        assert_eq!(
            check_trust_invite(&invite(good()), SENDER, NOW),
            Ok("alice".into())
        );
    }

    #[test]
    fn padding_does_not_decide_who_the_sender_is() {
        let unpadded = SENDER.trim_end_matches('=');
        assert!(check_trust_invite(&invite(good()), unpadded, NOW).is_ok());
    }

    #[test]
    fn rejects_an_invite_minted_by_someone_else() {
        let mut v = good();
        v["inviter_identity_pub"] = "b3RoZXI=".into();
        assert!(check_trust_invite(&invite(v), SENDER, NOW).is_err());
    }

    #[test]
    fn rejects_expired_and_overlong_invites() {
        let mut expired = good();
        expired["expires_at"] = (NOW - 1).into();
        assert!(check_trust_invite(&invite(expired), SENDER, NOW).is_err());

        let mut overlong = good();
        overlong["expires_at"] = (NOW + TRUST_INVITE_MAX_TTL_SECS + 1).into();
        assert!(check_trust_invite(&invite(overlong), SENDER, NOW).is_err());

        let mut none = good();
        none.as_object_mut().unwrap().remove("expires_at");
        assert!(check_trust_invite(&invite(none), SENDER, NOW).is_err());
    }

    #[test]
    fn rejects_supernode_room_and_incomplete_invites() {
        let mut sn = good();
        sn["is_supernode"] = true.into();
        assert!(check_trust_invite(&invite(sn), SENDER, NOW).is_err());

        let room = doubleslash_features::mint_invite_https(
            "room",
            &URL_SAFE_NO_PAD.encode(good().to_string().as_bytes()),
        );
        assert!(check_trust_invite(&room, SENDER, NOW).is_err());

        let mut no_eph = good();
        no_eph
            .as_object_mut()
            .unwrap()
            .remove("inviter_ephemeral_pub");
        assert!(check_trust_invite(&invite(no_eph), SENDER, NOW).is_err());

        assert!(check_trust_invite("https://example.com/", SENDER, NOW).is_err());
    }

    // ── Between two managers ─────────────────────────────────────────────

    use crate::identity::Identity;
    use parking_lot::RwLock;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    const ROOM: &str = "room";
    const HOST: &str = "host";

    struct Client {
        manager: ConnectionManager,
        outgoing: mpsc::Receiver<Message>,
        events: mpsc::Receiver<ConnectionEvent>,
        _profile: tempfile::TempDir,
    }

    fn client() -> Client {
        let identity = Arc::new(Identity::generate());
        let profile = tempfile::tempdir().unwrap();
        let store =
            crate::peer_store::PeerStore::open(&identity, Some(&profile.path().join("peers.dat")))
                .unwrap();
        let (mut manager, events) =
            ConnectionManager::new_for_test(identity, Arc::new(RwLock::new(store)));
        let outgoing = manager.test_add_supernode_session(HOST);
        Client {
            manager,
            outgoing,
            events,
            _profile: profile,
        }
    }

    /// Two clients sharing `ROOM`, each listing the other as a member.
    fn roommates() -> (Client, Client) {
        let a = client();
        let b = client();
        let key = format!("{HOST}:{ROOM}");
        let (a_pub, b_pub) = (
            a.manager.identity.public_id(),
            b.manager.identity.public_id(),
        );
        let (mut a, mut b) = (a, b);
        a.manager
            .room_group_members
            .insert(key.clone(), HashSet::from([b_pub]));
        b.manager
            .room_group_members
            .insert(key, HashSet::from([a_pub]));
        (a, b)
    }

    /// Hand every queued message from `source` to `target`; returns the raw
    /// frames so a test can check what crossed the supernode.
    async fn forward(source: &mut Client, target: &mut Client) -> Vec<String> {
        let mut seen = Vec::new();
        while let Ok(Message::Text(raw)) = source.outgoing.try_recv() {
            let message = SignalingMessage::from_json(&raw).unwrap();
            seen.push(raw.to_string());
            target
                .manager
                .handle_inbound_from_supernode(HOST.to_owned(), message)
                .await;
        }
        seen
    }

    fn drain(events: &mut mpsc::Receiver<ConnectionEvent>) -> Vec<ConnectionEvent> {
        let mut out = Vec::new();
        while let Ok(e) = events.try_recv() {
            out.push(e);
        }
        out
    }

    fn offers(events: &[ConnectionEvent]) -> Vec<(String, String)> {
        events
            .iter()
            .filter_map(|e| match e {
                ConnectionEvent::TrustInviteReceived {
                    sender_public_id,
                    invite_url,
                    ..
                } => Some((sender_public_id.clone(), invite_url.clone())),
                _ => None,
            })
            .collect()
    }

    fn result_error(events: &[ConnectionEvent]) -> Option<String> {
        events.iter().find_map(|e| match e {
            ConnectionEvent::TrustInviteResult { error, .. } => Some(error.clone()),
            _ => None,
        })
    }

    #[tokio::test]
    async fn an_offer_reaches_the_member_sealed_and_bound_to_its_sender() {
        let (mut a, mut b) = roommates();
        let a_pub = a.manager.identity.public_id();
        let b_pub = b.manager.identity.public_id();

        a.manager.send_trust_invite(ROOM, &b_pub).await;
        assert_eq!(result_error(&drain(&mut a.events)), Some(String::new()));

        let frames = forward(&mut a, &mut b).await;
        assert_eq!(frames.len(), 1);
        assert!(
            !frames[0].contains("invite_url") && !frames[0].contains("doubleslash.space"),
            "the invite must not cross the supernode in the clear"
        );

        let got = offers(&drain(&mut b.events));
        assert_eq!(got.len(), 1, "exactly one prompt");
        assert_eq!(got[0].0, a_pub);
        assert!(check_trust_invite(&got[0].1, &a_pub, unix_now_secs()).is_ok());
    }

    #[tokio::test]
    async fn a_repeat_offer_within_the_interval_is_not_sent() {
        let (mut a, _b) = roommates();
        let b_pub = _b.manager.identity.public_id();

        a.manager.send_trust_invite(ROOM, &b_pub).await;
        a.manager.send_trust_invite(ROOM, &b_pub).await;
        let errors: Vec<String> = drain(&mut a.events)
            .into_iter()
            .filter_map(|e| match e {
                ConnectionEvent::TrustInviteResult { error, .. } => Some(error),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].is_empty());
        assert!(!errors[1].is_empty(), "second click refused");
    }

    #[tokio::test]
    async fn the_receiver_rate_limits_a_sender_that_ignores_its_own_limit() {
        let (mut a, mut b) = roommates();
        let b_pub = b.manager.identity.public_id();

        a.manager.send_trust_invite(ROOM, &b_pub).await;
        forward(&mut a, &mut b).await;
        // A modified sender skips its own interval.
        a.manager.trust_invites_sent.clear();
        a.manager.send_trust_invite(ROOM, &b_pub).await;
        forward(&mut a, &mut b).await;

        assert_eq!(offers(&drain(&mut b.events)).len(), 1);
    }

    #[tokio::test]
    async fn offers_are_refused_from_and_to_anyone_outside_the_room() {
        let (mut a, mut b) = roommates();
        let b_pub = b.manager.identity.public_id();

        // A sender with no room in common gets nowhere on its own side…
        a.manager.send_trust_invite("elsewhere", &b_pub).await;
        assert!(!result_error(&drain(&mut a.events)).unwrap().is_empty());

        // …and a receiver that no longer lists them ignores the offer.
        a.manager.send_trust_invite(ROOM, &b_pub).await;
        b.manager.room_group_members.clear();
        forward(&mut a, &mut b).await;
        assert!(offers(&drain(&mut b.events)).is_empty());
    }

    #[tokio::test]
    async fn a_cleartext_offer_is_dropped() {
        let (a, mut b) = roommates();
        let a_id = a.manager.identity.clone();
        let invite_url = {
            let mut a = a;
            a.manager.generate_invite_url().unwrap()
        };
        let mut msg = SignalingMessage::new(MessageType::TrustRequest, a_id.public_id());
        msg.target = Some(b.manager.identity.public_id());
        msg.payload
            .insert("room_id".to_owned(), Value::String(ROOM.to_owned()));
        msg.payload
            .insert("invite_url".to_owned(), Value::String(invite_url));
        let sig = a_id.sign(&msg.canonical_bytes().unwrap());
        msg.signature = Some(base64::engine::general_purpose::URL_SAFE.encode(sig));

        b.manager
            .handle_inbound_from_supernode(HOST.to_owned(), msg)
            .await;
        assert!(offers(&drain(&mut b.events)).is_empty());
    }

    #[tokio::test]
    async fn no_offer_goes_to_or_comes_from_a_peer_already_known() {
        let (mut a, mut b) = roommates();
        let a_pub = a.manager.identity.public_id();
        let b_pub = b.manager.identity.public_id();
        let known = |id: &str| crate::peer_store::PeerRecord {
            peer_id: id.to_owned(),
            identity_pub: id.to_owned(),
            ..Default::default()
        };

        a.manager.peer_store.write().upsert(known(&b_pub));
        a.manager.send_trust_invite(ROOM, &b_pub).await;
        assert!(!result_error(&drain(&mut a.events)).unwrap().is_empty());

        // The receiver side holds its own record — e.g. it blocked the sender.
        a.manager.peer_store.write().remove(&b_pub);
        let mut blocked = known(&a_pub);
        blocked.blocked = true;
        b.manager.peer_store.write().upsert(blocked);
        a.manager.send_trust_invite(ROOM, &b_pub).await;
        forward(&mut a, &mut b).await;
        assert!(offers(&drain(&mut b.events)).is_empty());
    }

    #[test]
    fn handle_is_cleaned_for_display() {
        let mut v = good();
        v["inviter_handle"] = format!("a\u{202e}\u{7}b{}", "x".repeat(200)).into();
        let handle = check_trust_invite(&invite(v), SENDER, NOW).unwrap();
        assert!(!handle.contains('\u{7}'));
        assert!(!handle.contains('\u{202e}'));
        assert_eq!(handle.chars().count(), TRUST_INVITE_MAX_HANDLE_CHARS);
    }
}
