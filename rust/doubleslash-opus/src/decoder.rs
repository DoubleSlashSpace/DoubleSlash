//! Safe Opus decoder wrapper.

use crate::ffi;
use crate::OpusError;

/// Decoder complexity applied by [`OpusDecoder::new`].
///
/// libopus defaults the decoder to 0, which leaves Deep PLC off. 5 is the
/// floor that turns it on. LACE needs 6, NOLACE needs 7, and bandwidth
/// extension needs 4 plus an explicit enable this default does not set.
pub const DECODER_COMPLEXITY_DEFAULT: i32 = 5;

/// Lowest complexity libopus accepts.
pub const DECODER_COMPLEXITY_MIN: i32 = 0;

/// Highest complexity libopus accepts.
pub const DECODER_COMPLEXITY_MAX: i32 = 10;

/// Clamp to the range `OPUS_SET_COMPLEXITY` accepts.
pub fn clamp_decoder_complexity(complexity: i32) -> i32 {
    complexity.clamp(DECODER_COMPLEXITY_MIN, DECODER_COMPLEXITY_MAX)
}

/// Short name for what a complexity step actually enables.
///
/// Both clients show this next to the control. 0–4 is ordinary concealment.
/// 5 adds Deep PLC. 6 adds OSCE LACE. 7 and above add NOLACE. Bandwidth
/// extension is not included: it also needs `OPUS_SET_OSCE_BWE`.
pub fn decoder_complexity_label(complexity: i32) -> &'static str {
    match clamp_decoder_complexity(complexity) {
        0..=4 => "Basic concealment",
        5 => "Deep PLC",
        6 => "Deep PLC and LACE",
        _ => "Deep PLC and NOLACE",
    }
}

/// Safe, owned wrapper around a libopus decoder state.
///
/// `Send` but not `Sync` — same threading contract as [`OpusEncoder`](crate::OpusEncoder).
pub struct OpusDecoder {
    ptr: *mut ffi::OpusDecoder,
}

// SAFETY: Exclusive ownership of the pointer; libopus state is not thread-local.
unsafe impl Send for OpusDecoder {}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        // SAFETY: pointer created by opus_decoder_create, freed at most once.
        unsafe { ffi::opus_decoder_destroy(self.ptr) };
    }
}

impl OpusDecoder {
    /// Create an Opus decoder.
    ///
    /// * `sample_rate` — 8000, 12000, 16000, 24000, or 48000 Hz.
    /// * `channels` — 1 (mono) or 2 (stereo).
    ///
    /// When the `dnn` feature is enabled, the embedded DNN weights are
    /// available. This sets complexity to [`DECODER_COMPLEXITY_DEFAULT`] so
    /// Deep PLC runs. OSCE LACE and NOLACE stay off until complexity is
    /// raised; compiling the weights in is not enough to turn them on.
    pub fn new(sample_rate: u32, channels: u8) -> Result<Self, OpusError> {
        Self::with_complexity(sample_rate, channels, DECODER_COMPLEXITY_DEFAULT)
    }

    /// Create a decoder and set its complexity (clamped to 0–10).
    pub fn with_complexity(
        sample_rate: u32,
        channels: u8,
        complexity: i32,
    ) -> Result<Self, OpusError> {
        let mut error = 0i32;
        let ptr =
            unsafe { ffi::opus_decoder_create(sample_rate as i32, channels as i32, &mut error) };
        if ptr.is_null() {
            return Err(OpusError(if error != 0 {
                error
            } else {
                ffi::OPUS_ALLOC_FAIL
            }));
        }
        let mut dec = Self { ptr };
        unsafe { crate::dnn::load_into_decoder(dec.ptr)? };
        dec.set_complexity(complexity)?;
        Ok(dec)
    }

    /// Set computational complexity (0 = lowest, 10 = highest).
    ///
    /// Values outside 0–10 are clamped. Deep PLC runs at 5 and above, OSCE
    /// LACE at 6, NOLACE at 7. Takes effect on the next decoded frame.
    pub fn set_complexity(&mut self, complexity: i32) -> Result<(), OpusError> {
        let value = clamp_decoder_complexity(complexity);
        let r = unsafe { ffi::doubleslash_dec_set_complexity(self.ptr, value) };
        if r == ffi::OPUS_OK {
            Ok(())
        } else {
            Err(OpusError(r))
        }
    }

    /// Complexity currently installed, 0–10.
    pub fn complexity(&self) -> Result<i32, OpusError> {
        let mut value = 0i32;
        let r = unsafe { ffi::doubleslash_dec_get_complexity(self.ptr, &mut value) };
        if r == ffi::OPUS_OK {
            Ok(value)
        } else {
            Err(OpusError(r))
        }
    }

    /// Reset the decoder state.  Use when switching to a new incoming stream.
    pub fn reset_state(&mut self) -> Result<(), OpusError> {
        let r = unsafe { ffi::doubleslash_dec_reset_state(self.ptr) };
        if r == ffi::OPUS_OK {
            Ok(())
        } else {
            Err(OpusError(r))
        }
    }

    /// Decode one Opus packet to 16-bit PCM.
    ///
    /// Pass `input = None` to perform **packet loss concealment** (PLC) for a
    /// missing frame; `output` must be sized for exactly the duration of the
    /// missing frame.
    ///
    /// With `input = Some(data)` and `decode_fec = true`, the decoder
    /// reconstructs audio for the *previous* (lost) packet using the FEC data
    /// embedded in `data`.  `output` must be sized for the duration of that
    /// prior frame.
    ///
    /// A safe output buffer size for 48 kHz mono with 20 ms frames is
    /// `960` samples (`1920` bytes).  The absolute maximum is
    /// `5760 * channels` samples.
    ///
    /// Returns the number of **decoded samples per channel** on success.
    pub fn decode(
        &mut self,
        input: Option<&[u8]>,
        output: &mut [i16],
        decode_fec: bool,
    ) -> Result<usize, OpusError> {
        let (data_ptr, data_len) = match input {
            Some(d) => (d.as_ptr(), d.len() as i32),
            None => (std::ptr::null(), 0),
        };
        let n = unsafe {
            ffi::opus_decode(
                self.ptr,
                data_ptr,
                data_len,
                output.as_mut_ptr(),
                output.len() as i32, // frame_size
                decode_fec as i32,
            )
        };
        if n < 0 {
            Err(OpusError(n))
        } else {
            Ok(n as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_decoder_enables_deep_plc_and_leaves_osce_off() {
        let mut dec = OpusDecoder::new(48_000, 1).unwrap();
        assert_eq!(dec.complexity().unwrap(), DECODER_COMPLEXITY_DEFAULT);
        assert_eq!(decoder_complexity_label(5), "Deep PLC");
        assert_eq!(decoder_complexity_label(4), "Basic concealment");
        assert_eq!(decoder_complexity_label(6), "Deep PLC and LACE");
        assert_eq!(decoder_complexity_label(7), "Deep PLC and NOLACE");
        // A fresh decoder can conceal one 20 ms frame at this complexity.
        let mut pcm = [0i16; 960];
        assert_eq!(dec.decode(None, &mut pcm, false).unwrap(), 960);
    }

    #[test]
    fn complexity_is_clamped_to_the_opus_range() {
        let mut dec = OpusDecoder::with_complexity(48_000, 1, 11).unwrap();
        assert_eq!(dec.complexity().unwrap(), 10);
        dec.set_complexity(-3).unwrap();
        assert_eq!(dec.complexity().unwrap(), 0);
        dec.set_complexity(6).unwrap();
        assert_eq!(dec.complexity().unwrap(), 6);
    }
}
