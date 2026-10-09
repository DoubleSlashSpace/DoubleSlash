//! Coordination between root-authorized devices sharing a room identity.
//! Key material travels only inside an EncryptedSignal to the same root.

use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

use doubleslash_features::device::{DeviceId, MAX_LIVE_DEVICE_ROUTES};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{room_session::accept_group_key_epoch, ConnectionManager};
use crate::group_key::GroupKeySource;
use crate::protocol::{MessageType, SignalingMessage};

#[derive(Clone, Deserialize, serde::Serialize)]
pub(super) struct RoomEndpoint {
    identity: String,
    device: Option<DeviceId>,
}

/// Least time between two own-room snapshots. A burst of supernode connects
/// (one per cluster member) then sends two, the second once every member is
/// reachable, instead of one per connect.
const OWN_ROOM_SYNC_SPACING: Duration = Duration::from_secs(2);
/// Largest snapshot sent. Sealed and base64-encoded it stays under the
/// supernode's 256 KiB frame limit.
const OWN_ROOM_SYNC_MAX_BYTES: usize = 160 * 1024;

/// Outbound own-device room sync. See [`ConnectionManager::queue_own_room_sync`].
#[derive(Default)]
pub(super) struct OwnRoomSync {
    last_sent: Option<Instant>,
    pending: Option<(crate::room_store::OwnRoomSnapshot, bool)>,
}

pub(super) struct OwnKeyRound {
    members: BTreeSet<DeviceId>,
    heard: HashSet<DeviceId>,
    fingerprint: String,
    challenge: [u8; 32],
    last_request: Option<Instant>,
    conflict: bool,
}

impl ConnectionManager {
    pub(super) fn forget_room_device_scope(&mut self, host: &str, room: &str) {
        self.room_device_rosters.remove(&format!("{host}:{room}"));
        self.own_room_key_rounds.remove(room);
        let suffix = format!(":{room}");
        let remaining = self
            .room_device_rosters
            .iter()
            .find(|(scope, _)| scope.ends_with(&suffix))
            .map(|(scope, roster)| {
                (
                    scope[..scope.len() - suffix.len()].to_owned(),
                    roster.clone(),
                )
            });
        if let Some((host, roster)) = remaining {
            if let Ok(value) = serde_json::to_value(roster) {
                self.record_room_devices(&host, room, Some(&value));
            }
        }
        self.clear_outdated_warning_if_unrostered(room);
    }

    pub(super) fn forget_host_device_rosters(&mut self, host: &str) {
        let prefix = format!("{host}:");
        let rooms: Vec<_> = self
            .room_device_rosters
            .keys()
            .filter_map(|scope| scope.strip_prefix(&prefix).map(str::to_owned))
            .collect();
        for room in rooms {
            self.forget_room_device_scope(host, &room);
        }
    }

    /// Lift the outdated-own-device warning for `room` and tell the UI.
    fn clear_outdated_own_device(&mut self, room: &str) {
        if self.outdated_own_device_rooms.remove(room) {
            self.emit_event(
                crate::connection_manager::ConnectionEvent::OwnDeviceOutdated {
                    room_id: room.to_owned(),
                    outdated: false,
                },
            );
        }
    }

    /// Once no roster for `room` is held (left, or every host dropped), its
    /// warning is stale. Lifting it means rejoining while the old device is
    /// still signed in warns again instead of leaving chat silently dead.
    fn clear_outdated_warning_if_unrostered(&mut self, room: &str) {
        let suffix = format!(":{room}");
        if !self
            .room_device_rosters
            .keys()
            .any(|scope| scope.ends_with(&suffix))
        {
            self.clear_outdated_own_device(room);
        }
    }

    pub(super) fn room_devices(&self, room_id: &str, identity: &str) -> BTreeSet<Option<DeviceId>> {
        let suffix = format!(":{room_id}");
        self.room_device_rosters
            .iter()
            .filter(|(scope, _)| scope.ends_with(&suffix))
            .flat_map(|(_, roster)| roster.iter())
            .filter(|endpoint| {
                endpoint.identity.trim_end_matches('=') == identity.trim_end_matches('=')
            })
            .map(|endpoint| endpoint.device)
            .collect()
    }

    pub(super) fn record_room_devices(
        &mut self,
        supernode: &str,
        room: &str,
        value: Option<&Value>,
    ) -> bool {
        let Some(me) = self.device_id else {
            return true;
        };
        let scope = format!("{supernode}:{room}");
        let roster = value
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<RoomEndpoint>>(value).ok());
        let Some(roster) = roster.filter(|roster| roster.len() <= 1024) else {
            tracing::debug!("[own-room-key] {room}: no usable device roster from {supernode}");
            self.room_device_rosters.remove(&scope);
            self.own_room_key_rounds.remove(room);
            self.clear_outdated_warning_if_unrostered(room);
            return false;
        };
        if !roster.iter().any(|entry| {
            entry.identity.trim_end_matches('=') == self.identity.public_id().trim_end_matches('=')
                && entry.device == Some(me)
        }) {
            tracing::debug!(
                "[own-room-key] {room}: roster from {supernode} does not list this device"
            );
            self.room_device_rosters.remove(&scope);
            self.own_room_key_rounds.remove(room);
            self.clear_outdated_warning_if_unrostered(room);
            return false;
        }
        self.room_device_rosters.insert(scope, roster);
        let devices = self.room_devices(room, &self.identity.public_id());
        if devices.contains(&None) {
            // Another device signed in as us runs a build without device
            // routing. Keys cannot be coordinated with it, so the round is
            // abandoned and nobody in the room gets a key: say so once per room
            // rather than leaving room chat silently dead.
            if self.outdated_own_device_rooms.insert(room.to_owned()) {
                tracing::warn!(
                    "[own-room-key] {room}: another device on this identity lacks device routing; room keys paused until it is updated"
                );
                self.emit_event(
                    crate::connection_manager::ConnectionEvent::OwnDeviceOutdated {
                        room_id: room.to_owned(),
                        outdated: true,
                    },
                );
            }
            self.own_room_key_rounds.remove(room);
            return false;
        }
        self.clear_outdated_own_device(room);
        if devices.len() > MAX_LIVE_DEVICE_ROUTES {
            tracing::debug!(
                "[own-room-key] {room}: no round ({} devices exceeds the live route limit)",
                devices.len()
            );
            self.own_room_key_rounds.remove(room);
            return false;
        }
        let members: BTreeSet<_> = devices.into_iter().flatten().collect();
        if self
            .own_room_key_rounds
            .get(room)
            .is_some_and(|round| round.members == members)
        {
            return true;
        }
        let mut hash = Sha256::new();
        hash.update(b"doubleslash/own-room-key-roster/v1\0");
        hash.update(room.as_bytes());
        for device in &members {
            hash.update(device.0);
        }
        let mut challenge = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut challenge);
        tracing::debug!(
            "[own-room-key] {room}: new round over {} device(s)",
            members.len()
        );
        self.own_room_key_rounds.insert(
            room.to_owned(),
            OwnKeyRound {
                members,
                heard: HashSet::new(),
                fingerprint: hex::encode(hash.finalize()),
                challenge,
                last_request: None,
                conflict: false,
            },
        );
        true
    }

    pub(super) fn own_room_key_ready(&self, room: &str) -> bool {
        let Some(me) = self.device_id else {
            return true;
        };
        self.own_room_key_rounds.get(room).is_some_and(|round| {
            !round.conflict
                && round
                    .members
                    .iter()
                    .all(|device| *device == me || round.heard.contains(device))
        })
    }

    pub(super) fn elected_room_device(
        &self,
        room: &str,
        identity: &str,
        device: Option<DeviceId>,
    ) -> bool {
        if self.device_id.is_none() {
            return device.is_none();
        }
        let devices = self.room_devices(room, identity);
        devices.first().is_some_and(|first| *first == device)
    }

    pub(super) async fn retry_own_room_key_sync(&mut self) {
        let rooms: Vec<_> = self.own_room_key_rounds.keys().cloned().collect();
        for room in rooms {
            self.request_own_room_key(&room).await;
        }
    }

    pub(super) async fn request_own_room_key(&mut self, room: &str) {
        let Some(me) = self.device_id else {
            return;
        };
        let Some(round) = self.own_room_key_rounds.get_mut(room) else {
            return;
        };
        if round.conflict
            || round
                .last_request
                .is_some_and(|last| last.elapsed() < Duration::from_secs(3))
        {
            return;
        }
        // Ask only siblings that have not answered this round. Once every one
        // has, the round is settled and there is nothing to ask; a changed
        // roster starts a new round, which asks again. Re-asking a settled
        // round on every tick was constant traffic between a user's devices.
        let targets: Vec<_> = round
            .members
            .iter()
            .copied()
            .filter(|device| *device != me && !round.heard.contains(device))
            .collect();
        if targets.is_empty() {
            return;
        }
        round.last_request = Some(Instant::now());
        let payload = json!({"room_id": room, "roster": round.fingerprint,
            "challenge": round.challenge, "request": true});
        tracing::debug!(
            "[own-room-key] {room}: requesting key from {} sibling device(s)",
            targets.len()
        );
        for device in targets {
            self.send_own_room_key_frame(device, payload.clone()).await;
        }
    }

    async fn send_own_room_key_frame(&mut self, device: DeviceId, payload: Value) {
        let mut inner =
            SignalingMessage::new(MessageType::SfuDeviceKeySync, self.identity.public_id());
        inner.target = Some(self.identity.public_id());
        inner.target_device = Some(device);
        let Some(payload) = payload.as_object() else {
            return;
        };
        inner.payload = payload.clone().into_iter().collect();
        let Some(json) = self.sign_message_json(&mut inner) else {
            return;
        };
        if !self.feature_registry.gate_through_feature(
            "room.chat.v1",
            &self.identity.public_id(),
            json.len(),
        ) {
            return;
        }
        if let Some(envelope) = self.seal_signal_to_member(&inner, &self.identity.public_id()) {
            self.dispatch_outbound(envelope).await;
        }
    }

    /// Queue this device's rooms for our other devices and send if allowed.
    ///
    /// Not charged to a feature quota: a snapshot can be larger than a
    /// one-second quota bucket holds, so it would never pass. The spacing
    /// bounds the rate instead, and nothing is dropped: a newer snapshot
    /// replaces the waiting one, and the retry tick sends it.
    pub(super) async fn queue_own_room_sync(
        &mut self,
        snapshot: crate::room_store::OwnRoomSnapshot,
        reply_wanted: bool,
    ) {
        // Without device routing a sibling cannot be told apart from us.
        if self.device_id.is_none() {
            return;
        }
        let reply_wanted = reply_wanted
            || self
                .own_room_sync
                .pending
                .as_ref()
                .is_some_and(|(_, reply)| *reply);
        self.own_room_sync.pending = Some((snapshot, reply_wanted));
        self.flush_own_room_sync().await;
    }

    /// Send the waiting own-room snapshot once spacing and a supernode allow.
    pub(super) async fn flush_own_room_sync(&mut self) {
        if self.own_room_sync.pending.is_none()
            || self
                .own_room_sync
                .last_sent
                .is_some_and(|last| last.elapsed() < OWN_ROOM_SYNC_SPACING)
            || !self.supernodes.values().any(|sn| sn.connected)
        {
            return;
        }
        let Some((snapshot, reply_wanted)) = self.own_room_sync.pending.take() else {
            return;
        };
        let value = match serde_json::to_value(&snapshot) {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!("[own-rooms] could not encode the room snapshot: {e}");
                return;
            }
        };
        let size = value.to_string().len();
        if size > OWN_ROOM_SYNC_MAX_BYTES {
            tracing::warn!(
                "[own-rooms] room snapshot is {size} bytes, over {OWN_ROOM_SYNC_MAX_BYTES}; not sent"
            );
            return;
        }
        let me = self.identity.public_id();
        let mut inner = SignalingMessage::new(MessageType::DeviceRoomSync, me.clone());
        // No target device: the supernode delivers to every endpoint of our
        // identity, this one included, and each ignores its own.
        inner.target = Some(me.clone());
        inner.payload.insert("snapshot".to_owned(), value);
        inner
            .payload
            .insert("reply".to_owned(), Value::Bool(reply_wanted));
        if self.sign_message_json(&mut inner).is_none() {
            return;
        }
        let Some(envelope) = self.seal_signal_to_member(&inner, &me) else {
            return;
        };
        self.own_room_sync.last_sent = Some(Instant::now());
        tracing::debug!(
            "[own-rooms] sending {} room(s), {} space(s) to our other devices (reply={reply_wanted})",
            snapshot.rooms.len(),
            snapshot.spaces.len()
        );
        self.dispatch_outbound(envelope).await;
    }

    /// Hand a sibling device's room snapshot to the app layer to merge.
    ///
    /// Accepted only sealed (checked by the caller), signed by our own
    /// identity, and from another device of it.
    pub(super) fn handle_own_room_sync(&mut self, message: &SignalingMessage) {
        let Some(me) = self.device_id else {
            return;
        };
        if message.source_device.is_none()
            || message.source_device == Some(me)
            || message.target_device.is_some_and(|device| device != me)
            || message.sender.trim_end_matches('=')
                != self.identity.public_id().trim_end_matches('=')
        {
            return;
        }
        let Some(snapshot) = message
            .payload
            .get("snapshot")
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<crate::room_store::OwnRoomSnapshot>(value).ok()
            })
            .filter(|snapshot| snapshot.within_limits())
        else {
            tracing::debug!("[own-rooms] dropping a malformed room snapshot from a sibling");
            return;
        };
        let reply_wanted = message
            .payload
            .get("reply")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        tracing::debug!(
            "[own-rooms] {} room(s), {} space(s) from a sibling device (reply={reply_wanted})",
            snapshot.rooms.len(),
            snapshot.spaces.len()
        );
        self.emit_event(
            crate::connection_manager::ConnectionEvent::OwnRoomsReceived {
                snapshot,
                reply_wanted,
            },
        );
    }

    pub(super) async fn handle_own_room_key_sync(&mut self, message: &SignalingMessage) {
        let Some(me) = self.device_id else {
            return;
        };
        let Some(sender) = message.source_device.filter(|device| *device != me) else {
            return;
        };
        if message.sender.trim_end_matches('=') != self.identity.public_id().trim_end_matches('=')
            || message.target_device != Some(me)
        {
            return;
        }
        let Some(room) = message.payload.get("room_id").and_then(Value::as_str) else {
            return;
        };
        let Some(round) = self.own_room_key_rounds.get(room) else {
            tracing::debug!("[own-room-key] {room}: sync from a sibling but no round here");
            return;
        };
        let in_round = round.members.contains(&sender);
        let roster_match = message.payload.get("roster").and_then(Value::as_str)
            == Some(round.fingerprint.as_str());
        if !in_round || !roster_match {
            tracing::debug!(
                "[own-room-key] {room}: dropping sibling sync (in_round={in_round}, roster_match={roster_match})"
            );
            return;
        }
        let Some(challenge) = message
            .payload
            .get("challenge")
            .cloned()
            .and_then(|value| serde_json::from_value::<[u8; 32]>(value).ok())
        else {
            return;
        };
        if message.payload.get("request").and_then(Value::as_bool) == Some(true) {
            let epoch = self.group_keys.current_epoch(room);
            let key = self
                .group_keys
                .has_real_key(room)
                .then(|| self.group_keys.epoch_key(room, epoch))
                .flatten();
            let payload = json!({"room_id": room, "roster": round.fingerprint, "challenge": challenge,
                "request": false, "epoch": epoch, "key": key.map(|key| crate::crypto::b64url_encode(&key))});
            self.send_own_room_key_frame(sender, payload).await;
            tracing::debug!("[own-room-key] {room}: answered a sibling key request");
            return;
        }
        if challenge != round.challenge
            || message.payload.get("request").and_then(Value::as_bool) != Some(false)
        {
            tracing::debug!("[own-room-key] {room}: dropping sibling reply with a stale challenge");
            return;
        }
        let Some(epoch) = message
            .payload
            .get("epoch")
            .and_then(Value::as_u64)
            .and_then(|epoch| u8::try_from(epoch).ok())
        else {
            return;
        };
        let Some(key_value) = message.payload.get("key") else {
            return;
        };
        if !key_value.is_null() && !key_value.is_string() {
            return;
        }
        if let Some(encoded) = key_value.as_str() {
            let Some(key) = crate::crypto::b64url_decode(encoded)
                .ok()
                .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            else {
                return;
            };
            let has_key = self.group_keys.has_real_key(room);
            let current = self.group_keys.current_epoch(room);
            if has_key && current == epoch && self.group_keys.epoch_key(room, epoch) != Some(key) {
                if let Some(round) = self.own_room_key_rounds.get_mut(room) {
                    round.conflict = true;
                    tracing::debug!(
                        "[own-room-key] {room}: sibling holds a different key at epoch {epoch}"
                    );
                }
                return;
            }
            if !has_key || (epoch != current && accept_group_key_epoch(true, current, epoch)) {
                self.group_keys.install(room, epoch, key);
            }
        }
        if let Some(round) = self.own_room_key_rounds.get_mut(room) {
            round.heard.insert(sender);
            tracing::debug!(
                "[own-room-key] {room}: heard sibling ({} of {} siblings)",
                round.heard.len(),
                round.members.len().saturating_sub(1)
            );
        }
        // Reconcile after handoff without inventing a new membership snapshot.
        let suffix = format!(":{room}");
        let snapshot = self
            .room_group_members
            .iter()
            .find(|(scope, _)| scope.ends_with(&suffix))
            .map(|(scope, members)| {
                (
                    scope[..scope.len() - suffix.len()].to_owned(),
                    members.iter().cloned().collect::<Vec<_>>(),
                )
            });
        if let Some((host, mut members)) = snapshot {
            members.push(self.identity.public_id());
            self.sync_room_membership(&host, room, &members).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    struct Client {
        manager: ConnectionManager,
        outgoing: mpsc::Receiver<Message>,
        events: mpsc::Receiver<crate::connection_manager::ConnectionEvent>,
        _profile: tempfile::TempDir,
    }

    fn client(identity: Arc<crate::identity::Identity>, device: u8) -> Client {
        let profile = tempfile::tempdir().unwrap();
        let store =
            crate::peer_store::PeerStore::open(&identity, Some(&profile.path().join("peers.dat")))
                .unwrap();
        let (mut manager, events) =
            ConnectionManager::new_for_test(identity, Arc::new(RwLock::new(store)));
        manager.device_id = Some(DeviceId([device; 32]));
        let outgoing = manager.test_add_supernode_session("host");
        Client {
            manager,
            outgoing,
            events,
            _profile: profile,
        }
    }

    async fn forward(source: &mut Client, target: &mut Client) -> bool {
        let mut changed = false;
        while let Ok(Message::Text(raw)) = source.outgoing.try_recv() {
            changed = true;
            let message = SignalingMessage::from_json(&raw).unwrap();
            assert_eq!(message.msg_type, MessageType::EncryptedSignal);
            assert!(!raw.contains(&crate::crypto::b64url_encode(&[55; 32])));
            if message.target_device.is_none() || message.target_device == source.manager.device_id
            {
                source
                    .manager
                    .handle_inbound_from_supernode("host".into(), message.clone())
                    .await;
            }
            if message.target_device.is_none() || message.target_device == target.manager.device_id
            {
                target
                    .manager
                    .handle_inbound_from_supernode("host".into(), message)
                    .await;
            }
        }
        changed
    }

    #[tokio::test]
    async fn self_chat_is_sealed_deduplicated_and_acked_only_to_the_sending_device() {
        use crate::connection_manager::ConnectionEvent;
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        let mut message = SignalingMessage::new(MessageType::ChatMessage, identity.public_id());
        message.target = Some(identity.peer_id());
        message
            .payload
            .insert("body".into(), json!("private self-chat acceptance"));
        message
            .payload
            .insert("message_id".into(), json!("self-chat-1"));
        assert!(phone.manager.dispatch_outbound(message).await);
        let Message::Text(raw) = phone.outgoing.try_recv().unwrap() else {
            panic!("expected text envelope")
        };
        assert!(!raw.contains("private self-chat acceptance"));
        let sealed = SignalingMessage::from_json(&raw).unwrap();
        assert_eq!(sealed.msg_type, MessageType::EncryptedSignal);
        for receiver in [&mut phone, &mut desktop] {
            for _ in 0..2 {
                receiver
                    .manager
                    .handle_inbound_from_supernode("host".into(), sealed.clone())
                    .await;
            }
        }
        assert!(phone.events.try_recv().is_err());
        assert!(
            matches!(desktop.events.try_recv().unwrap(), ConnectionEvent::ChatMessage { peer_id, message_id, .. }
            if peer_id == identity.peer_id() && message_id == "self-chat-1")
        );
        assert!(desktop.events.try_recv().is_err());
        let Message::Text(ack_raw) = desktop.outgoing.try_recv().unwrap() else {
            panic!("expected ack envelope")
        };
        let ack = SignalingMessage::from_json(&ack_raw).unwrap();
        assert_eq!(ack.msg_type, MessageType::EncryptedSignal);
        assert_eq!(ack.target_device, phone.manager.device_id);
        phone
            .manager
            .handle_inbound_from_supernode("host".into(), ack)
            .await;
        assert!(
            matches!(phone.events.try_recv().unwrap(), ConnectionEvent::ChatAck { message_id, .. }
            if message_id == "self-chat-1")
        );
    }

    #[tokio::test]
    async fn self_chat_rejects_cleartext_foreign_missing_device_and_wrong_target() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        let mut stranger = client(Arc::new(crate::identity::Identity::generate()), 3);
        for case in 0..5 {
            let sender = if case == 1 { &mut stranger } else { &mut phone };
            let mut message = SignalingMessage::new(
                MessageType::ChatMessage,
                sender.manager.identity.public_id(),
            );
            message.target = Some(if case == 3 {
                "another-recipient".into()
            } else {
                identity.public_id()
            });
            message
                .payload
                .insert("body".into(), json!("must be rejected"));
            message
                .payload
                .insert("message_id".into(), json!(format!("bad-{case}")));
            if case == 2 {
                sender.manager.device_id = None;
            }
            if case == 4 {
                sender.manager.device_id = desktop.manager.device_id;
            }
            sender.manager.sign_message_json(&mut message).unwrap();
            let inbound = if case == 0 {
                message
            } else {
                sender
                    .manager
                    .seal_signal_to_member(&message, &identity.public_id())
                    .unwrap()
            };
            desktop
                .manager
                .handle_inbound_from_supernode("host".into(), inbound)
                .await;
            assert!(desktop.events.try_recv().is_err(), "case {case}");
            assert!(desktop.outgoing.try_recv().is_err(), "case {case}");
            phone.manager.device_id = Some(DeviceId([1; 32]));
        }
    }

    async fn settle(a: &mut Client, b: &mut Client) {
        for _ in 0..32 {
            let changed_a = forward(a, b).await;
            let changed_b = forward(b, a).await;
            if !changed_a && !changed_b {
                return;
            }
        }
        panic!("device room-key handoff did not settle");
    }

    async fn begin(a: &mut Client, b: &mut Client) {
        let identity = a.manager.identity.public_id();
        let roster = json!([
            {"identity": identity, "device": a.manager.device_id, "voice": false},
            {"identity": identity, "device": b.manager.device_id, "voice": false},
        ]);
        for client in [a, b] {
            assert!(client
                .manager
                .record_room_devices("host", "room", Some(&roster)));
            client
                .manager
                .sync_room_membership("host", "room", std::slice::from_ref(&identity))
                .await;
            assert!(!client.manager.own_room_key_ready("room"));
        }
    }

    fn call_clients() -> (Client, Client, Client) {
        let caller = Arc::new(crate::identity::Identity::generate());
        let receiver = Arc::new(crate::identity::Identity::generate());
        let clients = (
            client(caller.clone(), 9),
            client(receiver.clone(), 1),
            client(receiver.clone(), 2),
        );
        for (local, remote) in [
            (&clients.0, &receiver),
            (&clients.1, &caller),
            (&clients.2, &caller),
        ] {
            local
                .manager
                .peer_store
                .write()
                .upsert(crate::peer_store::PeerRecord {
                    peer_id: remote.peer_id(),
                    identity_pub: remote.public_id(),
                    ..Default::default()
                });
        }
        clients
    }

    async fn send_call(client: &mut Client, kind: MessageType, remote: &str) {
        let mut message = SignalingMessage::new(kind, client.manager.identity.public_id());
        message.target = Some(remote.into());
        assert!(client.manager.dispatch_outbound(message).await);
    }

    async fn deliver_calls(source: &mut Client, targets: &mut [&mut Client]) {
        while let Ok(Message::Text(raw)) = source.outgoing.try_recv() {
            let message = SignalingMessage::from_json(&raw).unwrap();
            assert_eq!(message.msg_type, MessageType::EncryptedSignal);
            for target in targets.iter_mut() {
                if message.target_device.is_none()
                    || message.target_device == target.manager.device_id
                {
                    target
                        .manager
                        .handle_inbound_from_supernode("host".into(), message.clone())
                        .await;
                }
            }
        }
    }

    #[tokio::test]
    async fn competing_phone_and_desktop_answers_select_only_first_endpoint() {
        use crate::connection_manager::ConnectionEvent;
        let (mut caller, mut phone, mut desktop) = call_clients();
        let caller_id = caller.manager.identity.peer_id();
        let receiver_id = phone.manager.identity.peer_id();
        send_call(&mut caller, MessageType::CallRequest, &receiver_id).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        send_call(&mut phone, MessageType::CallAccept, &caller_id).await;
        send_call(&mut desktop, MessageType::CallAccept, &caller_id).await;
        assert!(!phone
            .manager
            .device_call_accepts_media(&caller_id, caller.manager.device_id));
        deliver_calls(&mut phone, &mut [&mut caller]).await;
        deliver_calls(&mut desktop, &mut [&mut caller]).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        deliver_calls(&mut phone, &mut [&mut caller]).await;
        assert!(caller
            .manager
            .device_call_accepts_media(&receiver_id, phone.manager.device_id));
        assert!(!caller
            .manager
            .device_call_accepts_media(&receiver_id, desktop.manager.device_id));
        assert!(phone
            .manager
            .device_call_accepts_media(&caller_id, caller.manager.device_id));
        assert!(!desktop
            .manager
            .device_call_accepts_media(&caller_id, caller.manager.device_id));
        let mut accepts = 0;
        while let Ok(event) = caller.events.try_recv() {
            match event {
                ConnectionEvent::CallAccepted { .. } => accepts += 1,
                ConnectionEvent::CallEnded { .. } => panic!("losing answer ended winning call"),
                _ => {}
            }
        }
        assert_eq!(accepts, 1);
        let mut stopped = false;
        while let Ok(event) = desktop.events.try_recv() {
            match event {
                ConnectionEvent::CallAnsweredElsewhere { .. } => stopped = true,
                ConnectionEvent::CallEnded { .. } => {
                    panic!("an answer on the phone is not a missed call")
                }
                _ => {}
            }
        }
        assert!(stopped);
    }

    /// The ordinary case: one device answers and the other never touches the
    /// call. The untouched device must stop ringing, not ring until timeout.
    #[tokio::test]
    async fn answering_on_one_device_stops_the_other_ringing() {
        use crate::connection_manager::ConnectionEvent;
        let (mut caller, mut phone, mut desktop) = call_clients();
        let caller_id = caller.manager.identity.peer_id();
        let receiver_id = phone.manager.identity.peer_id();
        send_call(&mut caller, MessageType::CallRequest, &receiver_id).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        while desktop.events.try_recv().is_ok() {}

        send_call(&mut phone, MessageType::CallAccept, &caller_id).await;
        deliver_calls(&mut phone, &mut [&mut caller]).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        deliver_calls(&mut phone, &mut [&mut caller]).await;

        assert!(caller
            .manager
            .device_call_accepts_media(&receiver_id, phone.manager.device_id));
        assert!(!desktop
            .manager
            .device_call_accepts_media(&caller_id, caller.manager.device_id));
        let mut stopped = false;
        while let Ok(event) = desktop.events.try_recv() {
            match event {
                ConnectionEvent::CallAnsweredElsewhere { .. } => stopped = true,
                ConnectionEvent::CallEnded { .. } => {
                    panic!("an answer on the phone is not a missed call")
                }
                _ => {}
            }
        }
        assert!(stopped, "the device that did not answer is still ringing");
    }

    #[tokio::test]
    async fn one_device_declining_does_not_cancel_other_devices_answer() {
        let (mut caller, mut phone, mut desktop) = call_clients();
        let caller_id = caller.manager.identity.peer_id();
        let receiver_id = phone.manager.identity.peer_id();
        send_call(&mut caller, MessageType::CallRequest, &receiver_id).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        send_call(&mut desktop, MessageType::CallReject, &caller_id).await;
        deliver_calls(&mut desktop, &mut [&mut caller]).await;
        send_call(&mut phone, MessageType::CallAccept, &caller_id).await;
        deliver_calls(&mut phone, &mut [&mut caller]).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        deliver_calls(&mut phone, &mut [&mut caller]).await;
        assert!(caller
            .manager
            .device_call_accepts_media(&receiver_id, phone.manager.device_id));
        send_call(&mut caller, MessageType::CallEnd, &receiver_id).await;
        deliver_calls(&mut caller, &mut [&mut phone, &mut desktop]).await;
        assert!(!phone
            .manager
            .device_call_accepts_media(&caller_id, caller.manager.device_id));
    }

    #[tokio::test]
    async fn two_devices_bootstrap_one_room_key_through_opaque_signaling() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity, 2);
        begin(&mut phone, &mut desktop).await;
        settle(&mut phone, &mut desktop).await;
        for client in [&phone, &desktop] {
            assert!(client.manager.own_room_key_ready("room"));
            assert!(client.manager.group_keys.has_real_key("room"));
        }
        assert_eq!(
            phone.manager.group_keys.epoch_key("room", 0),
            desktop.manager.group_keys.epoch_key("room", 0)
        );
    }

    #[tokio::test]
    async fn newly_elected_phone_adopts_running_desktops_key_without_rotating() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity, 2);
        desktop.manager.group_keys.install("room", 17, [55; 32]);
        begin(&mut phone, &mut desktop).await;
        settle(&mut phone, &mut desktop).await;
        for client in [&phone, &desktop] {
            assert_eq!(client.manager.group_keys.current_epoch("room"), 17);
            assert_eq!(
                client.manager.group_keys.epoch_key("room", 17),
                Some([55; 32])
            );
        }
    }

    #[tokio::test]
    async fn direct_desktop_route_does_not_hide_phone_relay_route() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut sender = client(identity.clone(), 1);
        let recipient = crate::identity::Identity::generate();
        sender
            .manager
            .peer_store
            .write()
            .upsert(crate::peer_store::PeerRecord {
                peer_id: recipient.peer_id(),
                identity_pub: recipient.public_id(),
                ..Default::default()
            });
        let mut direct = sender.manager.test_add_peer_session(&recipient.peer_id());
        let mut message = SignalingMessage::new(MessageType::ChatMessage, identity.public_id());
        message.target = Some(recipient.peer_id());
        message
            .payload
            .insert("body".into(), json!("message for both"));
        assert!(sender.manager.dispatch_outbound(message).await);
        assert!(direct.try_recv().is_ok());
        let Message::Text(raw) = sender.outgoing.try_recv().unwrap() else {
            panic!("expected relay envelope");
        };
        assert!(!raw.contains("message for both"));
        let envelope = SignalingMessage::from_json(&raw).unwrap();
        assert_eq!(envelope.msg_type, MessageType::EncryptedSignal);
        assert_eq!(envelope.target_device, None);
        let key = recipient
            .derive_pairwise_relay_key(&identity.public_id())
            .unwrap();
        let encrypted =
            crate::crypto::b64url_decode(envelope.payload["ciphertext"].as_str().unwrap()).unwrap();
        let plaintext = crate::crypto::decrypt_blob(&key, &encrypted).unwrap();
        let inner = SignalingMessage::from_json(std::str::from_utf8(&plaintext).unwrap()).unwrap();
        assert_eq!(inner.payload["body"], "message for both");
    }

    #[tokio::test]
    async fn direct_device_reconnect_preserves_sibling_and_exact_targets() {
        use crate::connection_manager::internal::{
            DirectEndpoint, InternalEvent, PeerConnection, PeerOutbound,
        };
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut client = client(identity, 1);
        let remote = crate::identity::Identity::generate();
        let remote_id = remote.peer_id();
        let desktop = DirectEndpoint {
            device: Some(DeviceId([4; 32])),
            connection_id: 10,
        };
        let phone = DirectEndpoint {
            device: Some(DeviceId([5; 32])),
            connection_id: 11,
        };
        let replacement = DirectEndpoint {
            connection_id: 12,
            ..desktop
        };
        let (desktop_tx, mut desktop_rx) = mpsc::channel(32);
        let (phone_tx, mut phone_rx) = mpsc::channel(32);
        let (replacement_tx, mut replacement_rx) = mpsc::channel(32);
        let mut peer = PeerConnection::new(&remote_id);
        assert!(peer.register_endpoint(desktop, desktop_tx));
        assert!(peer.register_endpoint(phone, phone_tx));
        client.manager.peers.insert(remote_id.clone(), peer);
        let mut message = SignalingMessage::new(
            MessageType::ChatMessage,
            client.manager.identity.public_id(),
        );
        message.target = Some(remote_id.clone());
        message.payload.insert("body".into(), json!("both devices"));
        assert!(client.manager.dispatch_outbound(message.clone()).await);
        assert!(matches!(
            desktop_rx.try_recv(),
            Ok(PeerOutbound::Reliable(_))
        ));
        assert!(matches!(phone_rx.try_recv(), Ok(PeerOutbound::Reliable(_))));
        assert!(client
            .manager
            .peers
            .get_mut(&remote_id)
            .unwrap()
            .register_endpoint(replacement, replacement_tx));
        client
            .manager
            .handle_internal_event(InternalEvent::QuicDisconnected {
                peer_id: remote_id.clone(),
                endpoint: Some(desktop),
            })
            .await;
        assert_eq!(client.manager.peers[&remote_id].endpoints.len(), 2);
        assert!(desktop_rx.is_closed());
        message.target_device = phone.device;
        assert!(client.manager.dispatch_outbound(message).await);
        assert!(matches!(phone_rx.try_recv(), Ok(PeerOutbound::Reliable(_))));
        assert!(replacement_rx.try_recv().is_err());
        client
            .manager
            .handle_internal_event(InternalEvent::QuicDisconnected {
                peer_id: remote_id.clone(),
                endpoint: Some(phone),
            })
            .await;
        assert!(client.manager.peers[&remote_id].has_endpoint(replacement));
        assert_eq!(client.manager.peers[&remote_id].endpoints.len(), 1);
        client
            .manager
            .handle_internal_event(InternalEvent::QuicDisconnected {
                peer_id: remote_id.clone(),
                endpoint: None,
            })
            .await;
        assert!(client.manager.peers[&remote_id].has_endpoint(replacement));
    }

    #[tokio::test]
    async fn room_message_from_desktop_is_decrypted_once_on_phone() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        begin(&mut phone, &mut desktop).await;
        settle(&mut phone, &mut desktop).await;
        desktop
            .manager
            .send_sfu_chat("host", "room", "From desktop", "Me", "own-message")
            .await;
        let Message::Text(raw) = desktop.outgoing.try_recv().unwrap() else {
            panic!("expected chat");
        };
        assert!(!raw.contains("From desktop"));
        let message = SignalingMessage::from_json(&raw).unwrap();
        assert_eq!(message.source_device, desktop.manager.device_id);
        for _ in 0..2 {
            phone
                .manager
                .handle_inbound_from_supernode("host".into(), message.clone())
                .await;
        }
        let mut received = 0;
        while let Ok(event) = phone.events.try_recv() {
            if let crate::connection_manager::ConnectionEvent::RoomChatMessage {
                sender_id,
                body,
                message_id,
                ..
            } = event
            {
                assert_eq!(sender_id, identity.public_id());
                assert_eq!(body, "From desktop");
                assert_eq!(message_id, "own-message");
                received += 1;
            }
        }
        assert_eq!(received, 1);
    }

    #[tokio::test]
    async fn leaving_last_host_discards_handoff_and_rejects_late_reply() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity, 2);
        desktop.manager.group_keys.install("room", 17, [55; 32]);
        begin(&mut phone, &mut desktop).await;
        forward(&mut phone, &mut desktop).await;
        phone.manager.forget_host_device_rosters("host");
        forward(&mut desktop, &mut phone).await;
        assert!(phone.manager.room_device_rosters.is_empty());
        assert!(!phone.manager.own_room_key_rounds.contains_key("room"));
        assert!(!phone.manager.group_keys.has_real_key("room"));
    }

    #[tokio::test]
    async fn malformed_or_cleartext_handoff_does_not_authorize_key_creation() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        begin(&mut phone, &mut desktop).await;
        let round = phone.manager.own_room_key_rounds.get("room").unwrap();
        let mut reply = SignalingMessage::new(MessageType::SfuDeviceKeySync, identity.public_id());
        reply.target = Some(identity.public_id());
        reply.source_device = desktop.manager.device_id;
        reply.target_device = phone.manager.device_id;
        reply.payload = json!({"room_id":"room", "roster":round.fingerprint,
            "challenge":round.challenge, "request":false, "epoch":17})
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .collect();
        for key in [
            None,
            Some(json!(true)),
            Some(json!(5)),
            Some(json!("invalid")),
        ] {
            reply.payload.remove("key");
            if let Some(key) = key {
                reply.payload.insert("key".into(), key);
            }
            phone.manager.handle_own_room_key_sync(&reply).await;
            assert!(!phone.manager.own_room_key_ready("room"));
        }
        reply.payload.insert("key".into(), Value::Null);
        desktop.manager.sign_message_json(&mut reply).unwrap();
        phone
            .manager
            .handle_inbound_from_supernode("host".into(), reply)
            .await;
        assert!(!phone.manager.own_room_key_ready("room"));
        assert!(!phone.manager.group_keys.has_real_key("room"));
    }

    #[tokio::test]
    async fn stale_handoff_challenge_cannot_authorize_minting_or_install_a_key() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        begin(&mut phone, &mut desktop).await;
        let round = phone.manager.own_room_key_rounds.get("room").unwrap();
        let mut stale = SignalingMessage::new(MessageType::SfuDeviceKeySync, identity.public_id());
        stale.source_device = desktop.manager.device_id;
        stale.target_device = phone.manager.device_id;
        stale.payload = json!({"room_id":"room", "roster":round.fingerprint,
            "challenge":([0u8;32]), "request":false, "epoch":17, "key":crate::crypto::b64url_encode(&[55;32])})
            .as_object().unwrap().clone().into_iter().collect();
        phone.manager.handle_own_room_key_sync(&stale).await;
        assert!(!phone.manager.own_room_key_ready("room"));
        assert!(!phone.manager.group_keys.has_real_key("room"));
    }

    #[tokio::test]
    async fn settled_round_stops_asking_siblings() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity, 2);
        begin(&mut phone, &mut desktop).await;
        settle(&mut phone, &mut desktop).await;
        for client in [&mut phone, &mut desktop] {
            assert!(client.manager.own_room_key_ready("room"));
            // Past the resend throttle, a settled round must still send nothing.
            client
                .manager
                .own_room_key_rounds
                .get_mut("room")
                .unwrap()
                .last_request = None;
            client.manager.request_own_room_key("room").await;
            client.manager.retry_own_room_key_sync().await;
            assert!(
                client.outgoing.try_recv().is_err(),
                "a settled round kept asking"
            );
        }
    }

    #[tokio::test]
    async fn outdated_own_device_warns_once_and_rearms_after_recovery() {
        use crate::connection_manager::ConnectionEvent;
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut desktop = client(identity.clone(), 2);
        let me = identity.public_id();
        let with_legacy = json!([
            {"identity": me, "device": desktop.manager.device_id, "voice": false},
            {"identity": me, "device": null, "voice": false},
        ]);
        for _ in 0..3 {
            assert!(!desktop
                .manager
                .record_room_devices("host", "room", Some(&with_legacy)));
        }
        let warned = std::iter::from_fn(|| desktop.events.try_recv().ok())
            .filter(|event| matches!(event, ConnectionEvent::OwnDeviceOutdated { room_id, outdated: true } if room_id == "room"))
            .count();
        assert_eq!(warned, 1, "repeated rosters must not repeat the warning");

        let updated =
            json!([{"identity": me, "device": desktop.manager.device_id, "voice": false}]);
        assert!(desktop
            .manager
            .record_room_devices("host", "room", Some(&updated)));
        assert!(
            std::iter::from_fn(|| desktop.events.try_recv().ok()).any(|event| matches!(
                event,
                ConnectionEvent::OwnDeviceOutdated {
                    outdated: false,
                    ..
                }
            )),
            "recovery must lift the warning"
        );
        assert!(!desktop
            .manager
            .record_room_devices("host", "room", Some(&with_legacy)));
        assert!(
            std::iter::from_fn(|| desktop.events.try_recv().ok()).any(|event| matches!(
                event,
                ConnectionEvent::OwnDeviceOutdated { outdated: true, .. }
            )),
            "a recurrence after recovery must warn again"
        );
    }

    #[tokio::test]
    async fn leaving_a_room_rearms_the_outdated_device_warning() {
        use crate::connection_manager::ConnectionEvent;
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut desktop = client(identity.clone(), 2);
        let me = identity.public_id();
        let with_legacy = json!([
            {"identity": me, "device": desktop.manager.device_id, "voice": false},
            {"identity": me, "device": null, "voice": false},
        ]);
        assert!(!desktop
            .manager
            .record_room_devices("host", "room", Some(&with_legacy)));
        desktop.manager.forget_room_device_scope("host", "room");
        let states: Vec<bool> = std::iter::from_fn(|| desktop.events.try_recv().ok())
            .filter_map(|event| match event {
                ConnectionEvent::OwnDeviceOutdated { outdated, .. } => Some(outdated),
                _ => None,
            })
            .collect();
        assert_eq!(states, [true, false], "leaving must lift the warning");

        // The old device is still signed in when we come back.
        assert!(!desktop
            .manager
            .record_room_devices("host", "room", Some(&with_legacy)));
        assert!(
            std::iter::from_fn(|| desktop.events.try_recv().ok()).any(|event| matches!(
                event,
                ConnectionEvent::OwnDeviceOutdated { outdated: true, .. }
            )),
            "rejoining with the old device still signed in must warn again"
        );
    }

    fn room_snapshot(identity: &crate::identity::Identity) -> crate::room_store::OwnRoomSnapshot {
        crate::room_store::OwnRoomSnapshot {
            v: 1,
            rooms: vec![crate::room_store::RoomEntry::new("r1", "Phone Lounge")
                .with_supernode("host")
                .with_creator(identity.public_id(), true)],
            spaces: vec![],
        }
    }

    fn rooms_received(client: &mut Client) -> Vec<(crate::room_store::OwnRoomSnapshot, bool)> {
        std::iter::from_fn(|| client.events.try_recv().ok())
            .filter_map(|event| match event {
                crate::connection_manager::ConnectionEvent::OwnRoomsReceived {
                    snapshot,
                    reply_wanted,
                } => Some((snapshot, reply_wanted)),
                _ => None,
            })
            .collect()
    }

    /// Seal `inner` from `from` to our identity and deliver it to `to`.
    async fn deliver_sealed(
        from: &mut Client,
        to: &mut Client,
        inner: &SignalingMessage,
        target: &str,
    ) {
        let envelope = from.manager.seal_signal_to_member(inner, target).unwrap();
        assert!(from.manager.dispatch_outbound(envelope).await);
        let Message::Text(raw) = from.outgoing.try_recv().unwrap() else {
            panic!("expected an envelope");
        };
        to.manager
            .handle_inbound_from_supernode(
                "host".into(),
                SignalingMessage::from_json(&raw).unwrap(),
            )
            .await;
    }

    #[tokio::test]
    async fn a_room_snapshot_reaches_the_other_device_sealed() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        phone
            .manager
            .queue_own_room_sync(room_snapshot(&identity), true)
            .await;
        let Message::Text(raw) = phone.outgoing.try_recv().unwrap() else {
            panic!("expected a sealed snapshot");
        };
        assert!(
            !raw.contains("Phone Lounge"),
            "the supernode sees no room names"
        );
        let envelope = SignalingMessage::from_json(&raw).unwrap();
        assert_eq!(envelope.msg_type, MessageType::EncryptedSignal);
        assert_eq!(
            envelope.target.as_deref(),
            Some(identity.public_id().as_str())
        );
        assert_eq!(envelope.target_device, None, "every device of ours gets it");
        for client in [&mut phone, &mut desktop] {
            client
                .manager
                .handle_inbound_from_supernode("host".into(), envelope.clone())
                .await;
        }
        let got = rooms_received(&mut desktop);
        assert_eq!(got.len(), 1);
        assert!(got[0].1, "the request for a reply survives the trip");
        assert_eq!(got[0].0.rooms[0].room_name, "Phone Lounge");
        assert!(
            rooms_received(&mut phone).is_empty(),
            "a device ignores its own"
        );
    }

    #[tokio::test]
    async fn room_snapshots_are_spaced_and_the_latest_one_goes_out() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        phone
            .manager
            .queue_own_room_sync(room_snapshot(&identity), false)
            .await;
        assert!(phone.outgoing.try_recv().is_ok());
        let mut newer = room_snapshot(&identity);
        newer.rooms[0].room_name = "Renamed".into();
        phone
            .manager
            .queue_own_room_sync(room_snapshot(&identity), true)
            .await;
        phone.manager.queue_own_room_sync(newer, false).await;
        assert!(
            phone.outgoing.try_recv().is_err(),
            "held back by the spacing"
        );
        phone.manager.own_room_sync.last_sent = Some(Instant::now() - OWN_ROOM_SYNC_SPACING);
        phone.manager.flush_own_room_sync().await;
        let Message::Text(raw) = phone.outgoing.try_recv().unwrap() else {
            panic!("expected the waiting snapshot");
        };
        assert!(phone.outgoing.try_recv().is_err(), "sent once");
        desktop
            .manager
            .handle_inbound_from_supernode(
                "host".into(),
                SignalingMessage::from_json(&raw).unwrap(),
            )
            .await;
        let got = rooms_received(&mut desktop);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.rooms[0].room_name, "Renamed");
        assert!(got[0].1, "a queued reply request is not lost when replaced");
    }

    #[tokio::test]
    async fn room_snapshots_need_our_identity_a_seal_and_a_fresh_signature() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let me = identity.public_id();
        let mut phone = client(identity.clone(), 1);
        let mut desktop = client(identity.clone(), 2);
        let snapshot = serde_json::to_value(room_snapshot(&identity)).unwrap();
        let message = |sender: &str, snapshot: Value| {
            let mut inner = SignalingMessage::new(MessageType::DeviceRoomSync, sender.to_owned());
            inner.target = Some(me.clone());
            inner.payload.insert("snapshot".into(), snapshot);
            inner
        };

        // In the clear, correctly signed by our own other device.
        let mut clear = message(&me, snapshot.clone());
        phone.manager.sign_message_json(&mut clear).unwrap();
        desktop
            .manager
            .handle_inbound_from_supernode("host".into(), clear)
            .await;
        assert!(
            rooms_received(&mut desktop).is_empty(),
            "cleartext is refused"
        );

        // Sealed to us, but from another identity.
        let stranger_identity = Arc::new(crate::identity::Identity::generate());
        let mut stranger = client(stranger_identity.clone(), 3);
        let mut foreign = message(&stranger_identity.public_id(), snapshot.clone());
        stranger.manager.sign_message_json(&mut foreign).unwrap();
        deliver_sealed(&mut stranger, &mut desktop, &foreign, &me).await;
        assert!(
            rooms_received(&mut desktop).is_empty(),
            "another identity is refused"
        );

        // Ours and sealed, but stale.
        let mut stale = message(&me, snapshot.clone());
        stale.timestamp -= ConnectionManager::MAX_MESSAGE_AGE_SECS + 60.0;
        phone.manager.sign_message_json(&mut stale).unwrap();
        deliver_sealed(&mut phone, &mut desktop, &stale, &me).await;
        assert!(
            rooms_received(&mut desktop).is_empty(),
            "a stale snapshot is refused"
        );

        // Ours, sealed and fresh, but malformed.
        let mut bad = message(&me, json!({"v": 1, "rooms": "nope"}));
        phone.manager.sign_message_json(&mut bad).unwrap();
        deliver_sealed(&mut phone, &mut desktop, &bad, &me).await;
        assert!(
            rooms_received(&mut desktop).is_empty(),
            "a malformed snapshot is refused"
        );

        // The same, well formed, is accepted: the refusals above were not luck.
        let mut good = message(&me, snapshot);
        phone.manager.sign_message_json(&mut good).unwrap();
        deliver_sealed(&mut phone, &mut desktop, &good, &me).await;
        assert_eq!(rooms_received(&mut desktop).len(), 1);
    }

    #[tokio::test]
    async fn room_snapshots_wait_for_device_routing_and_a_supernode() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let mut legacy = client(identity.clone(), 1);
        legacy.manager.device_id = None;
        legacy
            .manager
            .queue_own_room_sync(room_snapshot(&identity), true)
            .await;
        assert!(legacy.outgoing.try_recv().is_err());
        assert!(legacy.manager.own_room_sync.pending.is_none());

        let mut phone = client(identity.clone(), 2);
        for session in phone.manager.supernodes.values_mut() {
            session.connected = false;
        }
        phone
            .manager
            .queue_own_room_sync(room_snapshot(&identity), true)
            .await;
        assert!(phone.outgoing.try_recv().is_err());
        for session in phone.manager.supernodes.values_mut() {
            session.connected = true;
        }
        phone.manager.flush_own_room_sync().await;
        assert!(
            phone.outgoing.try_recv().is_ok(),
            "sent once a supernode is up"
        );
    }
}
