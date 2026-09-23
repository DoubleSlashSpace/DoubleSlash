//! One answering endpoint per call, while other devices keep their sessions.

use std::time::{Duration, Instant};

use doubleslash_features::DeviceId;
use rand::RngCore;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::ConnectionManager;
use crate::connection_manager::{internal::PeerOutbound, ConnectionEvent};
use crate::protocol::{MessageType, SignalingMessage};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Outgoing,
    Incoming,
    Answering,
    Active,
}

pub(super) struct DeviceCall {
    id: String,
    remote: Option<DeviceId>,
    phase: Phase,
    deadline: Instant,
    initiator: bool,
    confirmed: bool,
    last_selection: Option<Instant>,
}

fn is_call(kind: &MessageType) -> bool {
    matches!(
        kind,
        MessageType::CallRequest
            | MessageType::CallAccept
            | MessageType::CallReject
            | MessageType::CallEnd
    )
}

impl ConnectionManager {
    pub(super) fn prepare_device_call(&mut self, message: &mut SignalingMessage) -> bool {
        if self.device_id.is_none() || !is_call(&message.msg_type) {
            return true;
        }
        // Selection confirmations are assembled only by the authenticated
        // incoming-answer handler, with the originating call ID preserved.
        if message.payload.get("device_selection") == Some(&Value::Bool(true))
            || message.payload.get("device_selection_ack") == Some(&Value::Bool(true))
        {
            return true;
        }
        let Some(target) = message.target.as_deref() else {
            return false;
        };
        let peer = self.canonical_peer_id_for_sender(target);
        if message.msg_type == MessageType::CallRequest {
            let call = self.device_calls.entry(peer).or_insert_with(|| {
                let mut nonce = [0; 16];
                rand::rngs::OsRng.fill_bytes(&mut nonce);
                DeviceCall {
                    id: hex::encode(nonce),
                    remote: None,
                    phase: Phase::Outgoing,
                    deadline: Instant::now() + Duration::from_secs(60),
                    initiator: true,
                    confirmed: false,
                    last_selection: None,
                }
            });
            message.payload.insert("call_id".into(), json!(call.id));
            message.target_device = call.remote;
            return true;
        }
        let Some(call) = self.device_calls.get_mut(&peer) else {
            return false;
        };
        message.payload.insert("call_id".into(), json!(call.id));
        message.target_device = call.remote;
        if message.msg_type == MessageType::CallAccept {
            if call.phase != Phase::Incoming && call.phase != Phase::Answering {
                return false;
            }
            call.phase = Phase::Answering;
            call.deadline = Instant::now() + Duration::from_secs(15);
        } else {
            self.device_calls.remove(&peer);
            self.pending_call_fallback_checks.remove(&peer);
            if self.direct_fallback.is_pending_for(&peer) {
                self.direct_fallback.cancel();
            }
        }
        true
    }

    /// Returns whether normal UI call handling should consume this message.
    pub(super) async fn receive_device_call(&mut self, message: &SignalingMessage) -> bool {
        if self.device_id.is_none() || !is_call(&message.msg_type) {
            return true;
        }
        if !self.check_inbound_feature_quota("core.audio.opus", &message.sender, 256) {
            return false;
        }
        let Some(source) = message.source_device else {
            return false;
        };
        let Some(id) = message
            .payload
            .get("call_id")
            .and_then(Value::as_str)
            .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        else {
            return false;
        };
        let peer = self.canonical_peer_id_for_sender(&message.sender);
        if message.msg_type == MessageType::CallRequest {
            if let Some(call) = self.device_calls.get(&peer) {
                // An active call may receive updated private-room fallback
                // coordinates; another device's unrelated call cannot replace it.
                return call.id == id
                    && call.remote == Some(source)
                    && message.payload.contains_key("fallback_room_id");
            }
            self.device_calls.insert(
                peer,
                DeviceCall {
                    id: id.into(),
                    remote: Some(source),
                    phase: Phase::Incoming,
                    deadline: Instant::now() + Duration::from_secs(60),
                    initiator: false,
                    confirmed: false,
                    last_selection: None,
                },
            );
            return true;
        }
        let Some(call) = self
            .device_calls
            .get_mut(&peer)
            .filter(|call| call.id == id)
        else {
            return false;
        };
        if message.msg_type == MessageType::CallAccept {
            if message.payload.get("device_selection_ack") == Some(&Value::Bool(true)) {
                if call.initiator && call.phase == Phase::Active && call.remote == Some(source) {
                    call.confirmed = true;
                }
                return false;
            }
            if message.payload.get("device_selection") == Some(&Value::Bool(true)) {
                if call.remote != Some(source) || call.phase == Phase::Outgoing {
                    return false;
                }
                let Some(selected) = message
                    .payload
                    .get("selected_device")
                    .and_then(|value| serde_json::from_value::<DeviceId>(value.clone()).ok())
                else {
                    return false;
                };
                if Some(selected) == self.device_id {
                    if call.phase != Phase::Answering && call.phase != Phase::Active {
                        return false;
                    }
                    call.phase = Phase::Active;
                    call.confirmed = true;
                    let mut ack =
                        SignalingMessage::new(MessageType::CallAccept, self.identity.public_id());
                    ack.target = Some(peer);
                    ack.target_device = Some(source);
                    ack.payload.insert("call_id".into(), json!(id));
                    ack.payload
                        .insert("device_selection_ack".into(), Value::Bool(true));
                    self.dispatch_outbound(ack).await;
                } else {
                    self.device_calls.remove(&peer);
                    self.emit_event(ConnectionEvent::CallAnsweredElsewhere { peer_id: peer });
                }
                return false;
            }
            let first = call.phase == Phase::Outgoing;
            if !call.initiator || (!first && call.phase != Phase::Active) {
                return false;
            }
            if first {
                call.remote = Some(source);
                call.phase = Phase::Active;
                call.deadline = Instant::now() + Duration::from_secs(15);
            }
            call.last_selection = Some(Instant::now());
            let mut selection =
                SignalingMessage::new(MessageType::CallAccept, self.identity.public_id());
            selection.target = Some(peer);
            selection.payload.insert("call_id".into(), json!(id));
            selection
                .payload
                .insert("device_selection".into(), Value::Bool(true));
            selection
                .payload
                .insert("selected_device".into(), json!(call.remote));
            self.dispatch_outbound(selection).await;
            return first;
        }
        // A decline from one ringing phone must not cancel the other device's
        // answer. The outgoing ring expires if no device accepts.
        if call.phase == Phase::Outgoing || call.remote != Some(source) {
            return false;
        }
        self.device_calls.remove(&peer);
        true
    }

    pub(super) fn expire_device_calls(&mut self) {
        let expired: Vec<_> = self
            .device_calls
            .iter()
            .filter(|(_, call)| {
                (call.phase != Phase::Active || !call.confirmed) && call.deadline <= Instant::now()
            })
            .map(|(peer, _)| peer.clone())
            .collect();
        for peer in expired {
            self.device_calls.remove(&peer);
            self.pending_call_fallback_checks.remove(&peer);
            if self.direct_fallback.is_pending_for(&peer) {
                self.direct_fallback.cancel();
            }
            self.emit_event(ConnectionEvent::CallEnded { peer_id: peer });
        }
    }

    pub(super) async fn retry_device_call_selections(&mut self) {
        let now = Instant::now();
        let messages: Vec<_> = self
            .device_calls
            .iter_mut()
            .filter_map(|(peer, call)| {
                if !call.initiator
                    || call.phase != Phase::Active
                    || call.confirmed
                    || call
                        .last_selection
                        .is_some_and(|last| now.duration_since(last) < Duration::from_secs(3))
                {
                    return None;
                }
                call.last_selection = Some(now);
                let mut selection =
                    SignalingMessage::new(MessageType::CallAccept, self.identity.public_id());
                selection.target = Some(peer.clone());
                selection.payload.insert("call_id".into(), json!(call.id));
                selection
                    .payload
                    .insert("device_selection".into(), Value::Bool(true));
                selection
                    .payload
                    .insert("selected_device".into(), json!(call.remote));
                Some(selection)
            })
            .collect();
        for message in messages {
            self.dispatch_outbound(message).await;
        }
    }

    pub(super) fn device_call_accepts_media(&self, peer: &str, device: Option<DeviceId>) -> bool {
        self.device_id.is_none()
            || self
                .device_calls
                .get(peer)
                .is_some_and(|call| call.phase == Phase::Active && call.remote == device)
    }

    pub(super) fn direct_media_sender(&self, peer: &str) -> Option<mpsc::Sender<PeerOutbound>> {
        let connection = self.peers.get(peer)?;
        if self.device_id.is_none() {
            return connection.quic_out_tx.clone();
        }
        let call = self
            .device_calls
            .get(peer)
            .filter(|call| call.phase == Phase::Active)?;
        connection
            .endpoints
            .get(&call.remote)
            .map(|(_, sender)| sender.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::Arc;

    #[tokio::test]
    async fn unconfirmed_selection_retries_then_expires_without_enabling_another_device() {
        let identity = Arc::new(crate::identity::Identity::generate());
        let remote = crate::identity::Identity::generate();
        let profile = tempfile::tempdir().unwrap();
        let store =
            crate::peer_store::PeerStore::open(&identity, Some(&profile.path().join("peers.dat")))
                .unwrap();
        let (mut manager, mut events) =
            ConnectionManager::new_for_test(identity.clone(), Arc::new(RwLock::new(store)));
        manager.device_id = Some(DeviceId([1; 32]));
        manager
            .peer_store
            .write()
            .upsert(crate::peer_store::PeerRecord {
                peer_id: remote.peer_id(),
                identity_pub: remote.public_id(),
                ..Default::default()
            });
        let mut outbound = manager.test_add_supernode_session("host");
        manager.device_calls.insert(
            remote.peer_id(),
            DeviceCall {
                id: "1".repeat(32),
                remote: Some(DeviceId([2; 32])),
                phase: Phase::Active,
                deadline: Instant::now() + Duration::from_secs(15),
                initiator: true,
                confirmed: false,
                last_selection: None,
            },
        );
        manager.retry_device_call_selections().await;
        assert!(outbound.try_recv().is_ok());
        manager.retry_device_call_selections().await;
        assert!(outbound.try_recv().is_err());
        assert!(!manager.device_call_accepts_media(&remote.peer_id(), Some(DeviceId([3; 32]))));
        manager
            .device_calls
            .get_mut(&remote.peer_id())
            .unwrap()
            .deadline = Instant::now() - Duration::from_secs(1);
        manager.expire_device_calls();
        assert!(manager.device_calls.is_empty());
        assert!(matches!(
            events.try_recv(),
            Ok(ConnectionEvent::CallEnded { .. })
        ));
        assert!(!manager.device_call_accepts_media(&remote.peer_id(), Some(DeviceId([2; 32]))));
    }
}
