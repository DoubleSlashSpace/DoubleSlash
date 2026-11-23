//! Local identity denials. Membership and room-key control remain independent.

use super::{ConnectionEvent, ConnectionManager, PeerConnectionState};

impl ConnectionManager {
    pub(super) async fn set_identity_blocked(&mut self, id: &str, label: &str, blocked: bool) {
        self.apply_identity_block(id, label, blocked, false).await;
    }

    pub(super) async fn apply_identity_block(
        &mut self,
        id: &str,
        label: &str,
        blocked: bool,
        purge_messages: bool,
    ) {
        let own = self.identity.public_id();
        let (public_id, peer_id) = {
            let store = self.peer_store.read();
            let record = store.find_identity(id);
            (
                record.map_or(id, |r| r.identity_pub.as_str()).to_owned(),
                record.map_or(id, |r| r.peer_id.as_str()).to_owned(),
            )
        };
        let result = if public_id.trim_end_matches('=') == own.trim_end_matches('=')
            || id == self.identity.peer_id()
        {
            Err(crate::error::ClientError::Store(
                "you cannot block your own identity".into(),
            ))
        } else {
            self.peer_store
                .write()
                .set_identity_blocked(id, label, blocked)
        };
        let public_id = match result {
            Ok(public_id) => public_id,
            Err(error) => {
                self.emit_event(ConnectionEvent::IdentityBlockResult {
                    public_id,
                    blocked,
                    purge_messages,
                    error: error.to_string(),
                });
                return;
            }
        };
        if blocked {
            self.cancel_peer_reconnect(&peer_id);
            self.pending_call_fallback_checks.remove(&peer_id);
            if self.direct_fallback.is_pending_for(&peer_id) {
                self.direct_fallback.cancel();
            }
            if let Some(conn) = self.peers.get_mut(&peer_id) {
                conn.state = PeerConnectionState::Disconnected;
                conn.quic_out_tx = None;
                conn.endpoints.clear();
            }
            let mut cancelled = self.file_mgr.stop_identity(&public_id, &peer_id);
            cancelled.extend(self.room_file_mgr.stop_identity(&public_id, &peer_id));
            for transfer_id in cancelled {
                self.emit_event(ConnectionEvent::FileFailed {
                    transfer_id,
                    reason: "sender blocked".into(),
                    retryable: false,
                });
            }
            if let Some((_, senders)) = &self.video_subscriptions {
                let senders = senders
                    .iter()
                    .filter(|s| s.trim_end_matches('=') != public_id)
                    .cloned()
                    .collect();
                self.send_video_subscriptions(senders, false).await;
            }
            // Flush existing video presentation as well as filtering new frames.
            self.emit_event(ConnectionEvent::PeerVideoStateChanged {
                peer_id: public_id.clone(),
                active: false,
            });
        }
        self.emit_event(ConnectionEvent::IdentityBlockResult {
            public_id,
            blocked,
            purge_messages: blocked && purge_messages,
            error: String::new(),
        });
    }

    /// A final boundary before any client stores, notifies, decodes or plays
    /// participant content. Keep room-key recovery/election on the receive path.
    pub(super) fn blocked_content_event(&self, event: &ConnectionEvent) -> bool {
        let sender = match event {
            ConnectionEvent::RoomChatMessage { sender_id, .. } => sender_id,
            ConnectionEvent::SfuAudioReceived { peer_id, .. }
            | ConnectionEvent::DirectAudioReceived { peer_id, .. }
            | ConnectionEvent::VideoFrameReceived { peer_id, .. }
            | ConnectionEvent::ContentAudioReceived { peer_id, .. }
            | ConnectionEvent::ChatMessage { peer_id, .. }
            | ConnectionEvent::CallRequest { peer_id, .. }
            | ConnectionEvent::TypingIndicator { peer_id, .. } => peer_id,
            ConnectionEvent::PeerVideoStateChanged {
                peer_id,
                active: true,
            } => peer_id,
            ConnectionEvent::FileOffered {
                origin_id,
                is_self: false,
                ..
            } => origin_id,
            ConnectionEvent::FileComplete { peer_id, .. } => peer_id,
            ConnectionEvent::TrustInviteReceived {
                sender_public_id, ..
            } => sender_public_id,
            _ => return false,
        };
        self.peer_store.read().is_blocked(sender)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity::Identity,
        peer_store::PeerStore,
        protocol::{MessageType, SignalingMessage},
    };
    use parking_lot::RwLock;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn receiver() -> (
        ConnectionManager,
        mpsc::Receiver<ConnectionEvent>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let id = Arc::new(Identity::generate());
        let store = PeerStore::open(&id, Some(&dir.path().join("peers.dat"))).unwrap();
        let (cm, events) = ConnectionManager::new_for_test(id, Arc::new(RwLock::new(store)));
        (cm, events, dir)
    }

    fn drain(events: &mut mpsc::Receiver<ConnectionEvent>) -> Vec<ConnectionEvent> {
        let mut out = Vec::new();
        while let Ok(event) = events.try_recv() {
            out.push(event);
        }
        out
    }

    fn signed(
        sender: &Identity,
        kind: MessageType,
        payload: serde_json::Value,
    ) -> SignalingMessage {
        let mut message = SignalingMessage::new(kind, sender.public_id());
        message.payload = serde_json::from_value(payload).unwrap();
        message.signature = Some(crate::crypto::b64url_encode(
            &sender.sign(&message.canonical_bytes().unwrap()),
        ));
        message
    }

    fn chat(cm: &ConnectionManager, sender: &Identity, id: &str) -> SignalingMessage {
        let (epoch, body) = crate::group_key::seal_chat_body(
            &cm.group_keys,
            "room",
            &sender.public_id(),
            id,
            b"hello",
        )
        .unwrap();
        signed(
            sender,
            MessageType::SfuChat,
            json!({ "room_id": "room", "message_id": id, "e2e": true, "epoch": epoch, "body": crate::crypto::b64url_encode(&body) }),
        )
    }

    #[tokio::test]
    async fn room_chat_and_voice_are_filtered_by_identity_without_granting_trust() {
        let (mut cm, mut events, _dir) = receiver();
        let sender = Identity::generate();
        cm.group_keys.install("room", 0, [7; 32]);
        let _out = cm.test_add_supernode_session("host");
        cm.room_group_members.insert(
            "host:room".into(),
            std::collections::HashSet::from([sender.public_id()]),
        );
        let first = chat(&cm, &sender, "before");
        cm.handle_inbound_from_supernode("host".into(), first).await;
        assert!(drain(&mut events)
            .iter()
            .any(|e| matches!(e, ConnectionEvent::RoomChatMessage { .. })));
        cm.set_identity_blocked(sender.public_id().trim_end_matches('='), "Stranger", true)
            .await;
        drain(&mut events);
        let second = chat(&cm, &sender, "blocked");
        cm.handle_inbound_from_supernode("host".into(), second)
            .await;
        let audio = crate::group_key::seal_voice_frame(
            &cm.group_keys,
            "room",
            &sender.public_id(),
            1,
            b"opus",
        )
        .unwrap();
        cm.handle_inbound_from_supernode("host".into(), signed(&sender, MessageType::SfuAudio,
            json!({ "room_id": "room", "seq": 1, "e2e": true, "audio": crate::crypto::b64url_encode(&audio) }))).await;
        assert!(!drain(&mut events).iter().any(|e| matches!(
            e,
            ConnectionEvent::RoomChatMessage { .. } | ConnectionEvent::SfuAudioReceived { .. }
        )));
        assert!(cm.room_group_members["host:room"].contains(&sender.public_id()));
        cm.set_identity_blocked(&sender.public_id(), "", false)
            .await;
        assert!(!ConnectionManager::is_trusted_sender(
            &cm.peer_store,
            &sender.public_id()
        ));
        let third = chat(&cm, &sender, "unblocked");
        cm.handle_inbound_from_supernode("host".into(), third).await;
        assert!(drain(&mut events)
            .iter()
            .any(|e| matches!(e, ConnectionEvent::RoomChatMessage { .. })));
        assert!(cm.peer_store.read().list_peers().is_empty());
    }

    #[tokio::test]
    async fn purge_intent_is_acknowledged_only_after_a_successful_block() {
        let (mut cm, mut events, _dir) = receiver();
        let stranger = Identity::generate().public_id();
        cm.apply_identity_block(&stranger, "Member", true, true)
            .await;
        assert!(cm.peer_store.read().is_blocked(&stranger));
        assert!(drain(&mut events).iter().any(|event| matches!(event,
            ConnectionEvent::IdentityBlockResult { blocked: true, purge_messages: true, error, .. }
            if error.is_empty())));
        cm.apply_identity_block(&stranger, "", false, true).await;
        assert!(drain(&mut events).iter().any(|event| matches!(event,
            ConnectionEvent::IdentityBlockResult { blocked: false, purge_messages: false, error, .. }
            if error.is_empty())));
        cm.apply_identity_block(&cm.identity.public_id(), "Me", true, true)
            .await;
        assert!(!drain(&mut events).iter().any(|event| matches!(event,
            ConnectionEvent::IdentityBlockResult { blocked: true, purge_messages: true, error, .. }
            if error.is_empty())));
        assert!(cm.peer_store.read().list_peers().is_empty());
    }

    #[tokio::test]
    async fn self_and_malformed_blocks_fail_without_changing_the_list() {
        let (mut cm, mut events, _dir) = receiver();
        cm.set_identity_blocked(&cm.identity.public_id(), "Me", true)
            .await;
        cm.set_identity_blocked("not-a-key", "Bad", true).await;
        assert_eq!(drain(&mut events).iter().filter(|e| matches!(e, ConnectionEvent::IdentityBlockResult { error, .. } if !error.is_empty())).count(), 2);
        assert!(cm.peer_store.read().blocked_people().is_empty());
    }

    #[tokio::test]
    async fn authenticated_room_video_and_shared_audio_follow_block_and_unblock() {
        use crate::group_key::{seal_media_frame, MediaKind};
        use doubleslash_features::video_codec::VideoCodec;
        let (mut cm, mut events, _dir) = receiver();
        let sender = Identity::generate();
        let public_id = sender.public_id();
        cm.current_room_id = "room".into();
        cm.group_keys.install("room", 0, [7; 32]);
        for (seq, blocked) in [(1, false), (2, true), (3, false)] {
            cm.set_identity_blocked(&public_id, "Room member", blocked)
                .await;
            drain(&mut events);
            let payload = seal_media_frame(
                &cm.group_keys,
                MediaKind::Video,
                "room",
                &public_id,
                u64::from(seq),
                b"video",
            )
            .unwrap();
            let signature = sender.sign(&crate::video::video_frame_signing_bytes(
                "room",
                &public_id,
                seq,
                VideoCodec::Vp9,
                10,
                &payload,
            ));
            let fragments = crate::video::fragment::fragment_frame(
                &public_id,
                seq,
                true,
                VideoCodec::Vp9,
                10,
                signature
                    .as_slice()
                    .try_into()
                    .expect("Ed25519 signature length"),
                &payload,
                1200,
            )
            .unwrap();
            for fragment in fragments {
                cm.accept_video_fragment("host", &fragment, true).await;
            }
            let payload = seal_media_frame(
                &cm.group_keys,
                MediaKind::ContentAudio,
                "room",
                &public_id,
                u64::from(seq),
                b"opus",
            )
            .unwrap();
            let signature = sender.sign(&crate::content_audio::content_audio_signing_bytes(
                "room", &public_id, seq, 10, &payload,
            ));
            let frame = crate::content_audio::encode_frame(
                &public_id,
                seq,
                10,
                signature
                    .as_slice()
                    .try_into()
                    .expect("Ed25519 signature length"),
                &payload,
            )
            .unwrap();
            cm.accept_content_audio_frame("host", &frame, true).await;
            let media = drain(&mut events)
                .into_iter()
                .filter(|e| {
                    matches!(
                        e,
                        ConnectionEvent::VideoFrameReceived { .. }
                            | ConnectionEvent::ContentAudioReceived { .. }
                    )
                })
                .count();
            assert_eq!(media, if blocked { 0 } else { 2 });
        }
    }

    #[tokio::test]
    async fn blocked_room_frames_still_record_an_unopenable_epoch() {
        let (mut cm, mut events, _dir) = receiver();
        let sender = Identity::generate();
        cm.group_keys.install("room", 0, [7; 32]);
        cm.set_identity_blocked(&sender.public_id(), "Member", true)
            .await;
        drain(&mut events);
        let mut newer = crate::group_key::SenderKeysGroup::new();
        newer.install("room", 4, [8; 32]);
        let (epoch, body) = crate::group_key::seal_chat_body(
            &newer,
            "room",
            &sender.public_id(),
            "newer",
            b"hello",
        )
        .unwrap();
        let message = signed(
            &sender,
            MessageType::SfuChat,
            json!({"room_id": "room", "message_id": "newer", "e2e": true, "epoch": epoch, "body": crate::crypto::b64url_encode(&body)}),
        );
        cm.handle_inbound_from_supernode("host".into(), message)
            .await;
        assert!(cm.group_keys.is_behind("room"));
        assert!(!drain(&mut events)
            .iter()
            .any(|e| matches!(e, ConnectionEvent::RoomChatMessage { .. })));
    }

    #[tokio::test]
    async fn room_file_offers_and_requests_from_blocked_people_are_ignored() {
        let (mut cm, mut events, _dir) = receiver();
        let sender = Identity::generate();
        cm.set_identity_blocked(&sender.public_id(), "Member", true)
            .await;
        drain(&mut events);
        cm.handle_inbound_from_supernode("host".into(), signed(&sender, MessageType::SfuFileOffer,
            json!({ "room_id": "room", "transfer_id": "blocked-file", "sha256": "hash", "size": 12, "total_chunks": 1 }))).await;
        assert!(!cm.room_file_mgr.has_inbound("blocked-file"));
        let (offer, _) = cm
            .room_file_mgr
            .offer_file("room", "file", vec![1; 12], "room_file", None, false)
            .unwrap();
        cm.handle_inbound_from_supernode(
            "host".into(),
            signed(
                &sender,
                MessageType::SfuFileRequest,
                json!({ "room_id": "room", "transfer_id": offer, "to": cm.identity.public_id() }),
            ),
        )
        .await;
        assert!(cm.room_file_mgr.active_outbound_streams().is_empty());
        assert!(!drain(&mut events)
            .iter()
            .any(|e| matches!(e, ConnectionEvent::FileOffered { .. })));
    }
}
