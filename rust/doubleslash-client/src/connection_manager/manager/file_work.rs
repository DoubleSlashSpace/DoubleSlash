//! File hashing kept off the manager loop.
//!
//! Offering a file hashes all of it, and a streamed download is hashed again
//! once every chunk is on disk. At 4 GiB either takes seconds, and this loop
//! also carries signaling and voice, so both run on a blocking thread and post
//! their result back as an [`InternalEvent`].

use std::path::PathBuf;

use tracing::{info, warn};

use crate::file_transfer::{self, PreparedOffer, INLINE_MAX};

use super::super::events::ConnectionEvent;
use super::super::internal::{InternalEvent, OfferTarget};
use super::ConnectionManager;

impl ConnectionManager {
    /// Hash a file the user is sending; [`Self::offer_prepared`] offers it.
    pub(super) fn prepare_offer(
        &mut self,
        target: OfferTarget,
        rel_path: String,
        path: String,
        transfer_id: String,
        purpose: String,
    ) {
        // Small 1:1 files keep the in-RAM path so compression and delta still
        // apply; large ones, and every room file, stream from disk. Room
        // offers are always path-backed so a late pull can re-read them.
        let inline_allowed = matches!(target, OfferTarget::Peer { .. });
        let old_data = if inline_allowed {
            self.file_mgr.get_old_data(&rel_path)
        } else {
            None
        };
        self.offers_in_preparation.insert(transfer_id.clone());
        let internal_tx = self.internal_tx.clone();
        let (rel, tid, kind) = (rel_path.clone(), transfer_id.clone(), purpose.clone());
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let src = PathBuf::from(&path);
                let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
                if inline_allowed && size <= INLINE_MAX as u64 {
                    let data = std::fs::read(&src)
                        .map_err(|e| format!("cannot read {}: {e}", src.display()))?;
                    file_transfer::prepare_inline_offer(
                        &rel,
                        data,
                        &kind,
                        old_data.as_deref(),
                        Some(&tid),
                    )
                } else {
                    file_transfer::prepare_path_offer(&src, &kind, Some(&tid))
                }
            })
            .await
            .unwrap_or_else(|e| Err(format!("preparing the offer failed: {e}")));
            let _ = internal_tx
                .send(InternalEvent::OfferPrepared {
                    target,
                    rel_path,
                    transfer_id,
                    purpose,
                    result,
                })
                .await;
        });
    }

    /// Record and advertise an offer [`Self::prepare_offer`] built, unless the
    /// user withdrew it while it was being hashed.
    pub(super) async fn offer_prepared(
        &mut self,
        target: OfferTarget,
        rel_path: String,
        transfer_id: String,
        purpose: String,
        result: Result<PreparedOffer, String>,
    ) {
        if !self.offers_in_preparation.remove(&transfer_id) {
            info!(
                "[file] transfer {} was withdrawn before it was offered",
                &transfer_id[..8.min(transfer_id.len())]
            );
            return;
        }
        match target {
            OfferTarget::Peer { peer_id } => {
                let offer = match result {
                    Ok(offer) => offer,
                    Err(e) => {
                        warn!("SendFile error: {e}");
                        return;
                    }
                };
                let size = offer.size();
                let (transfer_id, evs) = self
                    .file_mgr
                    .insert_prepared(&peer_id, &rel_path, offer, false);
                self.emit_event(ConnectionEvent::FileOffered {
                    transfer_id,
                    peer_id,
                    rel_path,
                    size,
                    purpose,
                    is_self: true,
                    origin_id: self.identity.public_id(),
                    supernode_id: String::new(),
                });
                self.dispatch_transfer_events(evs).await;
            }
            OfferTarget::Room {
                supernode_id,
                room_id,
            } => {
                let offer = match result {
                    Ok(offer) => offer,
                    Err(e) => {
                        warn!("SendSfuFile error: {e}");
                        return;
                    }
                };
                let size = offer.size();
                // Advertisement only: `auto_push = false`. Room files used to
                // broadcast every chunk immediately, so every member
                // downloaded every file whether they wanted it or not. Chunks
                // now flow only to peers who answer the offer with an
                // SfuFileRequest, re-read from this path.
                let (transfer_id, evs) = self
                    .room_file_mgr
                    .insert_prepared(&room_id, &rel_path, offer, false);
                self.emit_event(ConnectionEvent::FileOffered {
                    transfer_id,
                    peer_id: room_id.clone(),
                    rel_path,
                    size,
                    purpose,
                    is_self: true,
                    origin_id: self.identity.public_id(),
                    supernode_id: supernode_id.clone(),
                });
                self.dispatch_room_transfer_events(evs, &supernode_id, &room_id)
                    .await;
            }
        }
    }

    /// Stop an offer that is still being hashed from ever going out.
    ///
    /// Returns false when `transfer_id` is not being prepared.
    pub(super) fn withdraw_offer_in_preparation(&mut self, transfer_id: &str) -> bool {
        self.offers_in_preparation.remove(transfer_id)
    }

    /// Hash a finished streamed download; [`Self::received_file_hashed`]
    /// publishes it.
    pub(super) fn hash_received_file(&self, transfer_id: String, path: PathBuf, room: bool) {
        let internal_tx = self.internal_tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || file_transfer::sha256_hex_file(&path))
                .await
                .unwrap_or_else(|e| Err(format!("hashing the received file failed: {e}")));
            let _ = internal_tx
                .send(InternalEvent::FileHashed {
                    transfer_id,
                    room,
                    result,
                })
                .await;
        });
    }

    pub(super) async fn received_file_hashed(
        &mut self,
        transfer_id: String,
        room: bool,
        result: Result<String, String>,
    ) {
        if room {
            // The transfer record carries its room and supernode.
            let evs = self.room_file_mgr.on_file_hashed(&transfer_id, result);
            self.dispatch_room_transfer_events(evs, "", "").await;
        } else {
            let evs = self.file_mgr.on_file_hashed(&transfer_id, result);
            self.dispatch_transfer_events(evs).await;
        }
    }
}
