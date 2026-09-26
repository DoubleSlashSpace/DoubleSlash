//! Video codec seam, the codec registry, plus a compression-free stub.
//!
//! [`VideoEncoder`] and [`VideoDecoder`] are the seam; [`make_encoder`] and
//! [`make_decoder`] pick an implementation for a
//! [`VideoCodec`](doubleslash_features::video_codec::VideoCodec), and
//! [`available_codecs`] reports what this build can run so the client
//! advertises that set and nothing more.
//!
//! Peers negotiate over these sets rather than assuming a single codec, because
//! what a build can run is a platform fact: Media Foundation's H.264 exists on
//! Windows and nowhere else. See [`doubleslash_features::video_codec`] for the
//! negotiation rules and why the capability id names no codec.
//!
//! The stub exists so the transport can be proven end to end without a real
//! codec. It deliberately does **no** compression: it packs raw I420 and
//! declares every frame a keyframe. At the 160x120 working size that is 28.8 KB
//! per frame — about 27 fragments — which stresses reassembly *harder* than a
//! real codec will, with zero new dependencies. If a frame arrives corrupt
//! while the stub is in use, the bug is in the transport, not the codec. It is
//! never advertised to a peer.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use doubleslash_features::video_codec::VideoCodec;
use tracing::warn;

use super::frame::RawFrame;

/// Working resolution for the stub codec.
///
/// Small on purpose: without compression a larger frame would exceed
/// [`MAX_FRAGS_PER_FRAME`](super::fragment::MAX_FRAGS_PER_FRAME).
pub const STUB_WIDTH: u32 = 160;
/// See [`STUB_WIDTH`].
pub const STUB_HEIGHT: u32 = 120;

/// The frame size encoders are asked to stay under.
///
/// A quarter of the transport's hard ceiling
/// ([`MAX_ENCODED_FRAME_BYTES`](super::fragment::MAX_ENCODED_FRAME_BYTES)),
/// because encoders treat it as a target and miss it upward: on a detailed
/// 1080p desktop libvpx still produced 2-6x the cap on the first keyframe,
/// which is sized before rate control has seen any content. A cap near the
/// ceiling did nothing at all there. Keyframes are what reach it — a shared
/// desktop is detail everywhere — and each one is a burst of datagrams a slow
/// path absorbs at once, so smaller is also a shorter wait for the picture.
/// What still overshoots the ceiling is handled by the capture loop.
pub const FRAME_SIZE_TARGET_BYTES: usize = super::fragment::MAX_ENCODED_FRAME_BYTES / 4;

/// Bytes of stub header before the planes: `[width:u16 BE][height:u16 BE]`.
const STUB_HEADER_LEN: usize = 4;

/// Encodes raw frames into a compressed (or, for the stub, packed) byte string.
pub trait VideoEncoder: Send {
    /// Encode one frame. Returns the encoded bytes and whether it is a
    /// keyframe — the caller needs the flag for the fragment header so a
    /// receiver can tell whether it can start decoding here.
    fn encode(&mut self, frame: &RawFrame) -> anyhow::Result<(Vec<u8>, bool)>;

    /// Ask for the next frame to be a keyframe, in response to a receiver's
    /// keyframe request. Implementations must treat this as a request for the
    /// *next* encode, never encode one synchronously here.
    fn request_keyframe(&mut self);

    /// Retarget the encoder's average bitrate, for adaptive control.
    ///
    /// Must adjust the *running* encoder rather than rebuild it: a rebuild
    /// resets reference frames, so every rate change would cost a keyframe —
    /// a bandwidth spike at exactly the moment the link is already struggling.
    ///
    /// Defaults to accepting and ignoring the request, which is right for
    /// codecs with no rate control (the stub packs raw I420 at a fixed size).
    fn set_bitrate(&mut self, _bps: u32) -> anyhow::Result<()> {
        Ok(())
    }

    /// The codec this encoder produces.
    ///
    /// Rides every frame's header, so a receiver picks the matching decoder —
    /// and so an encoder that changes codec mid-stream ([`FallbackEncoder`]) is
    /// followed by every receiver without any renegotiation.
    fn codec(&self) -> VideoCodec;
}

/// Forward the trait through a box, so [`make_encoder`]'s
/// `Box<dyn VideoEncoder>` satisfies the generic `E: VideoEncoder` bound that
/// [`VideoSender::start`](super::sender::VideoSender::start) takes. Without
/// this the registry could only be used by callers willing to name a concrete
/// encoder type — which is exactly what picking a codec at runtime rules out.
impl VideoEncoder for Box<dyn VideoEncoder> {
    fn encode(&mut self, frame: &RawFrame) -> anyhow::Result<(Vec<u8>, bool)> {
        (**self).encode(frame)
    }

    fn request_keyframe(&mut self) {
        (**self).request_keyframe()
    }

    fn set_bitrate(&mut self, bps: u32) -> anyhow::Result<()> {
        (**self).set_bitrate(bps)
    }

    fn codec(&self) -> VideoCodec {
        (**self).codec()
    }
}

/// Decodes what a [`VideoEncoder`] produced.
pub trait VideoDecoder: Send {
    /// Decode one encoded frame.
    ///
    /// `Ok(None)` means the frame was **accepted but produced no picture yet**
    /// — a pipelined decoder holds the first submissions while it fills, and a
    /// decoder that has not seen a keyframe yet has nothing it could draw. That
    /// is a normal state, not a failure, and the distinction is load-bearing:
    /// the receiver escalates on `Err` (keyframe request, then rebuilding the
    /// decoder) and would tear a healthy decoder down mid-warm-up if the two
    /// were collapsed into one. See [`super::receiver`].
    fn decode(&mut self, encoded: &[u8]) -> anyhow::Result<Option<RawFrame>>;
}

/// See the [`VideoEncoder`] box forward above.
impl VideoDecoder for Box<dyn VideoDecoder> {
    fn decode(&mut self, encoded: &[u8]) -> anyhow::Result<Option<RawFrame>> {
        (**self).decode(encoded)
    }
}

/// Compression-free encoder: packs I420 behind a 4-byte dimension header.
#[derive(Debug, Default)]
pub struct StubEncoder;

impl VideoEncoder for StubEncoder {
    fn encode(&mut self, frame: &RawFrame) -> anyhow::Result<(Vec<u8>, bool)> {
        if !frame.is_consistent() {
            anyhow::bail!(
                "inconsistent frame: {}x{} with planes {}/{}/{}",
                frame.width,
                frame.height,
                frame.y.len(),
                frame.u.len(),
                frame.v.len()
            );
        }
        let mut out =
            Vec::with_capacity(STUB_HEADER_LEN + frame.y.len() + frame.u.len() + frame.v.len());
        out.extend_from_slice(&(frame.width as u16).to_be_bytes());
        out.extend_from_slice(&(frame.height as u16).to_be_bytes());
        out.extend_from_slice(&frame.y);
        out.extend_from_slice(&frame.u);
        out.extend_from_slice(&frame.v);
        // Every stub frame stands alone, so every frame is a keyframe.
        Ok((out, true))
    }

    fn request_keyframe(&mut self) {
        // No-op: every stub frame is already a keyframe.
    }

    fn codec(&self) -> VideoCodec {
        VideoCodec::Stub
    }
}

/// Counterpart to [`StubEncoder`].
#[derive(Debug, Default)]
pub struct StubDecoder;

impl VideoDecoder for StubDecoder {
    /// Never `Ok(None)`: the stub carries a whole picture in every frame, so
    /// there is no pipeline to fill and nothing to wait for.
    fn decode(&mut self, encoded: &[u8]) -> anyhow::Result<Option<RawFrame>> {
        if encoded.len() < STUB_HEADER_LEN {
            anyhow::bail!("stub frame too short: {} bytes", encoded.len());
        }
        let width = u16::from_be_bytes([encoded[0], encoded[1]]) as u32;
        let height = u16::from_be_bytes([encoded[2], encoded[3]]) as u32;
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            anyhow::bail!("stub frame has bad dimensions {width}x{height}");
        }

        let y_len = (width * height) as usize;
        let c_len = ((width / 2) * (height / 2)) as usize;
        let want = STUB_HEADER_LEN + y_len + 2 * c_len;
        // Checked before slicing: a hostile header must not make us index past
        // the buffer or allocate against a size the payload doesn't back.
        if encoded.len() != want {
            anyhow::bail!(
                "stub frame length {} does not match {width}x{height} (want {want})",
                encoded.len()
            );
        }

        let y_end = STUB_HEADER_LEN + y_len;
        let u_end = y_end + c_len;
        Ok(Some(RawFrame {
            width,
            height,
            y: encoded[STUB_HEADER_LEN..y_end].to_vec(),
            u: encoded[y_end..u_end].to_vec(),
            v: encoded[u_end..].to_vec(),
        }))
    }
}

// ── VP8 and VP9, via vendored libvpx ─────────────────────────────────────────

/// The libvpx codec for a [`VideoCodec`], if libvpx implements it.
fn vpx_codec(codec: VideoCodec) -> Option<doubleslash_vpx::Codec> {
    match codec {
        VideoCodec::Vp8 => Some(doubleslash_vpx::Codec::Vp8),
        VideoCodec::Vp9 => Some(doubleslash_vpx::Codec::Vp9),
        VideoCodec::H264 | VideoCodec::Stub => None,
    }
}

/// VP8 or VP9 encoder.
///
/// These are the codecs that make video work everywhere: Media Foundation
/// H.264 relies on a licence the OS holds and exists only on Windows, while
/// libvpx is royalty-free and built from vendored source on every platform —
/// so every build can both encode and decode both.
pub struct VpxEncoderAdapter {
    inner: doubleslash_vpx::VpxEncoder,
    codec: VideoCodec,
}

impl VpxEncoderAdapter {
    pub fn new(codec: VideoCodec, params: EncoderParams) -> anyhow::Result<Self> {
        let Some(vpx) = vpx_codec(codec) else {
            anyhow::bail!("{} is not a libvpx codec", codec.as_str());
        };
        let config = doubleslash_vpx::EncoderConfig {
            max_frame_bytes: FRAME_SIZE_TARGET_BYTES as u32,
            ..doubleslash_vpx::EncoderConfig::realtime(
                vpx,
                params.width,
                params.height,
                params.bitrate_bps,
                params.fps,
                params.keyframe_interval_secs,
            )
        };
        Ok(Self {
            inner: doubleslash_vpx::VpxEncoder::new(vpx, config)?,
            codec,
        })
    }
}

impl VideoEncoder for VpxEncoderAdapter {
    fn encode(&mut self, frame: &RawFrame) -> anyhow::Result<(Vec<u8>, bool)> {
        if !frame.is_consistent() {
            anyhow::bail!(
                "inconsistent frame: {}x{} with planes {}/{}/{}",
                frame.width,
                frame.height,
                frame.y.len(),
                frame.u.len(),
                frame.v.len()
            );
        }
        let (ew, eh) = self.inner.dimensions();
        if (frame.width, frame.height) != (ew, eh) {
            // libvpx encodes at the size it was built for; a mismatched frame
            // would be read with the wrong stride rather than rescaled.
            anyhow::bail!(
                "frame is {}x{} but the encoder was built for {ew}x{eh}",
                frame.width,
                frame.height
            );
        }
        self.inner.encode(&frame.y, &frame.u, &frame.v)
    }

    fn request_keyframe(&mut self) {
        self.inner.request_keyframe();
    }

    fn set_bitrate(&mut self, bps: u32) -> anyhow::Result<()> {
        self.inner.set_bitrate(bps)
    }

    fn codec(&self) -> VideoCodec {
        self.codec
    }
}

/// VP8 or VP9 decoder. See [`VpxEncoderAdapter`].
pub struct VpxDecoderAdapter(doubleslash_vpx::VpxDecoder);

impl VpxDecoderAdapter {
    pub fn new(codec: VideoCodec) -> anyhow::Result<Self> {
        let Some(vpx) = vpx_codec(codec) else {
            anyhow::bail!("{} is not a libvpx codec", codec.as_str());
        };
        Ok(Self(doubleslash_vpx::VpxDecoder::new(vpx)?))
    }
}

impl VideoDecoder for VpxDecoderAdapter {
    /// libvpx decodes each packet on the call that submits it. A packet that
    /// decodes without a picture to show is the "not yet" answer.
    fn decode(&mut self, encoded: &[u8]) -> anyhow::Result<Option<RawFrame>> {
        Ok(self.0.decode(encoded)?.map(|f| RawFrame {
            width: f.width,
            height: f.height,
            y: f.y,
            u: f.u,
            v: f.v,
        }))
    }
}

// ── Falling back when a device cannot keep up ──────────────────────────────

/// Frames skipped before judging encode time: the opening keyframe and the
/// rate controller settling are not what a call spends its time doing.
const BUDGET_WARMUP_FRAMES: u32 = 30;

/// Frames judged together. Two seconds at 30 fps: long enough that one busy
/// moment elsewhere on the device does not condemn the codec, short enough that
/// a device genuinely too slow is rescued before the call has visibly suffered.
const BUDGET_WINDOW_FRAMES: usize = 60;

/// Mean encode time, as a share of the frame interval, above which the codec
/// is too slow. Not 1.0: the capture thread also converts, previews and sends
/// each frame, and the voice pipeline needs the CPU too.
const BUDGET_MEAN_SHARE: f64 = 0.7;

/// Share of frames in the window that may take longer than a whole frame
/// interval. A codec that is fine on average but regularly blows the interval
/// still stutters.
const BUDGET_LATE_SHARE: f64 = 0.25;

/// Judges encode times against the frame interval.
///
/// Pure bookkeeping, so the policy is testable without a slow encoder.
pub struct EncodeBudget {
    interval: Duration,
    window: VecDeque<Duration>,
    seen: u32,
}

impl EncodeBudget {
    pub fn new(fps: u32) -> Self {
        Self {
            interval: Duration::from_micros(1_000_000 / u64::from(fps.max(1))),
            window: VecDeque::with_capacity(BUDGET_WINDOW_FRAMES),
            seen: 0,
        }
    }

    /// Record one encode. Returns true once a full window shows the codec
    /// cannot keep up.
    pub fn record(&mut self, took: Duration) -> bool {
        self.seen = self.seen.saturating_add(1);
        if self.seen <= BUDGET_WARMUP_FRAMES {
            return false;
        }
        if self.window.len() == BUDGET_WINDOW_FRAMES {
            self.window.pop_front();
        }
        self.window.push_back(took);
        if self.window.len() < BUDGET_WINDOW_FRAMES {
            return false;
        }

        let n = self.window.len() as f64;
        let mean = self.window.iter().map(Duration::as_secs_f64).sum::<f64>() / n;
        let late = self.window.iter().filter(|t| **t > self.interval).count() as f64;
        let interval = self.interval.as_secs_f64();
        mean > interval * BUDGET_MEAN_SHARE || late > n * BUDGET_LATE_SHARE
    }
}

/// An encoder that swaps to a cheaper codec when this device cannot run the
/// one it started with in real time.
///
/// VP9 is the room default because on the hardware measured it costs no more
/// than VP8 and looks better. Older phones and SIMD-less desktops can still
/// fall short, and a codec that cannot keep up is worse than a weaker one that
/// can: frames arrive late, the capture clock slips, and the picture stutters.
/// So encode time is watched, and past [`EncodeBudget`]'s limits the encoder is
/// replaced — once, for the rest of the session, since a device that could not
/// keep up a moment ago is not going to start now, and flapping would cost a
/// keyframe each way.
///
/// The swap needs nothing from receivers beyond what they already do: each
/// frame carries its codec, a new encoder's first frame is a keyframe, and a
/// receiver rebuilds its decoder when a sender's codec changes.
pub struct FallbackEncoder {
    current: Box<dyn VideoEncoder>,
    /// Consumed by the swap.
    fallback: Option<(VideoCodec, EncoderParams)>,
    budget: EncodeBudget,
    /// Latest rate asked for, so the replacement starts where adaptation left
    /// the stream rather than back at the preset.
    bitrate_bps: u32,
}

impl FallbackEncoder {
    pub fn new(
        primary: Box<dyn VideoEncoder>,
        fallback: VideoCodec,
        params: EncoderParams,
    ) -> Self {
        Self {
            current: primary,
            budget: EncodeBudget::new(params.fps),
            bitrate_bps: params.bitrate_bps,
            fallback: Some((fallback, params)),
        }
    }

    /// Account for one encode, swapping codec if the budget says so.
    fn after_encode(&mut self, took: Duration) {
        if self.fallback.is_none() || !self.budget.record(took) {
            return;
        }
        let Some((codec, params)) = self.fallback.take() else {
            return;
        };
        let params = EncoderParams {
            bitrate_bps: self.bitrate_bps,
            ..params
        };
        let from = self.current.codec();
        match make_encoder(codec, params) {
            Ok(replacement) => {
                warn!(
                    "[video] this device cannot encode {} in real time; switching to {}",
                    from.as_str(),
                    codec.as_str()
                );
                self.current = replacement;
            }
            // Keep going on the slow codec: late frames beat no frames.
            Err(e) => warn!(
                "[video] {} is too slow here, but no {} encoder could be built: {e}",
                from.as_str(),
                codec.as_str()
            ),
        }
    }
}

impl VideoEncoder for FallbackEncoder {
    fn encode(&mut self, frame: &RawFrame) -> anyhow::Result<(Vec<u8>, bool)> {
        let started = Instant::now();
        let out = self.current.encode(frame);
        self.after_encode(started.elapsed());
        out
    }

    fn request_keyframe(&mut self) {
        self.current.request_keyframe();
    }

    fn set_bitrate(&mut self, bps: u32) -> anyhow::Result<()> {
        self.bitrate_bps = bps;
        self.current.set_bitrate(bps)
    }

    fn codec(&self) -> VideoCodec {
        self.current.codec()
    }
}

/// The codec to drop to if `codec` proves too slow for this device.
///
/// Only VP9 has one. VP8 adapts its own effort to the CPU it finds, so it is
/// the floor every build can run; H.264 is Media Foundation's hardware path,
/// where encode time is not the CPU's problem.
pub fn realtime_fallback(codec: VideoCodec) -> Option<VideoCodec> {
    (codec == VideoCodec::Vp9).then_some(VideoCodec::Vp8)
}

/// [`make_encoder`], wrapped in a [`FallbackEncoder`] when `fallback` names a
/// different codec.
pub fn make_realtime_encoder(
    codec: VideoCodec,
    params: EncoderParams,
    fallback: Option<VideoCodec>,
) -> anyhow::Result<Box<dyn VideoEncoder>> {
    let primary = make_encoder(codec, params)?;
    Ok(match fallback.filter(|f| *f != codec) {
        Some(f) => Box::new(FallbackEncoder::new(primary, f, params)),
        None => primary,
    })
}

// ── Codec registry ──────────────────────────────────────────────────────────

/// Codec-neutral encoder settings.
///
/// Mirrors the fields every encoder needs, so callers configure a codec without
/// naming one. Codec-specific tuning stays inside the implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderParams {
    /// Encoded frame width in pixels.
    pub width: u32,
    /// Encoded frame height in pixels.
    pub height: u32,
    /// Target average bitrate. Encoders that support rate control retarget a
    /// running encoder via [`VideoEncoder::set_bitrate`] rather than rebuilding.
    pub bitrate_bps: u32,
    /// Target frame rate.
    pub fps: u32,
    /// Maximum seconds between keyframes.
    pub keyframe_interval_secs: u32,
}

/// Codecs this build can encode **and** decode.
///
/// Both directions, deliberately: negotiation produces one codec used for the
/// whole session, so advertising a codec we can only decode would let a peer
/// pick something we cannot send.
///
/// This is a *build* capability, not a live probe. Constructing a Media
/// Foundation encoder allocates COM objects and can fail on a machine with no
/// usable MFT, and doing that at startup to answer "what do we advertise" would
/// cost every launch. A runtime failure still soft-fails the camera toggle —
/// what this function exists to prevent is the categorically worse case of
/// advertising a codec this binary has no implementation of at all.
// `vec_init_then_push` fires on non-Windows, where the `cfg` block below is
// empty and the first push therefore follows `Vec::new()` directly. Collapsing
// it into a `vec![]` literal would mean one literal per platform, which is what
// this shape exists to avoid as macOS and its VideoToolbox entry arrive.
#[allow(clippy::vec_init_then_push)]
pub fn available_codecs() -> Vec<VideoCodec> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        // Media Foundation H.264, using the codec licence held by the OS.
        // First because it is the hardware path where both peers have it.
        out.push(VideoCodec::H264);
    }
    // VP9 and VP8 are built from vendored libvpx on every platform, so they
    // are always available. That is deliberate rather than incidental: it is
    // what gives every pair of peers, and every room, codecs in common.
    out.push(VideoCodec::Vp9);
    out.push(VideoCodec::Vp8);
    out
}

// ── Choosing a codec ────────────────────────────────────────────────────────

/// Pull the video codec set out of a peer's `CAPABILITY_ANNOUNCE` payload.
///
/// Reads `core.video.v1` — the direct-call descriptor. Room video is not
/// negotiated pairwise (see [`pick_room_codec`]), so its advertised set
/// is not consulted here.
///
/// A peer with no video capability, or one advertising no codecs, yields an
/// empty set, which negotiates to "send no video" rather than to a default.
pub fn peer_codecs_from_caps_json(
    caps_json: &str,
) -> Vec<doubleslash_features::video_codec::VideoCodec> {
    let Ok(parsed) =
        serde_json::from_str::<Vec<doubleslash_features::CapabilityDescriptor>>(caps_json)
    else {
        return Vec::new();
    };
    parsed
        .iter()
        .find(|c| c.id == "core.video.v1")
        .map(|c| doubleslash_features::video_codec::codecs_from_params(&c.params))
        .unwrap_or_default()
}

/// Codec for a direct 1:1 call: the best codec both ends can run, preferring
/// `preferred` when both ends have it.
///
/// `None` means no mutual codec, and the caller must not start the camera —
/// sending frames the peer provably cannot decode wastes their bandwidth and
/// shows them nothing.
pub fn pick_direct_codec(
    peer_codecs: &[doubleslash_features::video_codec::VideoCodec],
    preferred: Option<doubleslash_features::video_codec::VideoCodec>,
) -> Option<doubleslash_features::video_codec::VideoCodec> {
    // An *empty* list means "we have not heard what this peer speaks", which is
    // not the same as "this peer speaks nothing we do". Treating the two alike
    // silently refused to start the camera whenever the capability announce had
    // not arrived yet, or came from a build using the previous capability id —
    // the caller sees only that video stopped working.
    //
    // Unknown therefore falls back to our own preferred codec and sends. A
    // receiver that cannot decode it drops the frames and says so in its log,
    // which is a far better failure than a camera that never turns on.
    if peer_codecs.is_empty() {
        return pick_room_codec(preferred);
    }
    doubleslash_features::video_codec::negotiate_preferring(
        &available_codecs(),
        peer_codecs,
        preferred,
    )
}

/// Codec for room video: our own most-preferred available codec, or the user's
/// choice when this build can encode it.
///
/// Deliberately not a negotiation. A room sender fans one encoded stream out to
/// every member, so satisfying everyone would mean either encoding once per
/// codec (N encoders on the sender) or falling to the worst common denominator
/// — and membership changes mid-call, which would force a re-encode and a
/// keyframe every time someone joined. Instead the sender picks what it does
/// best, stamps it on each frame, and a member without that decoder drops the
/// frames and logs why. Room-wide codec convergence is a later problem, and the
/// per-frame codec byte is what leaves room to solve it.
///
/// That structure is also why a user preference is safe here: nothing about a
/// room send was ever agreed pairwise, so choosing VP8 over H.264 changes only
/// which decoder each member routes the frames to.
pub fn pick_room_codec(
    preferred: Option<doubleslash_features::video_codec::VideoCodec>,
) -> Option<doubleslash_features::video_codec::VideoCodec> {
    doubleslash_features::video_codec::best_for_room(&available_codecs(), preferred)
}

/// Build an encoder for `codec`, or `Err` if this build cannot encode it.
pub fn make_encoder(
    codec: VideoCodec,
    params: EncoderParams,
) -> anyhow::Result<Box<dyn VideoEncoder>> {
    match codec {
        #[cfg(target_os = "windows")]
        VideoCodec::H264 => {
            let enc =
                super::mediafoundation::MfEncoder::new(super::mediafoundation::MfEncoderConfig {
                    width: params.width,
                    height: params.height,
                    bitrate_bps: params.bitrate_bps,
                    fps: params.fps,
                    keyframe_interval_secs: params.keyframe_interval_secs,
                })?;
            Ok(Box::new(enc))
        }
        #[cfg(not(target_os = "windows"))]
        VideoCodec::H264 => anyhow::bail!(
            "H.264 encode needs Media Foundation, which is Windows-only; \
             this build has no H.264 encoder"
        ),
        VideoCodec::Vp8 | VideoCodec::Vp9 => Ok(Box::new(VpxEncoderAdapter::new(codec, params)?)),
        VideoCodec::Stub => Ok(Box::new(StubEncoder)),
    }
}

/// Build a decoder for `codec`, or `Err` if this build cannot decode it.
///
/// Called per (sender, codec) rather than once per session: in a room, two
/// members may legitimately send in different codecs, and each needs its own
/// decoder instance anyway because inter frames reference that sender's history.
pub fn make_decoder(codec: VideoCodec) -> anyhow::Result<Box<dyn VideoDecoder>> {
    match codec {
        #[cfg(target_os = "windows")]
        VideoCodec::H264 => Ok(Box::new(super::mediafoundation::MfDecoder::new()?)),
        #[cfg(not(target_os = "windows"))]
        VideoCodec::H264 => anyhow::bail!(
            "H.264 decode needs Media Foundation, which is Windows-only; \
             this build has no H.264 decoder"
        ),
        VideoCodec::Vp8 | VideoCodec::Vp9 => Ok(Box::new(VpxDecoderAdapter::new(codec)?)),
        VideoCodec::Stub => Ok(Box::new(StubDecoder)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_round_trips_a_frame() {
        let frame = RawFrame::test_pattern(STUB_WIDTH, STUB_HEIGHT, 9);
        let (encoded, keyframe) = StubEncoder.encode(&frame).unwrap();
        assert!(keyframe);
        assert_eq!(
            encoded.len(),
            STUB_HEADER_LEN + RawFrame::packed_len(STUB_WIDTH, STUB_HEIGHT)
        );
        assert_eq!(StubDecoder.decode(&encoded).unwrap(), Some(frame));
    }

    #[test]
    fn stub_frame_fits_the_fragment_budget() {
        // The stub must stay encodable within MAX_FRAGS_PER_FRAME, or Phase 3
        // would be testing the fragmenter's refusal path instead of its
        // reassembly path.
        let frame = RawFrame::test_pattern(STUB_WIDTH, STUB_HEIGHT, 0);
        let (encoded, _) = StubEncoder.encode(&frame).unwrap();
        let parts = super::super::fragment::fragment_frame(
            "sender-id-placeholder-000000000000000000000",
            1,
            true,
            VideoCodec::Stub,
            0,
            &[0u8; super::super::fragment::SIGNATURE_LEN],
            &encoded,
            1198,
        );
        let parts = parts.expect("stub frame must fit MAX_FRAGS_PER_FRAME");
        assert!(
            parts.len() > 20,
            "expected a genuinely multi-fragment frame"
        );
    }

    /// The advertised ceiling has to be one the fragmenter really accepts, at
    /// the tightest datagram size and a full-length sender id, once sealed.
    #[test]
    fn the_largest_encoded_frame_still_fragments() {
        use super::super::fragment::{fragment_frame, MAX_ENCODED_FRAME_BYTES, SIGNATURE_LEN};
        // epoch + nonce + GCM tag, as `group_key::seal_media_frame` adds.
        const SEAL: usize = 1 + 12 + 16;
        let sealed = vec![0u8; MAX_ENCODED_FRAME_BYTES + SEAL];
        let sender = "S".repeat(44);
        let parts = fragment_frame(
            &sender,
            1,
            true,
            VideoCodec::Vp9,
            0,
            &[0u8; SIGNATURE_LEN],
            &sealed,
            super::super::PORTABLE_MAX_DATAGRAM - 2,
        );
        assert!(parts.is_some(), "the declared ceiling must fit");
        const { assert!(FRAME_SIZE_TARGET_BYTES < MAX_ENCODED_FRAME_BYTES) };
    }

    #[test]
    fn decode_rejects_truncated_input() {
        assert!(StubDecoder.decode(&[]).is_err());
        assert!(StubDecoder.decode(&[0, 160]).is_err());
    }

    #[test]
    fn decode_rejects_length_mismatch() {
        // Header claims 160x120 but the payload is far too short — must be
        // refused rather than panicking on a slice.
        let mut bad = Vec::new();
        bad.extend_from_slice(&160u16.to_be_bytes());
        bad.extend_from_slice(&120u16.to_be_bytes());
        bad.extend_from_slice(&[0u8; 100]);
        assert!(StubDecoder.decode(&bad).is_err());
    }

    #[test]
    fn decode_rejects_bad_dimensions() {
        for (w, h) in [(0u16, 120u16), (160, 0), (161, 120), (160, 121)] {
            let mut bad = Vec::new();
            bad.extend_from_slice(&w.to_be_bytes());
            bad.extend_from_slice(&h.to_be_bytes());
            bad.extend_from_slice(&[0u8; 64]);
            assert!(
                StubDecoder.decode(&bad).is_err(),
                "{w}x{h} must be rejected"
            );
        }
    }

    #[test]
    fn encode_rejects_inconsistent_frame() {
        let mut frame = RawFrame::black(64, 48);
        frame.u.truncate(3);
        assert!(StubEncoder.encode(&frame).is_err());
    }

    fn test_params() -> EncoderParams {
        EncoderParams {
            width: STUB_WIDTH,
            height: STUB_HEIGHT,
            bitrate_bps: 600_000,
            fps: 30,
            keyframe_interval_secs: 4,
        }
    }

    /// The stub must never be advertised, so it must never be in the set the
    /// client hands to capability registration.
    #[test]
    fn available_codecs_never_includes_the_stub() {
        assert!(!available_codecs().contains(&VideoCodec::Stub));
    }

    /// Advertising a codec we cannot construct is the exact dishonesty this
    /// registry exists to prevent, so every advertised codec must build both
    /// halves on this platform.
    #[test]
    fn every_available_codec_can_build_both_halves() {
        for codec in available_codecs() {
            assert!(
                make_encoder(codec, test_params()).is_ok(),
                "{codec:?} is advertised but has no encoder"
            );
            assert!(
                make_decoder(codec).is_ok(),
                "{codec:?} is advertised but has no decoder"
            );
        }
    }

    #[test]
    fn the_stub_is_always_constructible_for_transport_tests() {
        assert!(make_encoder(VideoCodec::Stub, test_params()).is_ok());
        assert!(make_decoder(VideoCodec::Stub).is_ok());
    }

    /// VP8 is what a Linux peer will negotiate, so it has to be present in
    /// every build regardless of platform — not only where an OS codec is
    /// missing.
    #[test]
    fn vp8_is_available_on_every_platform() {
        assert!(
            available_codecs().contains(&VideoCodec::Vp8),
            "VP8 must be built everywhere or cross-platform calls have no mutual codec"
        );
    }

    /// The adapter has to survive a real frame, not just construct: a mismatch
    /// between `RawFrame`'s packing and what libvpx expects would corrupt the
    /// picture rather than error.
    #[test]
    fn vp8_adapter_round_trips_a_real_frame() {
        let params = EncoderParams {
            width: 320,
            height: 240,
            ..test_params()
        };
        let mut enc = make_encoder(VideoCodec::Vp8, params).expect("vp8 encoder");
        let mut dec = make_decoder(VideoCodec::Vp8).expect("vp8 decoder");

        let original = RawFrame::test_pattern(320, 240, 5);
        let (packet, keyframe) = enc.encode(&original).expect("encode");
        assert!(!packet.is_empty() && keyframe);

        let out = dec.decode(&packet).expect("decode").expect("a picture");
        assert_eq!((out.width, out.height), (320, 240));
        assert!(out.is_consistent());
    }

    /// libvpx encodes at the size it was constructed for, so a mismatched
    /// frame must be refused rather than read with the wrong stride.
    #[test]
    fn vp8_adapter_refuses_a_frame_of_the_wrong_size() {
        let params = EncoderParams {
            width: 320,
            height: 240,
            ..test_params()
        };
        let mut enc = make_encoder(VideoCodec::Vp8, params).unwrap();
        assert!(enc.encode(&RawFrame::black(160, 120)).is_err());
    }

    /// On Windows the advertised set must contain H.264, since that is what
    /// the Media Foundation path actually encodes.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_advertises_h264() {
        assert!(available_codecs().contains(&VideoCodec::H264));
    }

    // ── VP9 and the realtime fallback ──────────────────────────────────────

    #[test]
    fn every_build_encodes_and_decodes_vp9() {
        assert!(available_codecs().contains(&VideoCodec::Vp9));
        let params = EncoderParams {
            width: 320,
            height: 240,
            bitrate_bps: 400_000,
            fps: 30,
            keyframe_interval_secs: 4,
        };
        let mut enc = make_encoder(VideoCodec::Vp9, params).unwrap();
        assert_eq!(enc.codec(), VideoCodec::Vp9);
        let (packet, keyframe) = enc.encode(&RawFrame::black(320, 240)).unwrap();
        assert!(keyframe);
        let mut dec = make_decoder(VideoCodec::Vp9).unwrap();
        let frame = dec.decode(&packet).unwrap().expect("a keyframe is shown");
        assert_eq!((frame.width, frame.height), (320, 240));
    }

    /// Rooms default to VP9 on every platform, including Windows, where H.264
    /// used to win and left every non-Windows member without a picture.
    #[test]
    fn a_room_sends_vp9_by_default() {
        assert_eq!(pick_room_codec(None), Some(VideoCodec::Vp9));
    }

    fn slow(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// Past warm-up, a full window of encodes at 80% of the frame interval is
    /// a codec this device cannot afford.
    #[test]
    fn a_consistently_slow_codec_is_over_budget() {
        let mut budget = EncodeBudget::new(30);
        let verdicts: Vec<bool> = (0..BUDGET_WARMUP_FRAMES as usize + BUDGET_WINDOW_FRAMES)
            .map(|_| budget.record(slow(27)))
            .collect();
        assert!(
            !verdicts[..verdicts.len() - 1].iter().any(|v| *v),
            "no verdict before warm-up plus a full window"
        );
        assert!(verdicts[verdicts.len() - 1]);
    }

    #[test]
    fn a_codec_with_headroom_is_left_alone() {
        let mut budget = EncodeBudget::new(30);
        // What VP9 720p measured on a Pixel 11: 4.5 ms of a 33 ms interval.
        assert!(!(0..300).any(|_| budget.record(slow(5))));
    }

    /// The warm-up exists for the opening keyframe, which is legitimately slow.
    #[test]
    fn a_slow_start_is_forgiven() {
        let mut budget = EncodeBudget::new(30);
        for _ in 0..BUDGET_WARMUP_FRAMES {
            assert!(!budget.record(slow(200)));
        }
        assert!(!(0..300).any(|_| budget.record(slow(5))));
    }

    /// Fine on average but regularly blowing the interval still stutters.
    #[test]
    fn frequent_overruns_are_over_budget_even_with_a_low_mean() {
        let mut budget = EncodeBudget::new(30);
        let mut over = false;
        for i in 0..(BUDGET_WARMUP_FRAMES as usize + BUDGET_WINDOW_FRAMES) {
            // Every third frame blows the 33 ms interval; the rest are quick.
            over = budget.record(if i % 3 == 0 { slow(40) } else { slow(2) });
        }
        assert!(over);
    }

    fn fallback_params() -> EncoderParams {
        EncoderParams {
            width: 320,
            height: 240,
            bitrate_bps: 400_000,
            fps: 30,
            keyframe_interval_secs: 4,
        }
    }

    #[test]
    fn a_slow_encoder_is_replaced_once_and_keeps_its_bitrate() {
        let params = fallback_params();
        let primary = make_encoder(VideoCodec::Vp9, params).unwrap();
        let mut enc = FallbackEncoder::new(primary, VideoCodec::Vp8, params);
        enc.set_bitrate(250_000).unwrap();

        for _ in 0..(BUDGET_WARMUP_FRAMES as usize + BUDGET_WINDOW_FRAMES) {
            enc.after_encode(slow(30));
        }
        assert_eq!(enc.codec(), VideoCodec::Vp8, "switched to the fallback");
        assert_eq!(enc.bitrate_bps, 250_000);
        assert!(enc.fallback.is_none(), "the fallback is spent");

        // The replacement's first frame is a keyframe, which is what lets
        // every receiver start decoding the new codec at once.
        let (packet, keyframe) = enc.encode(&RawFrame::black(320, 240)).unwrap();
        assert!(!packet.is_empty());
        assert!(keyframe);

        // And it never switches back, however the budget reads afterwards.
        for _ in 0..200 {
            enc.after_encode(slow(30));
        }
        assert_eq!(enc.codec(), VideoCodec::Vp8);
    }

    #[test]
    fn a_fast_encoder_keeps_its_codec() {
        let params = fallback_params();
        let primary = make_encoder(VideoCodec::Vp9, params).unwrap();
        let mut enc = FallbackEncoder::new(primary, VideoCodec::Vp8, params);
        for _ in 0..300 {
            enc.after_encode(slow(4));
        }
        assert_eq!(enc.codec(), VideoCodec::Vp9);
    }

    #[test]
    fn only_vp9_falls_back() {
        assert_eq!(realtime_fallback(VideoCodec::Vp9), Some(VideoCodec::Vp8));
        assert_eq!(realtime_fallback(VideoCodec::Vp8), None);
        assert_eq!(realtime_fallback(VideoCodec::H264), None);
    }
}
