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
            return false;
        }
        self.room_device_rosters.insert(scope, roster);
        let devices = self.room_devices(room, &self.identity.public_id());
        if devices.contains(&None) || devices.len() > MAX_LIVE_DEVICE_ROUTES {
            tracing::debug!(
                "[own-room-key] {room}: no round (legacy endpoint={}, devices={})",
                devices.contains(&None),
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
        round.last_request = Some(Instant::now());
        let targets: Vec<_> = round
            .members
            .iter()
            .copied()
            .filter(|device| *device != me)
            .collect();
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
            if matches!(event, ConnectionEvent::CallEnded { .. }) {
                stopped = true;
            }
        }
        assert!(stopped);
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
}
