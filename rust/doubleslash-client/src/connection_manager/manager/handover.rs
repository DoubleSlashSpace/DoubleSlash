//! Carrying a voice room through a change of network.
//!
//! When the platform reports that the device moved to another network (Wi-Fi
//! to cellular, one Wi-Fi to another), every WebSocket is redialed, because a
//! TCP connection cannot change its address. QUIC connections can: the endpoint
//! is rebound so each of them announces the new path at once (see
//! [`crate::quic_tls::move_to_new_network`]), and the supernode keeps a voice
//! member whose WebSocket is replaced by the same device.
//!
//! What is left is the client's own reaction to the WebSocket dropping. For a
//! short while after a move, that drop is a handover, not a loss: the voice
//! room and every open relay connection are kept, so voice keeps flowing over
//! the relay while signaling redials. When the room's node is back, the join
//! is sent again in case the node let us go meanwhile, and a relay that has
//! heard nothing from its node since the move is replaced. A node that does
//! not come back in time is treated as lost, exactly as it would have been
//! without the handover, so cluster failover still happens.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use super::ConnectionManager;

/// How long after a network move a dropped WebSocket still counts as part of
/// it.
///
/// Long enough to cover the redial (immediate, after a short settle) on a slow
/// new network. Short enough that a node which really did go down in the same
/// moment is failed over from without a noticeable extra wait.
pub(super) const NETWORK_HANDOVER_GRACE: Duration = Duration::from_secs(15);

/// State of a network move in progress.
pub(super) struct NetworkHandover {
    /// Past this, a node that has not reconnected is treated as lost.
    deadline: Instant,
    /// Datagrams each open relay had received when the move was announced.
    ///
    /// A relay still open on a stranded path looks healthy to quinn until its
    /// idle timeout, two minutes away. One that has received nothing since the
    /// move by the time its node's WebSocket is back did not follow the move.
    relay_rx_at_move: HashMap<String, u64>,
    /// The voice room whose node's WebSocket dropped during the move:
    /// `(node, room)`.
    voice: Option<(String, String)>,
}

impl ConnectionManager {
    /// The platform reported a network move: probe every QUIC connection on
    /// the new path and open (or extend) the handover window.
    pub(super) fn begin_network_handover(&mut self) {
        if let (Some(endpoint), Some(socket)) = (&self.quic_endpoint, &self.quic_socket) {
            match crate::quic_tls::move_to_new_network(endpoint, socket) {
                Ok(()) => info!("[handover] QUIC connections told to move to the new network"),
                // Not fatal: each connection still moves at its next send or
                // keep-alive, just later.
                Err(e) => warn!("[handover] could not rebind the QUIC endpoint: {e}"),
            }
        }
        let relay_rx_at_move = self
            .quic_relays
            .iter()
            .filter(|(_, relay)| relay.is_alive())
            .map(|(id, relay)| (id.clone(), relay.datagrams_received()))
            .collect();
        // A second move before the first settled keeps the room it is carrying.
        let voice = self.network_handover.take().and_then(|h| h.voice);
        self.network_handover = Some(NetworkHandover {
            deadline: Instant::now() + NETWORK_HANDOVER_GRACE,
            relay_rx_at_move,
            voice,
        });
    }

    /// `supernode_id`'s WebSocket dropped. Returns whether that is part of a
    /// network move, in which case the caller keeps the voice room and any
    /// live relay to the node instead of tearing them down.
    pub(super) fn hold_through_network_handover(&mut self, supernode_id: &str) -> bool {
        let Some(handover) = self
            .network_handover
            .as_mut()
            .filter(|h| Instant::now() < h.deadline)
        else {
            return false;
        };
        if self.current_supernode_id == supernode_id && !self.current_room_id.is_empty() {
            info!(
                "[handover] keeping voice room {} while node {} redials",
                &self.current_room_id[..12.min(self.current_room_id.len())],
                &supernode_id[..12.min(supernode_id.len())]
            );
            handover.voice = Some((supernode_id.to_owned(), self.current_room_id.clone()));
        }
        true
    }

    /// `supernode_id`'s WebSocket is back. Replace its relay if that did not
    /// follow the move, and send the voice room's join again if this node
    /// hosts it.
    pub(super) async fn finish_network_handover_for(&mut self, supernode_id: &str) {
        let Some(handover) = self.network_handover.as_mut() else {
            return;
        };
        let heard_before = handover.relay_rx_at_move.remove(supernode_id);
        let voice = match &handover.voice {
            Some((node, _)) if node == supernode_id => handover.voice.take(),
            _ => None,
        };
        if handover.voice.is_none() && handover.relay_rx_at_move.is_empty() {
            self.network_handover = None;
        }

        if let Some(before) = heard_before {
            let stranded = self
                .quic_relays
                .get(supernode_id)
                .is_some_and(|relay| relay.is_alive() && relay.datagrams_received() <= before);
            if stranded {
                warn!(
                    "[handover] relay to {} heard nothing since the move; replacing it",
                    &supernode_id[..12.min(supernode_id.len())]
                );
                if let Some(relay) = self.quic_relays.remove(supernode_id) {
                    relay.close();
                }
            }
        }

        let Some((node, room_id)) = voice else {
            return;
        };
        if self.current_room_id != room_id {
            return; // Left the room while the node was away.
        }
        info!(
            "[handover] node {} is back; confirming voice room {}",
            &node[..12.min(node.len())],
            &room_id[..12.min(room_id.len())]
        );
        // Idempotent on the node when it kept us, which it does when the same
        // device's new WebSocket replaces the old one. If it let us go (its
        // side of the old socket failed first), this puts us back.
        self.send_room_join(&node, &room_id).await;
        self.ensure_room_relay(&node).await;
        // A node that let us go has also forgotten our camera and the senders
        // we watch; it would forward everyone until told otherwise.
        self.reannounce_video_state(&room_id).await;
        self.resend_video_subscriptions().await;
    }

    /// Test-only: close the handover window as if its grace had run out.
    #[cfg(test)]
    pub(super) fn test_end_network_handover_window(&mut self) {
        if let Some(handover) = self.network_handover.as_mut() {
            handover.deadline = Instant::now();
        }
    }

    /// Past the window, treat every node that has not come back as lost, as
    /// its dropped WebSocket would have been without the handover.
    pub(super) async fn expire_network_handover(&mut self) {
        if self
            .network_handover
            .as_ref()
            .is_none_or(|h| Instant::now() < h.deadline)
        {
            return;
        }
        let Some(handover) = self.network_handover.take() else {
            return;
        };
        let is_connected =
            |cm: &Self, id: &str| cm.supernodes.get(id).is_some_and(|sn| sn.connected);
        // Every relay the handover kept for a node still away, whether it was
        // open at the move or finished connecting during it.
        let away: Vec<String> = self
            .quic_relays
            .keys()
            .filter(|id| !is_connected(self, id))
            .cloned()
            .collect();
        for id in away {
            if let Some(relay) = self.quic_relays.remove(&id) {
                relay.close();
            }
        }
        let Some((node, room_id)) = handover.voice else {
            return;
        };
        if is_connected(self, &node)
            || self.current_supernode_id != node
            || self.current_room_id != room_id
        {
            return;
        }
        warn!(
            "[handover] node {} did not come back after the network move; leaving room {}",
            &node[..12.min(node.len())],
            &room_id[..12.min(room_id.len())]
        );
        self.current_room_id.clear();
        self.current_supernode_id.clear();
        self.maybe_failover_to_cluster(&node, &room_id).await;
    }
}
