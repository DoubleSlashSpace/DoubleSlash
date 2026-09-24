//! Video in both directions.
//!
//! Out: CameraX frames in, encoded VP8 out. The core already owns the hard
//! parts — capture pacing, encoding, adaptive bitrate, fragmentation. This
//! wires the Android camera into [`VideoSender`] and points the encoded output
//! at either a direct peer or a room.
//!
//! In: [`Inbound`] owns the core's [`VideoReceiver`] and the set of peers the
//! user chose to watch. Watching is opt-in, as on the desktop: the supernode is
//! told to forward only those senders, and the decode thread exists only while
//! at least one is watched, so a phone in a busy room pays for the pictures it
//! asked for and nothing else.

use doubleslash_client::call_controller::CallCommand;
use doubleslash_client::connection_manager::ConnectionCommand;
use doubleslash_client::video::codec::{
    make_decoder, make_realtime_encoder, pick_direct_codec, pick_room_codec, realtime_fallback,
    EncoderParams,
};
use doubleslash_client::video::receiver::VideoReceiver;
use doubleslash_client::video::sender::{
    CaptureLayout, FrameSink, Quality, SourceSpec, VideoSender,
};
use doubleslash_features::video_codec::VideoCodec;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::sink::EventSink;
use doubleslash_client::video::sink::RenderSink;

/// Where encoded frames go.
enum Target {
    /// A direct 1:1 call.
    Peer(String),
    /// The room the local user is in. The supernode fans it out.
    Room,
}

/// Adapts the core's `FrameSink` onto the connection manager's command channel.
struct CommandSink {
    tx: mpsc::Sender<ConnectionCommand>,
    target: Target,
}

impl FrameSink for CommandSink {
    fn send(&mut self, encoded: Vec<u8>, keyframe: bool, codec: VideoCodec, pts_us: u64) {
        // `try_send`, never `blocking_send`: this runs on the capture thread,
        // and blocking it would stall the camera rather than the network. A
        // dropped frame on a saturated link is the correct outcome — the
        // encoder will be asked for a keyframe if the far end falls apart.
        let command = match &self.target {
            Target::Peer(peer_id) => ConnectionCommand::SendVideoFrame {
                peer_id: peer_id.clone(),
                encoded,
                keyframe,
                codec,
                pts_us,
            },
            Target::Room => ConnectionCommand::SendRoomVideo {
                encoded,
                keyframe,
                codec,
                pts_us,
            },
        };

        if self.tx.try_send(command).is_err() {
            // Not warn-per-frame: on a congested link this fires continuously
            // and would bury the log the way the E2E path already does.
            tracing::trace!("[video] dropped an encoded frame; command channel full");
        }
    }
}

/// The codec a send starts with, and what it may drop to if the phone cannot
/// keep up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecChoice {
    pub codec: VideoCodec,
    pub fallback: Option<VideoCodec>,
}

/// Pick the codec for a send, and what it may fall back to.
///
/// A room takes the room default — VP9 — with VP8 behind it for a phone that
/// cannot keep up. A direct call negotiates from the peer's announced codecs,
/// as the desktop does; a peer that has not announced yet is sent the room
/// default rather than nothing.
pub fn choose_codec(peer_codecs: Option<&[VideoCodec]>) -> anyhow::Result<CodecChoice> {
    let codec = match peer_codecs {
        Some(theirs) => pick_direct_codec(theirs, None).ok_or_else(|| {
            anyhow::anyhow!("no video codec in common with this peer (they speak {theirs:?})")
        })?,
        None => pick_room_codec(None)
            .ok_or_else(|| anyhow::anyhow!("this build has no video encoder"))?,
    };
    let fallback = realtime_fallback(codec).filter(|f| {
        // A peer that told us what it decodes gets the fallback only if it is
        // on that list.
        peer_codecs.is_none_or(|theirs| theirs.is_empty() || theirs.contains(f))
    });
    Ok(CodecChoice { codec, fallback })
}

/// Start capturing and sending local video.
///
/// `peer_id` sends to one peer; `None` sends into the current room. The camera
/// itself is driven by CameraX on the Kotlin side — this only opens the
/// consumer end, and [`VideoSender`] blocks until the first frame arrives.
pub fn start(
    tx: mpsc::Sender<ConnectionCommand>,
    peer_id: Option<String>,
    CodecChoice { codec, fallback }: CodecChoice,
    device_id: &str,
    quality_preset: &str,
    slot: Arc<RwLock<Option<VideoSender>>>,
    sink: EventSink,
) -> anyhow::Result<VideoSender> {
    let quality = Quality::from_name(quality_preset);
    let layout = CaptureLayout::single(SourceSpec::from_device_id(device_id));

    // The encoder is sized from the quality preset rather than the camera,
    // because it has to be built before the first frame exists. The capture
    // loop scales frames to match.
    let encoder = make_realtime_encoder(
        codec,
        EncoderParams {
            width: quality.width,
            height: quality.height,
            bitrate_bps: quality.bitrate_bps,
            fps: quality.fps,
            keyframe_interval_secs: quality.keyframe_interval_secs,
        },
        fallback,
    )?;

    let target = match &peer_id {
        Some(id) => Target::Peer(id.clone()),
        None => Target::Room,
    };
    let frame_sink = CommandSink {
        tx: tx.clone(),
        target,
    };

    info!(
        "[video] starting Android capture ({} {}x{} @ {}fps, {} kbps) -> {}",
        codec.as_str(),
        quality.width,
        quality.height,
        quality.fps,
        quality.bitrate_bps / 1000,
        peer_id.as_deref().unwrap_or("room"),
    );

    Ok(VideoSender::start(
        layout,
        quality,
        encoder,
        frame_sink,
        None,
        None,
        move |end| {
            // A capture can die on its own: CameraX unbinds when the activity
            // backgrounds, and the camera can be revoked outright. Clearing
            // the slot is what makes video restartable - leaving a dead
            // `VideoSender` in it means every later `video.start` is refused
            // as "already running", with no way back short of restarting the
            // app.
            warn!("[video] capture ended: {end:?}");
            *slot.write() = None;

            // Tell the UI, or the button stays on "Stop video" for a capture
            // that is no longer running.
            let payload = serde_json::json!({
                "event": "video_ended",
                "reason": format!("{end:?}"),
            });
            match sink.attach() {
                Ok(mut guard) => sink.emit(&mut guard, &payload.to_string()),
                Err(e) => warn!("[video] could not report capture end: {e}"),
            }
        },
    ))
}

/// Inbound video: the watched set and the decoder serving it.
#[derive(Default)]
pub struct Inbound {
    receiver: Option<VideoReceiver>,
    /// Peer ids as the UI named them — the spelling the supernode matches
    /// subscriptions against, so they are passed on unchanged.
    watched: Vec<String>,
}

pub type SharedInbound = Arc<Mutex<Inbound>>;

impl Inbound {
    /// Queue a received frame. Dropped unless something is being watched,
    /// before it costs a copy or a place in the decode queue.
    pub fn submit(
        &self,
        peer_id: &str,
        encoded: &[u8],
        keyframe: bool,
        codec: VideoCodec,
        pts_us: u64,
    ) {
        if let Some(receiver) = &self.receiver {
            receiver.submit(peer_id, encoded.to_vec(), keyframe, codec, pts_us);
        }
    }

    /// A peer's camera went off: blank their tile and free their decoder now
    /// rather than when the stream cap next needs the slot.
    pub fn forget(&self, peer_id: &str) {
        if let Some(receiver) = &self.receiver {
            receiver.forget(peer_id);
        }
    }

    /// Take the receiver out, for a shutdown that must not hold the lock
    /// while the decode thread joins.
    pub fn take_receiver(&mut self) -> Option<VideoReceiver> {
        self.watched.clear();
        self.receiver.take()
    }
}

/// What a peer id compares as. See `render::normalise`.
fn same_peer(a: &str, b: &str) -> bool {
    a.trim_end_matches('=') == b.trim_end_matches('=')
}

/// Ids in `next` that are not in `previous`.
fn added<'a>(previous: &[String], next: &'a [String]) -> Vec<&'a String> {
    next.iter()
        .filter(|id| !previous.iter().any(|p| same_peer(p, id)))
        .collect()
}

/// Replace the set of watched peers.
///
/// Tells the supernode which room senders to forward, asks every newly watched
/// sender for a keyframe — they were not being forwarded a moment ago, so the
/// next frames to arrive are mid-GOP and undecodable — and starts or stops the
/// decode thread as the set becomes non-empty or empty.
pub fn watch(
    inbound: &SharedInbound,
    peers: Vec<String>,
    cmd_tx: &mpsc::Sender<ConnectionCommand>,
    call_tx: &mpsc::Sender<CallCommand>,
    events: &EventSink,
) {
    let mut unique: Vec<String> = Vec::with_capacity(peers.len());
    for id in peers {
        if !id.is_empty() && !unique.iter().any(|u| same_peer(u, &id)) {
            unique.push(id);
        }
    }
    let peers = unique;

    let retired = {
        let mut state = inbound.lock();
        let newly_watched: Vec<String> =
            added(&state.watched, &peers).into_iter().cloned().collect();
        let dropped: Vec<String> = added(&peers, &state.watched).into_iter().cloned().collect();
        state.watched = peers.clone();

        for id in newly_watched {
            let _ = cmd_tx.try_send(ConnectionCommand::RequestVideoKeyframe { peer_id: id });
        }

        if peers.is_empty() {
            state.receiver.take()
        } else {
            match &state.receiver {
                Some(receiver) => {
                    for id in &dropped {
                        receiver.forget(id);
                    }
                }
                None => {
                    let receiver = start_receiver(cmd_tx.clone(), events.clone());
                    // The call controller anchors video against shared audio
                    // from its playout tick, so it needs the same hold state.
                    let _ = call_tx.try_send(CallCommand::SetVideoPlayout(receiver.playout()));
                    state.receiver = Some(receiver);
                }
            }
            None
        }
    };

    // Whole set, every time: the manager suppresses a repeat within one room,
    // and a room the phone just joined needs to hear it even when it is empty,
    // or its supernode forwards every camera by default.
    let _ = cmd_tx.try_send(ConnectionCommand::SetVideoSubscriptions { senders: peers });

    // Outside the lock: dropping the receiver joins its decode thread.
    if let Some(receiver) = retired {
        info!("[video] nobody watched; stopping the decode thread");
        receiver.forget_all();
        drop(receiver);
    }
}

fn start_receiver(cmd_tx: mpsc::Sender<ConnectionCommand>, events: EventSink) -> VideoReceiver {
    let stall_events = events.clone();
    VideoReceiver::start(
        |codec| {
            make_decoder(codec)
                .map_err(|e| warn!("[video] no {} decoder: {e}", codec.as_str()))
                .ok()
        },
        move |peer_id| {
            let _ = cmd_tx.try_send(ConnectionCommand::RequestVideoKeyframe {
                peer_id: peer_id.to_owned(),
            });
        },
        move |peer_id, stalled| {
            // Edges only, so a JVM attach per report is fine here.
            let payload = serde_json::json!({
                "event": "peer_video_stalled",
                "peer_id": peer_id.trim_end_matches('='),
                "stalled": stalled,
            });
            match stall_events.attach() {
                Ok(mut env) => stall_events.emit(&mut env, &payload.to_string()),
                Err(e) => warn!("[video] could not report a stall: {e}"),
            }
        },
        render_sink(events),
    )
}

/// Where received frames are drawn: the `SurfaceView`s Kotlin attaches.
#[cfg(any(target_os = "android", test))]
fn render_sink(events: EventSink) -> Arc<dyn RenderSink> {
    crate::render::SurfaceSinks::new(events)
}

/// A host build has no surfaces. The core's desktop sink is a no-op without Qt
/// Multimedia — it watches nobody, so the decode thread skips every frame.
#[cfg(not(any(target_os = "android", test)))]
fn render_sink(_events: EventSink) -> Arc<dyn RenderSink> {
    Arc::new(doubleslash_client::video::sink::QtSinks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn padding_does_not_make_a_peer_new() {
        let previous = ids(&["AbCd=="]);
        let next = ids(&["AbCd", "EfGh"]);
        assert_eq!(added(&previous, &next), vec!["EfGh"]);
        assert!(added(&next, &previous).is_empty());
    }

    #[test]
    fn a_room_sends_vp9_with_vp8_behind_it() {
        let choice = choose_codec(None).unwrap();
        assert_eq!(choice.codec, VideoCodec::Vp9);
        assert_eq!(choice.fallback, Some(VideoCodec::Vp8));
    }

    /// A peer that has not announced yet gets the room default, not a refusal.
    #[test]
    fn an_unannounced_peer_is_sent_the_default() {
        let choice = choose_codec(Some(&[])).unwrap();
        assert_eq!(choice.codec, VideoCodec::Vp9);
        assert_eq!(choice.fallback, Some(VideoCodec::Vp8));
    }

    /// An older desktop that only speaks VP8 is sent VP8, and there is nothing
    /// to fall back to.
    #[test]
    fn a_vp8_only_peer_is_sent_vp8() {
        let choice = choose_codec(Some(&[VideoCodec::Vp8])).unwrap();
        assert_eq!(choice.codec, VideoCodec::Vp8);
        assert_eq!(choice.fallback, None);
    }

    /// The fallback must itself be something the peer said it decodes.
    #[test]
    fn a_peer_without_vp8_gets_no_fallback() {
        let choice = choose_codec(Some(&[VideoCodec::Vp9])).unwrap();
        assert_eq!(choice.codec, VideoCodec::Vp9);
        assert_eq!(choice.fallback, None);
    }

    #[test]
    fn a_peer_with_nothing_in_common_is_refused() {
        // The stub is never advertised by a real build, so nothing speaks it.
        assert!(choose_codec(Some(&[VideoCodec::Stub])).is_err());
    }

    #[test]
    fn removals_are_what_the_old_set_has_and_the_new_lacks() {
        let previous = ids(&["alice", "bob"]);
        let next = ids(&["bob"]);
        assert_eq!(added(&next, &previous), vec!["alice"]);
    }
}
