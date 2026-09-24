//! VP8 and VP9 encode and decode over a vendored libvpx.
//!
//! These are the codecs every platform can carry. Windows has Media Foundation
//! H.264 and macOS has VideoToolbox, both backed by a codec licence the OS
//! holds; Linux has no equivalent, and shipping our own AVC encoder would put
//! MPEG-LA licensing on this project. VP8 and VP9 are royalty-free, so they are
//! what a Windows peer and a Linux or Android peer can always agree on.
//!
//! VP9 spends more CPU than VP8 for a smaller stream at the same quality. On
//! aarch64 the build enables libvpx's NEON paths, which is what makes VP9
//! affordable on a phone; see `build.rs`. Everything crosses into C through
//! `src/shim.c` rather than through libvpx's own structs — also explained there.
//!
//! ```
//! use doubleslash_vpx::{Codec, EncoderConfig, VpxDecoder, VpxEncoder};
//! let (w, h) = (640usize, 360usize);
//! let y = vec![128u8; w * h];
//! let u = vec![128u8; (w / 2) * (h / 2)];
//! let v = vec![128u8; (w / 2) * (h / 2)];
//!
//! for codec in [Codec::Vp8, Codec::Vp9] {
//!     let config = EncoderConfig::realtime(codec, w as u32, h as u32, 600_000, 30, 4);
//!     let mut enc = VpxEncoder::new(codec, config)?;
//!     let (packet, _keyframe) = enc.encode(&y, &u, &v)?;
//!
//!     let mut dec = VpxDecoder::new(codec)?;
//!     let frame = dec.decode(&packet)?.expect("a keyframe always shows");
//!     assert_eq!((frame.width, frame.height), (640, 360));
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```

use std::os::raw::{c_int, c_uchar};

#[allow(non_camel_case_types)]
mod ffi {
    use std::os::raw::{c_int, c_uchar};

    // Opaque: both types are defined in shim.c and never inspected here.
    #[repr(C)]
    pub struct cq_vpx_enc {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct cq_vpx_dec {
        _private: [u8; 0],
    }

    extern "C" {
        pub fn cq_vpx_enc_new(
            codec: c_int,
            width: c_int,
            height: c_int,
            bitrate_bps: c_int,
            fps: c_int,
            keyframe_interval_secs: c_int,
            cpu_used: c_int,
            threads: c_int,
        ) -> *mut cq_vpx_enc;
        pub fn cq_vpx_enc_free(e: *mut cq_vpx_enc);
        pub fn cq_vpx_enc_request_keyframe(e: *mut cq_vpx_enc);
        pub fn cq_vpx_enc_set_bitrate(e: *mut cq_vpx_enc, bitrate_bps: c_int) -> c_int;
        pub fn cq_vpx_enc_encode(
            e: *mut cq_vpx_enc,
            y: *const c_uchar,
            u: *const c_uchar,
            v: *const c_uchar,
            out: *mut c_uchar,
            out_cap: c_int,
            is_keyframe: *mut c_int,
        ) -> c_int;

        pub fn cq_vpx_dec_new(codec: c_int, threads: c_int) -> *mut cq_vpx_dec;
        pub fn cq_vpx_dec_free(d: *mut cq_vpx_dec);
        pub fn cq_vpx_dec_decode(
            d: *mut cq_vpx_dec,
            data: *const c_uchar,
            len: c_int,
            out: *mut c_uchar,
            out_cap: c_int,
            width: *mut c_int,
            height: *mut c_int,
        ) -> c_int;
        pub fn cq_vpx_dec_copy_pending(
            d: *mut cq_vpx_dec,
            out: *mut c_uchar,
            out_cap: c_int,
            width: *mut c_int,
            height: *mut c_int,
        ) -> c_int;
    }
}

/// Which libvpx codec an encoder or decoder speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    Vp8,
    Vp9,
}

impl Codec {
    /// The selector `shim.c` switches on.
    fn id(self) -> c_int {
        match self {
            Codec::Vp8 => 0,
            Codec::Vp9 => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Codec::Vp8 => "VP8",
            Codec::Vp9 => "VP9",
        }
    }
}

/// Everything an encoder is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate_bps: u32,
    pub fps: u32,
    /// Maximum seconds between keyframes.
    pub keyframe_interval_secs: u32,
    /// libvpx's speed/quality dial: higher is faster and worse. Realtime use
    /// lives at the top of the range (VP8 up to 16, VP9 up to 9).
    pub cpu_used: i32,
    /// Encoder threads. VP9 turns these into tile columns and row-based
    /// multithreading; VP8 into row-parallel encoding.
    pub threads: u32,
}

/// Frames up to this many pixels encode VP9 at speed 7 rather than 8.
///
/// Measured with `examples/encode_bench.rs` on the Xiph FourPeople sequence:
/// at 640x360 speed 7 gains 0.4 dB over speed 8 and still encodes in 1.5 ms
/// on a Pixel 11 (NEON). At 1280x720 the gain shrinks to 0.06 dB for 28% more
/// time, which is not worth it.
const VP9_SLOW_SPEED_MAX_PIXELS: u32 = 640 * 480;

/// Frames from this many pixels up get a second encoder thread off ARM.
const THREADED_MIN_PIXELS: u32 = 1280 * 720;

impl EncoderConfig {
    /// The settings a live call uses for `codec`.
    ///
    /// The numbers come from `examples/encode_bench.rs`, run on a Pixel 11 and
    /// on an x86 desktop:
    ///
    /// * **Speed.** VP8 stays at 8, where calls have always run it; in
    ///   realtime mode it spends spare CPU on quality by itself. VP9 uses 7 for
    ///   small frames and 8 above ([`VP9_SLOW_SPEED_MAX_PIXELS`]).
    /// * **Threads.** One on aarch64: on the phone two and four threads were
    ///   3-8x *slower* than one, with large jitter — waking workers costs more
    ///   than a 720p frame takes to encode on a single big core. Elsewhere the
    ///   build has no SIMD (x86), so 720p and up takes a second thread, which
    ///   halved VP9's 720p encode time on the desktop.
    pub fn realtime(
        codec: Codec,
        width: u32,
        height: u32,
        bitrate_bps: u32,
        fps: u32,
        keyframe_interval_secs: u32,
    ) -> Self {
        let pixels = width.saturating_mul(height);
        let cpu_used = match codec {
            Codec::Vp8 => 8,
            Codec::Vp9 if pixels <= VP9_SLOW_SPEED_MAX_PIXELS => 7,
            Codec::Vp9 => 8,
        };
        let threads = if cfg!(target_arch = "aarch64") || pixels < THREADED_MIN_PIXELS {
            1
        } else {
            2
        };
        Self {
            width,
            height,
            bitrate_bps,
            fps,
            keyframe_interval_secs,
            cpu_used,
            threads,
        }
    }
}

/// A decoded I420 frame, tightly packed.
///
/// Mirrors the client's `RawFrame` without depending on it — this crate sits
/// below the client and must not reach up into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    /// Luma, `width * height` bytes.
    pub y: Vec<u8>,
    /// Chroma-blue, `((width+1)/2) * ((height+1)/2)` bytes.
    pub u: Vec<u8>,
    /// Chroma-red, same size as [`Self::u`].
    pub v: Vec<u8>,
}

/// A VP8 or VP9 encoder.
///
/// Not `Sync`: libvpx codec contexts have no internal locking, and the
/// intended use is one encoder owned by the capture thread.
pub struct VpxEncoder {
    inner: *mut ffi::cq_vpx_enc,
    codec: Codec,
    /// Reused across frames so a steady encode does not allocate per frame.
    scratch: Vec<u8>,
    width: u32,
    height: u32,
}

// SAFETY: the context is owned exclusively by this struct and every call goes
// through `&mut self`, so it can move between threads but never be shared.
unsafe impl Send for VpxEncoder {}

impl VpxEncoder {
    pub fn new(codec: Codec, config: EncoderConfig) -> anyhow::Result<Self> {
        let EncoderConfig {
            width,
            height,
            bitrate_bps,
            fps,
            keyframe_interval_secs,
            cpu_used,
            threads,
        } = config;
        // 4:2:0 chroma planes need even dimensions.
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            anyhow::bail!(
                "{} needs non-zero even dimensions, got {width}x{height}",
                codec.name()
            );
        }
        // SAFETY: dimensions validated above; the shim returns null on failure.
        let inner = unsafe {
            ffi::cq_vpx_enc_new(
                codec.id(),
                width as c_int,
                height as c_int,
                bitrate_bps as c_int,
                fps as c_int,
                keyframe_interval_secs as c_int,
                cpu_used as c_int,
                threads.max(1) as c_int,
            )
        };
        if inner.is_null() {
            anyhow::bail!(
                "libvpx rejected a {width}x{height} {} encoder at {bitrate_bps} bps",
                codec.name()
            );
        }
        Ok(Self {
            inner,
            codec,
            // A keyframe is the large case; sizing for an uncompressed frame
            // means the buffer never grows in practice.
            scratch: vec![0u8; (width as usize * height as usize * 3 / 2).max(64 * 1024)],
            width,
            height,
        })
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Encoded dimensions, as configured.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Ask for the next encoded frame to be a keyframe.
    pub fn request_keyframe(&mut self) {
        // SAFETY: `inner` is non-null for the lifetime of self.
        unsafe { ffi::cq_vpx_enc_request_keyframe(self.inner) }
    }

    /// Retarget the average bitrate without rebuilding the encoder.
    pub fn set_bitrate(&mut self, bitrate_bps: u32) -> anyhow::Result<()> {
        // SAFETY: `inner` is non-null for the lifetime of self.
        let rc = unsafe { ffi::cq_vpx_enc_set_bitrate(self.inner, bitrate_bps as c_int) };
        if rc != 0 {
            anyhow::bail!("libvpx rejected a bitrate change to {bitrate_bps} bps");
        }
        Ok(())
    }

    /// Encode one tightly-packed I420 frame.
    ///
    /// Returns the encoded packet and whether it is a keyframe. An empty
    /// packet is not an error: under rate control libvpx may legitimately drop
    /// a frame, and the caller should simply not send anything for it.
    pub fn encode(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> anyhow::Result<(Vec<u8>, bool)> {
        let (w, h) = (self.width as usize, self.height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        // Checked here rather than in C: the shim indexes these planes by row
        // and a short buffer would read out of bounds.
        if y.len() < w * h || u.len() < cw * ch || v.len() < cw * ch {
            anyhow::bail!(
                "frame planes too small for {w}x{h}: y={} u={} v={}",
                y.len(),
                u.len(),
                v.len()
            );
        }

        let mut is_keyframe: c_int = 0;
        loop {
            // SAFETY: planes are length-checked above; `scratch` is a valid
            // writable buffer of `scratch.len()`; `is_keyframe` is a live int.
            let rc = unsafe {
                ffi::cq_vpx_enc_encode(
                    self.inner,
                    y.as_ptr() as *const c_uchar,
                    u.as_ptr() as *const c_uchar,
                    v.as_ptr() as *const c_uchar,
                    self.scratch.as_mut_ptr() as *mut c_uchar,
                    self.scratch.len() as c_int,
                    &mut is_keyframe,
                )
            };
            match rc {
                -2 => {
                    // Output did not fit. Only possible for a pathological
                    // keyframe; grow once and retry rather than failing.
                    let bigger = self.scratch.len() * 2;
                    if bigger > 64 * 1024 * 1024 {
                        anyhow::bail!(
                            "{} frame exceeded the 64 MB output ceiling",
                            self.codec.name()
                        );
                    }
                    self.scratch.resize(bigger, 0);
                }
                -1 => anyhow::bail!("libvpx {} encode failed", self.codec.name()),
                n => return Ok((self.scratch[..n as usize].to_vec(), is_keyframe != 0)),
            }
        }
    }
}

impl Drop for VpxEncoder {
    fn drop(&mut self) {
        // SAFETY: `inner` was produced by `cq_vpx_enc_new` and is freed once.
        unsafe { ffi::cq_vpx_enc_free(self.inner) }
    }
}

/// A VP8 or VP9 decoder. One per sender — inter frames reference that
/// sender's history.
pub struct VpxDecoder {
    inner: *mut ffi::cq_vpx_dec,
    codec: Codec,
    scratch: Vec<u8>,
}

// SAFETY: see `VpxEncoder`.
unsafe impl Send for VpxDecoder {}

impl VpxDecoder {
    /// A single-threaded decoder, which is what a call's streams need.
    pub fn new(codec: Codec) -> anyhow::Result<Self> {
        Self::with_threads(codec, 1)
    }

    pub fn with_threads(codec: Codec, threads: u32) -> anyhow::Result<Self> {
        // SAFETY: returns null on failure, checked below.
        let inner = unsafe { ffi::cq_vpx_dec_new(codec.id(), threads.max(1) as c_int) };
        if inner.is_null() {
            anyhow::bail!("could not initialise a libvpx {} decoder", codec.name());
        }
        Ok(Self {
            inner,
            codec,
            // 640x360 I420; grows on demand for larger streams.
            scratch: vec![0u8; 640 * 360 * 3 / 2],
        })
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Decode one packet into I420.
    ///
    /// `Ok(None)` is a packet that decoded but carries no picture to show —
    /// possible in VP9, and not an error.
    pub fn decode(&mut self, data: &[u8]) -> anyhow::Result<Option<DecodedFrame>> {
        let name = self.codec.name();
        if data.is_empty() {
            anyhow::bail!("empty {name} packet");
        }
        let mut width: c_int = 0;
        let mut height: c_int = 0;

        // SAFETY: `data` is a valid slice; `scratch` is writable for its
        // length; both out-params are live ints.
        let mut rc = unsafe {
            ffi::cq_vpx_dec_decode(
                self.inner,
                data.as_ptr() as *const c_uchar,
                data.len() as c_int,
                self.scratch.as_mut_ptr() as *mut c_uchar,
                self.scratch.len() as c_int,
                &mut width,
                &mut height,
            )
        };
        if rc == -2 {
            // The shim reported the real dimensions and kept the picture, so
            // the buffer grows to exactly that and the copy is retried —
            // without decoding the packet again, which would advance the
            // stream past the frame being copied.
            let (w, h) = (width.max(0) as usize, height.max(0) as usize);
            let needed = w * h + 2 * w.div_ceil(2) * h.div_ceil(2);
            if needed == 0 || needed > 64 * 1024 * 1024 {
                anyhow::bail!("{name} stream declared an unusable size {width}x{height}");
            }
            self.scratch.resize(needed, 0);
            // SAFETY: as above; the pending picture is valid until the next
            // decode call, and none happens in between.
            rc = unsafe {
                ffi::cq_vpx_dec_copy_pending(
                    self.inner,
                    self.scratch.as_mut_ptr() as *mut c_uchar,
                    self.scratch.len() as c_int,
                    &mut width,
                    &mut height,
                )
            };
        }
        match rc {
            -3 => Ok(None),
            -2 => anyhow::bail!("{name} output buffer still too small after resize"),
            n if n < 0 => anyhow::bail!("libvpx {name} decode failed"),
            _ => {
                let (w, h) = (width as u32, height as u32);
                let (cw, chh) = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
                let y_len = w as usize * h as usize;
                let c_len = cw * chh;
                Ok(Some(DecodedFrame {
                    width: w,
                    height: h,
                    y: self.scratch[..y_len].to_vec(),
                    u: self.scratch[y_len..y_len + c_len].to_vec(),
                    v: self.scratch[y_len + c_len..y_len + 2 * c_len].to_vec(),
                }))
            }
        }
    }
}

impl Drop for VpxDecoder {
    fn drop(&mut self) {
        // SAFETY: `inner` was produced by `cq_vpx_dec_new` and is freed once.
        unsafe { ffi::cq_vpx_dec_free(self.inner) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 320;
    const H: u32 = 240;
    const BOTH: [Codec; 2] = [Codec::Vp8, Codec::Vp9];

    fn encoder(codec: Codec, bitrate_bps: u32) -> VpxEncoder {
        VpxEncoder::new(
            codec,
            EncoderConfig::realtime(codec, W, H, bitrate_bps, 30, 4),
        )
        .unwrap()
    }

    /// A deterministic pattern with real spatial detail, so a codec that
    /// silently produced a flat frame would not pass.
    fn test_frame(seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (w, h) = (W as usize, H as usize);
        let (cw, ch) = (w / 2, h / 2);
        let mut y = vec![0u8; w * h];
        for row in 0..h {
            for col in 0..w {
                y[row * w + col] = ((row * 3 + col * 5) as u8).wrapping_add(seed);
            }
        }
        let u = vec![100u8.wrapping_add(seed); cw * ch];
        let v = vec![160u8.wrapping_add(seed); cw * ch];
        (y, u, v)
    }

    #[test]
    fn encoder_rejects_odd_or_zero_dimensions() {
        for codec in BOTH {
            for (w, h) in [(0, 240), (321, 240), (320, 0)] {
                let config = EncoderConfig::realtime(codec, w, h, 600_000, 30, 4);
                assert!(VpxEncoder::new(codec, config).is_err(), "{codec:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn encoder_rejects_short_planes() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let (y, u, v) = test_frame(0);
            assert!(enc.encode(&y[..y.len() - 1], &u, &v).is_err());
            assert!(enc.encode(&y, &u[..u.len() - 1], &v).is_err());
        }
    }

    /// The property that matters: real bytes in, a decodable frame out, at the
    /// size we asked for. Without this the whole cross-platform story rests on
    /// a codec nobody has run.
    #[test]
    fn encode_decode_round_trip() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let mut dec = VpxDecoder::new(codec).unwrap();
            let (y, u, v) = test_frame(7);

            let (packet, keyframe) = enc.encode(&y, &u, &v).unwrap();
            assert!(
                !packet.is_empty(),
                "{codec:?}: first frame must produce output"
            );
            assert!(
                keyframe,
                "{codec:?}: the first encoded frame is always a keyframe"
            );

            let out = dec.decode(&packet).unwrap().expect("a keyframe is shown");
            assert_eq!((out.width, out.height), (W, H));
            assert_eq!(out.y.len(), (W * H) as usize);
            assert_eq!(out.u.len(), ((W / 2) * (H / 2)) as usize);
            assert_eq!(out.v.len(), out.u.len());
        }
    }

    /// Lossy, so exact equality is the wrong test — but a codec that returned
    /// garbage would not land anywhere near the input.
    #[test]
    fn decoded_luma_resembles_the_input() {
        for codec in BOTH {
            let mut enc = encoder(codec, 2_000_000);
            let mut dec = VpxDecoder::new(codec).unwrap();
            let (y, u, v) = test_frame(0);

            let (packet, _) = enc.encode(&y, &u, &v).unwrap();
            let out = dec.decode(&packet).unwrap().expect("a keyframe is shown");

            let total: u64 = out
                .y
                .iter()
                .zip(y.iter())
                .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64)
                .sum();
            let mean = total / out.y.len() as u64;
            assert!(
                mean < 24,
                "{codec:?}: mean luma error {mean} is too high to be a decode"
            );
        }
    }

    /// Compression has to actually happen, or the fragmenter's frame-size
    /// assumptions (and the relay quota) are wrong.
    #[test]
    fn output_is_much_smaller_than_the_raw_frame() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let (y, u, v) = test_frame(3);
            let (packet, _) = enc.encode(&y, &u, &v).unwrap();
            let raw = (W * H) as usize * 3 / 2;
            assert!(
                packet.len() < raw / 2,
                "{codec:?}: encoded {} bytes vs {raw} raw — that is not compression",
                packet.len()
            );
        }
    }

    /// Inter frames must reference the sender's history, so a decoder fed the
    /// stream in order tracks it; this also covers the multi-frame path where
    /// the encoder may drop or delay output.
    #[test]
    fn a_sequence_of_frames_decodes_in_order() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let mut dec = VpxDecoder::new(codec).unwrap();
            let mut decoded = 0;
            for seed in 0..10u8 {
                let (y, u, v) = test_frame(seed);
                let (packet, _) = enc.encode(&y, &u, &v).unwrap();
                if packet.is_empty() {
                    continue; // legitimately dropped under rate control
                }
                if let Some(out) = dec.decode(&packet).unwrap() {
                    assert_eq!((out.width, out.height), (W, H));
                    decoded += 1;
                }
            }
            assert!(
                decoded >= 5,
                "{codec:?}: only {decoded} of 10 frames decoded"
            );
        }
    }

    /// Several encoder threads must still produce a stream a single-threaded
    /// decoder reads — the tile layout is a property of the stream, not of the
    /// decoder.
    #[test]
    fn a_multithreaded_encode_decodes() {
        for codec in BOTH {
            let config = EncoderConfig {
                threads: 4,
                ..EncoderConfig::realtime(codec, 1280, 720, 1_500_000, 30, 4)
            };
            let mut enc = VpxEncoder::new(codec, config).unwrap();
            let mut dec = VpxDecoder::new(codec).unwrap();
            let y = vec![90u8; 1280 * 720];
            let c = vec![128u8; 640 * 360];
            let (packet, _) = enc.encode(&y, &c, &c).unwrap();
            let out = dec.decode(&packet).unwrap().expect("a keyframe is shown");
            assert_eq!((out.width, out.height), (1280, 720));
        }
    }

    #[test]
    fn requested_keyframe_is_produced() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let (y, u, v) = test_frame(1);
            // Frame 1 is a keyframe on its own; get past it first.
            let _ = enc.encode(&y, &u, &v).unwrap();
            let (y2, u2, v2) = test_frame(2);
            let _ = enc.encode(&y2, &u2, &v2).unwrap();

            enc.request_keyframe();
            let (y3, u3, v3) = test_frame(3);
            let (packet, keyframe) = enc.encode(&y3, &u3, &v3).unwrap();
            assert!(!packet.is_empty());
            assert!(
                keyframe,
                "{codec:?}: an explicitly requested keyframe must be produced"
            );
        }
    }

    #[test]
    fn bitrate_can_be_retargeted_on_a_running_encoder() {
        for codec in BOTH {
            let mut enc = encoder(codec, 600_000);
            let (y, u, v) = test_frame(4);
            let _ = enc.encode(&y, &u, &v).unwrap();
            enc.set_bitrate(200_000)
                .expect("retargeting a live encoder must not fail");
            let _ = enc.encode(&y, &u, &v).unwrap();
        }
    }

    /// A stream is only readable by the decoder for its own codec. Feeding a
    /// VP9 keyframe to a VP8 decoder must fail, not produce a picture — this
    /// is what makes the per-frame codec byte the thing that picks a decoder.
    #[test]
    fn a_decoder_refuses_the_other_codec() {
        let mut enc = encoder(Codec::Vp9, 600_000);
        let (y, u, v) = test_frame(5);
        let (packet, _) = enc.encode(&y, &u, &v).unwrap();
        let mut vp8 = VpxDecoder::new(Codec::Vp8).unwrap();
        assert!(!matches!(vp8.decode(&packet), Ok(Some(_))));
    }

    #[test]
    fn decoder_rejects_garbage() {
        for codec in BOTH {
            let mut dec = VpxDecoder::new(codec).unwrap();
            assert!(dec.decode(&[]).is_err());
            assert!(
                dec.decode(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11]).is_err(),
                "{codec:?}"
            );
        }
    }
}
