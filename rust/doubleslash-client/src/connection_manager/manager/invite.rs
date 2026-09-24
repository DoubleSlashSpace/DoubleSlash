//! Peer and room invite URL generation, parse, and handshake.

use std::time::Instant;

use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{info, warn};

use crate::protocol::{MessageType, SignalingMessage};

use super::super::events::ConnectionEvent;
use super::super::internal::{PendingInvite, INVITE_REJECT_GRACE};
use super::ConnectionManager;

use super::{parse_quic_lan_hint, unix_now_f64, unix_now_secs};

/// Most issued-but-unredeemed invites remembered at once.
const MAX_ISSUED_INVITES: usize = 256;

/// breaking change to [`build_room_invite_url`] / [`parse_room_invite`] and add
/// migration handling in the parser.
pub const ROOM_INVITE_SCHEMA: u32 = 1;

/// URL-level freshness guard for a shared room invite (24h). The supernode's
/// own token TTL is authoritative; this just stops stale links from dialing.
pub const ROOM_INVITE_TTL_SECS: u64 = 24 * 60 * 60;

/// Decoded fields of a `d://room#…` / `doubleslash://room#…` invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomInvitePayload {
    pub supernode_id: String,
    pub supernode_hint: String,
    pub room_id: String,
    pub room_name: String,
    pub room_type: String,
    pub invite_token: String,
    pub expires_at: u64,
    /// Space-tree proof-based admission fields, each a JSON object as text;
    /// empty when the inviter didn't include one. Carried to the joiner, who
    /// forwards them on `SfuJoin` for the supernode to verify.
    pub space_root: String,
    pub space_proof: String,
    pub space_grant: String,
}

/// A pasted room invite awaiting its host supernode's WebSocket to connect.
#[derive(Debug, Clone)]
pub(super) struct RoomInviteEntry {
    pub room_id: String,
    pub room_name: String,
    pub room_type: String,
    pub invite_token: String,
    /// Space-tree parent node id (from the invite's inclusion proof) and the
    /// owning Space id (from its signed root). `""` for legacy/flat invites.
    pub parent_id: String,
    pub space_id: String,
}

/// Build a self-contained room invite URL: `doubleslash://room#<base64url(JSON)>`.
///
/// Kept as a free function (separate from the `ConnectionManager` state) so the
/// wire format can be round-trip tested in isolation. See the golden field test
/// in `tests.rs`; any field rename here must update that test in lock-step.
#[allow(clippy::too_many_arguments)]
pub fn build_room_invite_url(
    supernode_id: &str,
    supernode_hint: &str,
    room_id: &str,
    room_name: &str,
    room_type: &str,
    invite_token: &str,
    expires_at: u64,
    space_root: &str,
    space_proof: &str,
    space_grant: &str,
) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mut payload = serde_json::json!({
        "v": ROOM_INVITE_SCHEMA,
        "supernode_id": supernode_id,
        "supernode_hint": supernode_hint,
        "room_id": room_id,
        "room_name": room_name,
        "room_type": room_type,
        "invite_token": invite_token,
        "expires_at": expires_at,
    });
    // Embed the Space fields as nested JSON objects (not strings) when present,
    // so the joiner deserializes them straight into the space types. The owner
    // signatures are over the struct fields, so a JSON round-trip is safe.
    if let Some(obj) = payload.as_object_mut() {
        for (key, text) in [
            ("space_root", space_root),
            ("space_proof", space_proof),
            ("space_grant", space_grant),
        ] {
            if !text.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(text) {
                    obj.insert(key.to_owned(), v);
                }
            }
        }
    }
    let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    doubleslash_features::mint_invite_https("room", &encoded)
}

/// Parse the base64url fragment of a room invite (the part after
/// `room#`). Returns an error string suitable for `emit_invite_failed`.
pub fn parse_room_invite(encoded: &str) -> Result<RoomInvitePayload, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    if encoded.len() > 262_144 {
        return Err(format!("room invite too large ({} bytes)", encoded.len()));
    }
    let json_bytes = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|e| format!("base64 decode error: {e}"))?;
    let payload: serde_json::Value =
        serde_json::from_slice(&json_bytes).map_err(|e| format!("JSON parse error: {e}"))?;

    // Unknown future schema: refuse rather than silently misinterpret.
    if let Some(v) = payload.get("v").and_then(Value::as_u64) {
        if v > ROOM_INVITE_SCHEMA as u64 {
            return Err(format!("unsupported room invite version {v}"));
        }
    }

    let get = |k: &str| {
        payload
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let supernode_id = get("supernode_id");
    let room_id = get("room_id");
    if supernode_id.is_empty() {
        return Err("room invite missing supernode_id".into());
    }
    if room_id.is_empty() {
        return Err("room invite missing room_id".into());
    }
    // `room_type` is additive within v1; invites minted before it existed were
    // always private, so that's the back-compat default.
    let room_type = match payload.get("room_type").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => t.to_owned(),
        _ => "private".to_owned(),
    };
    // Space fields: extract the nested objects back to JSON text ("" = absent).
    let get_obj = |k: &str| {
        payload
            .get(k)
            .filter(|v| v.is_object())
            .map(|v| v.to_string())
            .unwrap_or_default()
    };
    Ok(RoomInvitePayload {
        supernode_id,
        supernode_hint: get("supernode_hint"),
        room_id,
        room_name: get("room_name"),
        room_type,
        invite_token: get("invite_token"),
        expires_at: payload
            .get("expires_at")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        space_root: get_obj("space_root"),
        space_proof: get_obj("space_proof"),
        space_grant: get_obj("space_grant"),
    })
}

impl ConnectionManager {
    pub(in crate::connection_manager) fn generate_invite_url(&mut self) -> Option<String> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        // Best-effort local QUIC listener for LAN dials. Invites still work
        // without it when both peers share a supernode (relayed INIT/ACCEPT).
        let _ = self.ensure_quic_endpoint(0);
        let lan_hint = self.local_quic_hint().unwrap_or_default();

        let invite_id = uuid::Uuid::new_v4().to_string();
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + 900;
        let inviter_handle = super::peer_session::read_local_display_handle();
        // X25519 ephemeral public is required by AcceptInvite (session-key /
        // transcript binding). Peer invites previously omitted it, so every
        // personal invite failed closed with "missing inviter_ephemeral_pub".
        let inviter_eph = crate::crypto::generate_ephemeral_keypair();
        let inviter_ephemeral_pub =
            crate::crypto::b64url_encode_nopad(inviter_eph.public.as_bytes());
        let mut payload = serde_json::json!({
            "inviter_peer_id": self.identity.peer_id(),
            "inviter_identity_pub": self.identity.public_id(),
            "invite_id": invite_id,
            "expires_at": expires_at,
            "inviter_ephemeral_pub": inviter_ephemeral_pub,
            // Peers list label on the joiner before/without a HandleUpdate.
            "inviter_handle": inviter_handle,
        });
        if !lan_hint.is_empty() {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("lan_hint".to_owned(), serde_json::Value::String(lan_hint));
            }
        }
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        // Recorded so the handshake can tell an invite we issued from an id a
        // stranger made up: the INIT is signed by the joiner, which proves who
        // they are, not that we ever invited them.
        self.record_issued_invite(&invite_id, expires_at);
        Some(doubleslash_features::mint_invite_https("invite", &encoded))
    }

    /// Remember that we minted `invite_id`, live until `expires_at` (unix secs).
    ///
    /// In memory only: an invite lives 15 minutes, so one issued before a
    /// restart failing closed afterwards is the price of not persisting bearer
    /// credentials. Bounded, dropping the soonest-expiring first.
    pub(super) fn record_issued_invite(&mut self, invite_id: &str, expires_at: u64) {
        let now = unix_now_secs();
        self.issued_invites.retain(|_, exp| *exp >= now);
        if self.issued_invites.len() >= MAX_ISSUED_INVITES {
            if let Some(soonest) = self
                .issued_invites
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(id, _)| id.clone())
            {
                self.issued_invites.remove(&soonest);
            }
        }
        self.issued_invites.insert(invite_id.to_owned(), expires_at);
    }

    /// Consume `invite_id` if we issued it and it is still live.
    ///
    /// Single use: the first joiner to present an invite is the one it admits.
    /// A second INIT from that same joiner (it sends over QUIC and the relay
    /// both) is admitted as a re-run of a trusted peer instead, not by the id.
    pub(super) fn redeem_issued_invite(&mut self, invite_id: &str) -> bool {
        match self.issued_invites.remove(invite_id) {
            Some(expires_at) => expires_at >= unix_now_secs(),
            None => false,
        }
    }

    /// Tell a joiner we will not complete their handshake.
    pub(super) async fn send_invite_reject(&mut self, joiner_identity_pub: &str, invite_id: &str) {
        let mut reject = SignalingMessage::new(
            MessageType::InviteHandshakeReject,
            self.identity.public_id(),
        );
        reject.target = Some(joiner_identity_pub.to_owned());
        reject
            .payload
            .insert("invite_id".into(), Value::String(invite_id.to_owned()));
        reject.payload.insert(
            "reason".into(),
            Value::String(
                "the inviter does not recognise this invite — it may have expired, \
                 been used already, or been created before they restarted"
                    .into(),
            ),
        );
        self.dispatch_outbound(reject).await;
    }

    /// Joiner side: the inviter refused `invite_id` (or, from a supernode that
    /// names none, whichever invite we hold from it). Marks it; the failure is
    /// reported by [`Self::expire_rejected_invites`] if no accept follows.
    ///
    /// Supernode refusals are only logged: re-accepting a supernode invite the
    /// node has already consumed is routine, and was trusted from the URL.
    pub(super) fn note_invite_rejected(&mut self, sender: &str, invite_id: &str, reason: &str) {
        let sender = sender.trim_end_matches('=');
        let now = Instant::now();
        for pending in self.pending_invites.values_mut() {
            if pending.is_supernode
                || pending.rejected_at.is_some()
                || pending.inviter_identity_pub.trim_end_matches('=') != sender
                || (!invite_id.is_empty() && pending.invite_id != invite_id)
            {
                continue;
            }
            pending.rejected_at = Some(now);
            pending.reject_reason = reason.to_owned();
        }
    }

    /// Fail every refused invite whose grace period passed without an accept.
    pub(super) fn expire_rejected_invites(&mut self) {
        let failed: Vec<(String, String)> = self
            .pending_invites
            .iter()
            .filter(|(_, p)| {
                p.rejected_at
                    .is_some_and(|at| at.elapsed() >= INVITE_REJECT_GRACE)
            })
            .map(|(id, p)| (id.clone(), p.reject_reason.clone()))
            .collect();
        for (invite_id, reason) in failed {
            self.pending_invites.remove(&invite_id);
            self.emit_invite_failed(format!("invite refused: {reason}"));
        }
    }

    /// Build a self-contained room invite URL for a room hosted on
    /// `supernode_id`. Returns `None` if we don't know a signaling address for
    /// that supernode (so we can fall back to sharing the bare token).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn generate_room_invite_url(
        &self,
        supernode_id: &str,
        room_id: &str,
        room_name: &str,
        room_type: &str,
        invite_token: &str,
        space_root: &str,
        space_proof: &str,
        space_grant: &str,
    ) -> Option<String> {
        if supernode_id.is_empty() || room_id.is_empty() {
            return None;
        }
        // Prefer the live session's ws_url; fall back to a persisted relay hint
        // (e.g. the room was created earlier this session but the socket churned).
        let supernode_hint = self
            .supernodes
            .get(supernode_id)
            .map(|sn| sn.ws_url.clone())
            .or_else(|| {
                self.peer_store
                    .read()
                    .get(supernode_id)
                    .and_then(|r| r.relay_hints.first().cloned())
            })
            .filter(|h| !h.is_empty())?;
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + ROOM_INVITE_TTL_SECS;
        Some(build_room_invite_url(
            supernode_id,
            &supernode_hint,
            room_id,
            room_name,
            room_type,
            invite_token,
            expires_at,
            space_root,
            space_proof,
            space_grant,
        ))
    }

    pub(super) fn emit_invite_failed(&self, reason: impl Into<String>) {
        let reason = reason.into();
        warn!("AcceptInvite: {reason}");
        self.emit_event(ConnectionEvent::InviteFailed { reason });
    }

    pub(super) fn build_invite_handshake_init(
        &self,
        pending: &PendingInvite,
        target: String,
    ) -> SignalingMessage {
        let sender = self.identity.public_id();
        let joiner_peer_id = self.identity.peer_id();
        let joiner_eph = crate::crypto::generate_ephemeral_keypair();
        let joiner_ephemeral_pub = crate::crypto::b64url_encode_nopad(joiner_eph.public.as_bytes());
        let joiner_quic_port = self
            .quic_endpoint
            .as_ref()
            .and_then(|ep| ep.local_addr().ok())
            .map(|addr| addr.port())
            .unwrap_or(0);

        let joiner_handle = super::peer_session::read_local_display_handle();
        let mut msg = SignalingMessage::new(MessageType::InviteHandshakeInit, sender.clone());
        msg.target = Some(target);
        msg.payload
            .insert("invite_id".into(), Value::String(pending.invite_id.clone()));
        msg.payload
            .insert("joiner_identity_pub".into(), Value::String(sender.clone()));
        msg.payload
            .insert("joiner_peer_id".into(), Value::String(joiner_peer_id));
        msg.payload.insert(
            "joiner_ephemeral_pub".into(),
            Value::String(joiner_ephemeral_pub),
        );
        msg.payload.insert(
            "joiner_quic_port".into(),
            Value::Number(joiner_quic_port.into()),
        );
        if !joiner_handle.is_empty() {
            msg.payload
                .insert("joiner_handle".into(), Value::String(joiner_handle));
        }
        if let Some(hint) = self.local_quic_hint() {
            msg.payload
                .insert("joiner_lan_hint".into(), Value::String(hint));
        }
        // Sent alongside the LAN hint rather than instead of it: the two are
        // right in different places, and the peer keeps both. Older builds
        // that do not read this field simply fall back to the LAN hint, which
        // is what they did before it existed.
        if let Some(hint) = self.public_quic_hint.clone() {
            msg.payload
                .insert("joiner_public_hint".into(), Value::String(hint));
        }
        msg
    }

    pub(super) async fn send_pending_invite_inits_for_peer(&mut self, peer_id: &str) {
        let invite_ids: Vec<String> = self
            .pending_invites
            .iter()
            .filter(|(_, pending)| !pending.is_supernode && pending.inviter_peer_id == peer_id)
            .map(|(invite_id, _)| invite_id.clone())
            .collect();

        for invite_id in invite_ids {
            let Some(pending) = self.pending_invites.get(&invite_id) else {
                continue;
            };
            let msg = self.build_invite_handshake_init(pending, peer_id.to_owned());
            self.dispatch_outbound(msg).await;
        }
    }

    /// Accept a pasted self-contained room invite: connect to the embedded
    /// host supernode (if not already), then hand the room off to the UI to
    /// join. `encoded` is the base64url fragment after `room#`.
    pub(super) async fn handle_accept_room_invite(&mut self, encoded: &str) {
        let payload = match parse_room_invite(encoded) {
            Ok(p) => p,
            Err(e) => {
                self.emit_invite_failed(e);
                return;
            }
        };

        if payload.expires_at != 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if payload.expires_at < now {
                self.emit_invite_failed("room invite expired");
                return;
            }
        }

        let RoomInvitePayload {
            supernode_id,
            supernode_hint,
            room_id,
            room_name,
            room_type,
            invite_token,
            space_root,
            space_proof,
            space_grant,
            ..
        } = payload;

        // Pull the Space-tree linkage out of the proof/root before they're moved
        // into the pending join creds, so the joiner's sidebar can nest the room:
        // `parent_id` is the room's parent node in the owner's tree (a room id, or
        // "default"/the Server node for a top-level room); `space_id` names the
        // owning Space. Absent for legacy flat invites → "".
        let space_parent_id = serde_json::from_str::<Value>(&space_proof)
            .ok()
            .and_then(|v| {
                v.get("node")
                    .and_then(|n| n.get("parent_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let space_tree_id = serde_json::from_str::<Value>(&space_root)
            .ok()
            .and_then(|v| v.get("space_id").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();

        // Stash any Space proof-based admission creds from the invite; they are
        // attached (single-use) to the SfuJoin for this room so the supernode can
        // admit + materialize it by proof on any cluster member.
        if !space_proof.is_empty() {
            self.pending_join_space_creds
                .insert(room_id.clone(), (space_root, space_proof, space_grant));
        }

        info!(
            "Accepting room invite for room {} on supernode {}",
            &room_id[..12.min(room_id.len())],
            &supernode_id[..8.min(supernode_id.len())]
        );

        // Persist the host supernode so the room-store join path (which resolves
        // the supernode via the peer store) can find it, and so it survives a
        // restart / shows in the Nodes tab. Mirrors the supernode-invite path.
        if !supernode_hint.is_empty() {
            let mut store = self.peer_store.write();
            store.upsert_from_invite(crate::peer_store::PeerRecord {
                peer_id: supernode_id.clone(),
                identity_pub: supernode_id.clone(),
                relay_hints: vec![supernode_hint.clone()],
                is_supernode: true,
                supernode_from_invite: true,
                created_at: unix_now_f64(),
                last_seen_at: unix_now_f64(),
                ..Default::default()
            });
            let _ = store.save();
        }

        let entry = RoomInviteEntry {
            room_id,
            room_name,
            room_type,
            invite_token,
            parent_id: space_parent_id,
            space_id: space_tree_id,
        };

        let connected = self
            .supernodes
            .get(&supernode_id)
            .map(|sn| sn.connected)
            .unwrap_or(false);

        if connected {
            // Link is already up — enter the room immediately.
            self.emit_room_invite_ready(&supernode_id, &entry);
        } else {
            // Stash until WsConnected fires; open the session if we have no
            // task for this supernode yet.
            if !self.supernodes.contains_key(&supernode_id) {
                if supernode_hint.is_empty() {
                    self.emit_invite_failed("room invite missing supernode address");
                    return;
                }
                self.connect_supernode_ws(supernode_id.clone(), vec![supernode_hint.clone()])
                    .await;
            }
            self.pending_room_invite_entries.insert(supernode_id, entry);
        }
    }

    pub(super) fn emit_room_invite_ready(&self, supernode_id: &str, entry: &RoomInviteEntry) {
        self.emit_event(ConnectionEvent::RoomInviteReady {
            supernode_id: supernode_id.to_owned(),
            room_id: entry.room_id.clone(),
            room_name: entry.room_name.clone(),
            room_type: entry.room_type.clone(),
            invite_token: entry.invite_token.clone(),
            parent_id: entry.parent_id.clone(),
            space_id: entry.space_id.clone(),
        });
    }

    pub(in crate::connection_manager) async fn handle_accept_invite(&mut self, invite_url: String) {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let Some(rest) = doubleslash_features::normalize_app_url(&invite_url) else {
            self.emit_invite_failed(format!("invalid scheme in '{invite_url}'"));
            return;
        };
        let rest = rest.as_ref();

        // `normalize_app_url` has reduced every accepted form — the https
        // share link, `d://`, `doubleslash://`, `doubleslash://` — to the same
        // remainder. It carries an optional `action#` prefix before the
        // base64url fragment: `invite#<b64>`, `room#<b64>`, or a bare legacy
        // `<b64>`. Split it off so the payload decodes.
        let (action, encoded) = match rest.split_once('#') {
            Some((action, payload)) => (action, payload),
            None => ("", rest),
        };

        if action == "room" {
            self.handle_accept_room_invite(encoded).await;
            return;
        }

        if encoded.len() > 262_144 {
            self.emit_invite_failed(format!("invite URL too large ({} bytes)", encoded.len()));
            return;
        }

        let json_bytes = match URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')) {
            Ok(b) => b,
            Err(e) => {
                self.emit_invite_failed(format!("base64 decode error: {e}"));
                return;
            }
        };

        let payload: serde_json::Value = match serde_json::from_slice(&json_bytes) {
            Ok(v) => v,
            Err(e) => {
                self.emit_invite_failed(format!("JSON parse error: {e}"));
                return;
            }
        };

        let inviter_identity_pub = match payload.get("inviter_identity_pub").and_then(Value::as_str)
        {
            Some(s) => s.to_owned(),
            None => {
                self.emit_invite_failed("missing inviter_identity_pub");
                return;
            }
        };
        let inviter_peer_id = payload
            .get("inviter_peer_id")
            .and_then(Value::as_str)
            .unwrap_or(&inviter_identity_pub)
            .to_owned();
        let invite_id = payload
            .get("invite_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let relay_hint = payload
            .get("relay_hint")
            .and_then(Value::as_str)
            .map(|s| s.to_owned())
            .unwrap_or_default();
        let lan_hint = payload
            .get("lan_hint")
            .and_then(Value::as_str)
            .map(|s| s.to_owned())
            .unwrap_or_default();
        let supernode_hint = if relay_hint.is_empty() {
            lan_hint.clone()
        } else {
            relay_hint.clone()
        };
        let is_supernode = payload
            .get("is_supernode")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let inviter_ephemeral_pub = payload
            .get("inviter_ephemeral_pub")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        if invite_id.is_empty() {
            self.emit_invite_failed("missing invite_id");
            return;
        }
        if inviter_identity_pub == self.identity.public_id() {
            self.emit_invite_failed("cannot use own invite");
            return;
        }

        if let Some(expires_at) = payload.get("expires_at").and_then(Value::as_i64) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if expires_at < now {
                self.emit_invite_failed("invite expired");
                return;
            }
        }

        let inviter_handle = payload
            .get("inviter_handle")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        info!(
            "Accepting invite from {} (id={})",
            &inviter_identity_pub[..8.min(inviter_identity_pub.len())],
            &invite_id[..8.min(invite_id.len())]
        );

        // Supernode invites: trust + persist immediately from the signed URL
        // payload so the Rooms sidebar updates even if the WS handshake is slow
        // or the supernode no longer has this invite_id in its pending map.
        if is_supernode {
            {
                let mut store = self.peer_store.write();
                let relay_hints = if supernode_hint.is_empty() {
                    vec![]
                } else {
                    vec![supernode_hint.clone()]
                };
                store.upsert_from_invite(crate::peer_store::PeerRecord {
                    peer_id: inviter_peer_id.clone(),
                    identity_pub: inviter_identity_pub.clone(),
                    handle: inviter_handle.clone(),
                    relay_hints,
                    is_supernode: true,
                    supernode_from_invite: true,
                    created_at: unix_now_f64(),
                    last_seen_at: unix_now_f64(),
                    ..Default::default()
                });
                let _ = store.save();
            }
            self.emit_event(ConnectionEvent::InviteAccepted {
                peer_id: inviter_peer_id.clone(),
                handle: inviter_handle.clone(),
            });
        }

        // Store pending invite (matched when INVITE_HANDSHAKE_ACCEPT arrives)
        self.pending_invites.insert(
            invite_id.clone(),
            PendingInvite {
                inviter_peer_id: inviter_peer_id.clone(),
                inviter_identity_pub: inviter_identity_pub.clone(),
                invite_id: invite_id.clone(),
                relay_hint: supernode_hint.clone(),
                lan_hint: lan_hint.clone(),
                is_supernode,
                created_at: Instant::now(),
                rejected_at: None,
                reject_reason: String::new(),
            },
        );

        // Open a signaling session only for supernode invites. Ordinary peers
        // may carry a ws relay hint for NAT traversal — that must not register
        // them in the Rooms sidebar or key a WS session under their identity.
        if is_supernode && !supernode_hint.is_empty() {
            if let Some(sn) = self.supernodes.remove(&inviter_identity_pub) {
                sn.ws_task.abort();
            }
            self.connect_supernode_ws(inviter_identity_pub.clone(), vec![supernode_hint.clone()])
                .await;
        }

        if !is_supernode {
            if inviter_ephemeral_pub.is_empty() {
                self.emit_invite_failed(
                    "invite missing inviter_ephemeral_pub; generate a fresh invite",
                );
                return;
            }

            // Personal peer invites prefer direct QUIC (LAN hint on the invite).
            // When both peers already share a supernode (common for room users),
            // also send INVITE_HANDSHAKE_INIT over the supernode WS relay so trust
            // completes even if LAN QUIC is firewalled, wrong, or missing. The
            // supernode indexes sockets by identity public_id, so the target is
            // inviter_identity_pub — not the hex peer_id used for QUIC sessions.
            let mut attempted = false;
            if let Some((host, port)) = parse_quic_lan_hint(&lan_hint) {
                self.connect_direct_quic(&inviter_peer_id, &host, port)
                    .await;
                attempted = true;
            }

            let supernode_available = self.supernodes.values().any(|sn| sn.connected);
            if supernode_available {
                if let Some(pending) = self.pending_invites.get(&invite_id).cloned() {
                    // Same INIT shape as the post-QUIC path, but targeted at the
                    // inviter's identity public_id so the supernode can relay it.
                    let msg =
                        self.build_invite_handshake_init(&pending, inviter_identity_pub.clone());
                    info!(
                        "Peer invite: sending InviteHandshakeInit via supernode relay to {}",
                        &inviter_identity_pub[..8.min(inviter_identity_pub.len())]
                    );
                    self.dispatch_outbound(msg).await;
                    attempted = true;
                }
            }

            if !attempted {
                self.emit_invite_failed(
                    "invite has no reachable path (no local QUIC hint and no shared supernode online); \
                     generate a fresh invite while both are online on the same supernode, or on the same LAN",
                );
            }
            return;
        }

        // Build + sign INVITE_HANDSHAKE_INIT and queue directly on the WS send
        // channel (the message will be delivered once the WS connection is up).
        let sender = self.identity.public_id();
        let joiner_peer_id = self.identity.peer_id();
        let joiner_eph = crate::crypto::generate_ephemeral_keypair();
        let joiner_ephemeral_pub = crate::crypto::b64url_encode_nopad(joiner_eph.public.as_bytes());
        if inviter_ephemeral_pub.is_empty() {
            self.emit_invite_failed(
                "invite missing inviter_ephemeral_pub; generate a fresh invite",
            );
            return;
        }
        if let Err(e) = crate::crypto::derive_invite_session_key(
            &joiner_eph.secret,
            &inviter_ephemeral_pub,
            &invite_id,
            &inviter_identity_pub,
            &sender,
            &joiner_ephemeral_pub,
        ) {
            warn!("AcceptInvite: session key derivation failed: {e}");
        }
        let joiner_quic_port = self
            .quic_endpoint
            .as_ref()
            .and_then(|ep| ep.local_addr().ok())
            .map(|addr| addr.port())
            .unwrap_or(0);
        let joiner_handle = super::peer_session::read_local_display_handle();
        let mut msg = SignalingMessage::new(MessageType::InviteHandshakeInit, sender.clone());
        msg.target = Some(inviter_identity_pub.clone());
        msg.payload
            .insert("invite_id".into(), Value::String(invite_id));
        msg.payload
            .insert("joiner_identity_pub".into(), Value::String(sender.clone()));
        msg.payload
            .insert("joiner_peer_id".into(), Value::String(joiner_peer_id));
        msg.payload.insert(
            "joiner_ephemeral_pub".into(),
            Value::String(joiner_ephemeral_pub),
        );
        msg.payload.insert(
            "joiner_quic_port".into(),
            Value::Number(joiner_quic_port.into()),
        );
        if !joiner_handle.is_empty() {
            msg.payload
                .insert("joiner_handle".into(), Value::String(joiner_handle));
        }
        if let Some(hint) = self.local_quic_hint() {
            msg.payload
                .insert("joiner_lan_hint".into(), Value::String(hint));
        }
        // Sent alongside the LAN hint rather than instead of it: the two are
        // right in different places, and the peer keeps both. Older builds
        // that do not read this field simply fall back to the LAN hint, which
        // is what they did before it existed.
        if let Some(hint) = self.public_quic_hint.clone() {
            msg.payload
                .insert("joiner_public_hint".into(), Value::String(hint));
        }

        msg.source_device = self.device_id;
        if let Ok(canonical) = msg.canonical_bytes() {
            let sig = self.identity.sign(&canonical);
            use base64::Engine;
            msg.signature = Some(base64::engine::general_purpose::URL_SAFE.encode(sig));
        }

        if let Ok(json) = msg.to_json() {
            if let Some(sn) = self.supernodes.get(&inviter_identity_pub) {
                if sn.send_tx.try_send(WsMessage::Text(json)).is_err() {
                    self.note_ws_outbound_drop("invite handshake accept");
                }
            } else {
                warn!("AcceptInvite: no WS session for inviter — message dropped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The inviter side of the personal-invite handshake.
    //!
    //! A signed INIT proves who the joiner is, never that they were invited.
    //! These pin that only an invite this client issued — once — or a re-run
    //! from a peer already trusted gets anyone into the peer store.

    use super::*;
    use crate::identity::Identity;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

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

    fn public_id(c: &Client) -> String {
        c.manager.identity.public_id()
    }

    /// Everything `source` queued, as the supernode would relay it.
    fn drain(source: &mut Client) -> Vec<SignalingMessage> {
        let mut out = Vec::new();
        while let Ok(Message::Text(raw)) = source.outgoing.try_recv() {
            out.push(SignalingMessage::from_json(&raw).unwrap());
        }
        out
    }

    async fn deliver(messages: Vec<SignalingMessage>, target: &mut Client) {
        for m in messages {
            target
                .manager
                .handle_inbound_from_supernode(HOST.to_owned(), m)
                .await;
        }
    }

    fn knows(c: &Client, id: &str) -> bool {
        c.manager.peer_store.read().find_identity(id).is_some()
    }

    fn invite_failures(c: &mut Client) -> usize {
        let mut n = 0;
        while let Ok(e) = c.events.try_recv() {
            if matches!(e, ConnectionEvent::InviteFailed { .. }) {
                n += 1;
            }
        }
        n
    }

    /// A well-formed invite in `inviter`'s name that `inviter` never minted.
    fn forged_invite(inviter: &Client) -> String {
        let eph = crate::crypto::generate_ephemeral_keypair();
        let payload = serde_json::json!({
            "inviter_peer_id": inviter.manager.identity.peer_id(),
            "inviter_identity_pub": public_id(inviter),
            "invite_id": "made-up-by-the-joiner",
            "expires_at": unix_now_secs() + 900,
            "inviter_ephemeral_pub": crate::crypto::b64url_encode_nopad(eph.public.as_bytes()),
        });
        doubleslash_features::mint_invite_https(
            "invite",
            &URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes()),
        )
    }

    /// Run a whole handshake for `url` from `joiner` to `inviter`.
    async fn handshake(joiner: &mut Client, inviter: &mut Client, url: &str) {
        joiner.manager.handle_accept_invite(url.to_owned()).await;
        let init = drain(joiner);
        deliver(init, inviter).await;
        let reply = drain(inviter);
        deliver(reply, joiner).await;
    }

    #[tokio::test]
    async fn an_issued_invite_admits_one_joiner_and_only_once() {
        let mut inviter = client();
        let mut first = client();
        let mut second = client();
        let url = inviter.manager.generate_invite_url().unwrap();

        handshake(&mut first, &mut inviter, &url).await;
        assert!(
            knows(&inviter, &public_id(&first)),
            "the invited joiner is admitted"
        );

        handshake(&mut second, &mut inviter, &url).await;
        assert!(
            !knows(&inviter, &public_id(&second)),
            "a used invite admits nobody else"
        );
    }

    #[tokio::test]
    async fn an_invite_we_never_issued_is_refused_and_the_joiner_told() {
        let mut inviter = client();
        let mut joiner = client();
        let url = forged_invite(&inviter);

        joiner.manager.handle_accept_invite(url).await;
        deliver(drain(&mut joiner), &mut inviter).await;
        assert!(!knows(&inviter, &public_id(&joiner)), "nothing is trusted");

        let reply = drain(&mut inviter);
        assert!(!reply.is_empty());
        assert!(
            reply
                .iter()
                .all(|m| m.msg_type == MessageType::InviteHandshakeReject),
            "a refusal and nothing else goes back"
        );
        deliver(reply, &mut joiner).await;

        // Provisional until the grace passes without an accept.
        joiner.manager.expire_rejected_invites();
        assert_eq!(invite_failures(&mut joiner), 0);
        for p in joiner.manager.pending_invites.values_mut() {
            assert!(p.rejected_at.is_some(), "the refusal was recorded");
            p.rejected_at = Some(Instant::now() - INVITE_REJECT_GRACE);
        }
        joiner.manager.expire_rejected_invites();
        assert_eq!(invite_failures(&mut joiner), 1);
        assert!(joiner.manager.pending_invites.is_empty());
    }

    #[tokio::test]
    async fn a_trusted_peer_re_running_the_handshake_is_admitted() {
        let mut inviter = client();
        let mut joiner = client();
        let url = inviter.manager.generate_invite_url().unwrap();
        handshake(&mut joiner, &mut inviter, &url).await;
        assert!(knows(&inviter, &public_id(&joiner)));

        // Same, now-spent invite again: admitted as a known peer's re-run.
        joiner.manager.handle_accept_invite(url).await;
        deliver(drain(&mut joiner), &mut inviter).await;
        let reply = drain(&mut inviter);
        assert!(!reply.is_empty(), "the inviter answers");
        assert!(
            reply
                .iter()
                .all(|m| m.msg_type != MessageType::InviteHandshakeReject),
            "a trusted peer is not refused"
        );
    }

    #[tokio::test]
    async fn a_joiner_cannot_claim_another_peers_store_key() {
        let mut inviter = client();
        let joiner = client();
        let victim = crate::peer_store::PeerRecord {
            peer_id: "victim-key".into(),
            identity_pub: "victim-identity".into(),
            handle: "victim".into(),
            ..Default::default()
        };
        inviter.manager.peer_store.write().upsert(victim);
        let url = inviter.manager.generate_invite_url().unwrap();
        let rest = doubleslash_features::normalize_app_url(&url).unwrap();
        let encoded = rest.strip_prefix("invite#").unwrap().to_owned();
        let payload: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();

        let mut init = SignalingMessage::new(MessageType::InviteHandshakeInit, public_id(&joiner));
        init.target = Some(public_id(&inviter));
        init.payload
            .insert("invite_id".into(), payload["invite_id"].clone());
        init.payload
            .insert("joiner_peer_id".into(), Value::String("victim-key".into()));
        init.payload
            .insert("joiner_handle".into(), Value::String("mallory".into()));
        let sig = joiner
            .manager
            .identity
            .sign(&init.canonical_bytes().unwrap());
        init.signature = Some(base64::engine::general_purpose::URL_SAFE.encode(sig));
        inviter
            .manager
            .handle_inbound_from_supernode(HOST.to_owned(), init)
            .await;

        let store = inviter.manager.peer_store.read();
        let rec = store.get("victim-key").unwrap();
        assert_eq!(rec.handle, "victim", "the victim's record is untouched");
        assert_eq!(rec.identity_pub, "victim-identity");
        assert!(store.find_identity(&public_id(&joiner)).is_none());
    }

    #[tokio::test]
    async fn a_siblings_refusal_does_not_undo_the_minting_devices_accept() {
        let mut inviter = client();
        let mut joiner = client();
        let url = inviter.manager.generate_invite_url().unwrap();

        joiner.manager.handle_accept_invite(url).await;
        // Another device signed in as the inviter never issued it, and says so
        // first.
        let inviter_pub = public_id(&inviter);
        joiner
            .manager
            .note_invite_rejected(&inviter_pub, "", "not recognised");
        deliver(drain(&mut joiner), &mut inviter).await;
        deliver(drain(&mut inviter), &mut joiner).await;

        assert!(knows(&joiner, &inviter_pub), "the accept still completes");
        joiner.manager.expire_rejected_invites();
        assert_eq!(invite_failures(&mut joiner), 0);
    }

    #[tokio::test]
    async fn only_the_inviter_can_refuse_its_invite() {
        let inviter = client();
        let mut joiner = client();
        let url = forged_invite(&inviter);
        joiner.manager.handle_accept_invite(url).await;
        assert!(!joiner.manager.pending_invites.is_empty());

        joiner
            .manager
            .note_invite_rejected("someone-else", "", "go away");
        assert!(joiner
            .manager
            .pending_invites
            .values()
            .all(|p| p.rejected_at.is_none()));
    }
}
