//! Local speech for the Ollama assistant: TTS into the call, STT of others.
//!
//! Ollama does not synthesise speech. On Windows we use the OS
//! `SpeechSynthesizer` and inject 48 kHz mono PCM into the live capture path.
//! Optional listening transcribes remote Opus (already decoded for playout)
//! via Ollama `/v1/audio/transcriptions` when `ollama_stt_model` is set.
//!
//! Off unless `ollama_voice_enabled`. Peer audio never leaves this machine
//! except to the configured local Ollama URL.

use std::collections::HashMap;

use serde_json::json;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::call_controller::{CallCommand, FADE_SAMPLES, SAMPLES_PER_FRAME, SAMPLE_RATE};
use crate::content_capture::{f32_to_i16, resample_linear};
use crate::ollama_module::normalize_ollama_base_url;

/// Soft cap so a long chat reply cannot block the voice path for minutes.
pub const MAX_SPEAK_CHARS: usize = 500;
/// Drop a finished utterance longer than this (8 s at 48 kHz).
const MAX_UTTERANCE_SAMPLES: usize = SAMPLE_RATE as usize * 8;
const START_RMS: f32 = 400.0;
const START_FRAMES: u32 = 3;
const END_SILENCE_FRAMES: u32 = 25;

/// Per-peer endpointer used to turn decoded 20 ms frames into utterances.
#[derive(Debug, Default)]
pub struct UtteranceCollector {
    peers: HashMap<String, UtterancePeer>,
}

#[derive(Debug, Default)]
struct UtterancePeer {
    buf: Vec<i16>,
    voiced: u32,
    silent: u32,
    active: bool,
}

impl UtteranceCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one 20 ms decoded frame. Returns PCM when a turn ends.
    pub fn push(&mut self, peer_id: &str, pcm: &[i16]) -> Option<Vec<i16>> {
        if pcm.is_empty() {
            return None;
        }
        let rms = frame_rms(pcm);
        let slot = self.peers.entry(peer_id.to_owned()).or_default();
        if rms >= START_RMS {
            slot.voiced = slot.voiced.saturating_add(1);
            slot.silent = 0;
            if !slot.active && slot.voiced >= START_FRAMES {
                slot.active = true;
                slot.buf.clear();
            }
        } else {
            slot.silent = slot.silent.saturating_add(1);
            slot.voiced = 0;
        }
        if slot.active {
            slot.buf.extend_from_slice(pcm);
            if slot.buf.len() >= MAX_UTTERANCE_SAMPLES {
                slot.active = false;
                slot.silent = 0;
                return Some(std::mem::take(&mut slot.buf));
            }
            if slot.silent >= END_SILENCE_FRAMES {
                slot.active = false;
                slot.silent = 0;
                let out = std::mem::take(&mut slot.buf);
                if out.len() < SAMPLES_PER_FRAME * 4 {
                    return None;
                }
                return Some(out);
            }
        }
        None
    }
}

fn frame_rms(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / pcm.len() as f64).sqrt() as f32
}

/// Fade the first/last 5 ms so injected TTS does not click.
pub fn apply_edge_fade(pcm: &mut [i16]) {
    let n = FADE_SAMPLES.min(pcm.len() / 2);
    if n == 0 {
        return;
    }
    for i in 0..n {
        let g = i as f32 / n as f32;
        pcm[i] = (pcm[i] as f32 * g) as i16;
        let j = pcm.len() - 1 - i;
        pcm[j] = (pcm[j] as f32 * g) as i16;
    }
}

/// 48 kHz mono PCM for the capture injector. Windows SAPI; other OS: error.
pub fn synthesize_pcm(text: &str) -> Result<Vec<i16>, String> {
    let t = truncate_speak(text);
    if t.is_empty() {
        return Err("empty speech text".into());
    }
    #[cfg(target_os = "windows")]
    {
        let mut pcm = windows_synthesize(&t)?;
        apply_edge_fade(&mut pcm);
        Ok(pcm)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = t;
        Err("speech synthesis is currently Windows-only".into())
    }
}

fn truncate_speak(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= MAX_SPEAK_CHARS {
        return t.to_owned();
    }
    format!(
        "{}…",
        t.chars()
            .take(MAX_SPEAK_CHARS.saturating_sub(1))
            .collect::<String>()
    )
}

/// WAV bytes (16-bit PCM) covering `pcm` at 48 kHz, downsampled to 16 kHz for STT.
pub fn utterance_wav_16k(pcm_48k: &[i16]) -> Vec<u8> {
    let f32s: Vec<f32> = pcm_48k.iter().map(|&s| s as f32 / 32768.0).collect();
    let down = downsample_3(&f32s);
    let i16s = f32_to_i16(&down);
    write_wav_pcm16(&i16s, 16_000, 1)
}

fn le2(b: &[u8], off: usize) -> Result<[u8; 2], String> {
    let s = b
        .get(off..off + 2)
        .ok_or_else(|| "truncated WAV".to_string())?;
    s.try_into().map_err(|_| "truncated WAV".to_string())
}

fn le4(b: &[u8], off: usize) -> Result<[u8; 4], String> {
    let s = b
        .get(off..off + 4)
        .ok_or_else(|| "truncated WAV".to_string())?;
    s.try_into().map_err(|_| "truncated WAV".to_string())
}

/// 48 kHz → 16 kHz by averaging groups of 3 (exact integer ratio).
fn downsample_3(input: &[f32]) -> Vec<f32> {
    input
        .chunks(3)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

pub fn write_wav_pcm16(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * 2;
    out.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * 2;
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Parse a PCM WAV (the usual SAPI output) into 48 kHz mono i16.
pub fn pcm16_from_wav(bytes: &[u8]) -> Result<Vec<i16>, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // format, ch, rate, bits
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(le4(bytes, pos + 4)?) as usize;
        let start = pos + 8;
        let end = start.saturating_add(len).min(bytes.len());
        if id == b"fmt " && end - start >= 16 {
            let format = u16::from_le_bytes(le2(bytes, start)?);
            let ch = u16::from_le_bytes(le2(bytes, start + 2)?);
            let rate = u32::from_le_bytes(le4(bytes, start + 4)?);
            let bits = u16::from_le_bytes(le2(bytes, start + 14)?);
            fmt = Some((format, ch, rate, bits));
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        pos = end + (len % 2); // chunks are word-aligned
    }
    let (format, ch, rate, bits) = fmt.ok_or_else(|| "WAV missing fmt chunk".to_string())?;
    if format != 1 {
        return Err(format!("WAV codec {format} is not PCM"));
    }
    if bits != 16 {
        return Err(format!("WAV bits {bits} is not 16"));
    }
    let payload = data.ok_or_else(|| "WAV missing data chunk".to_string())?;
    if payload.len() < 2 {
        return Err("WAV data is empty".into());
    }
    let mut mono = Vec::with_capacity(payload.len() / 2);
    let stride = usize::from(ch.max(1));
    let mut i = 0;
    while i + 2 * stride <= payload.len() {
        let mut acc = 0i32;
        for c in 0..stride {
            let off = i + c * 2;
            acc += i16::from_le_bytes(le2(payload, off)?) as i32;
        }
        mono.push((acc / stride as i32) as i16);
        i += 2 * stride;
    }
    if rate == SAMPLE_RATE || rate == 0 {
        return Ok(mono);
    }
    let f32s: Vec<f32> = mono.iter().map(|&s| s as f32 / 32768.0).collect();
    Ok(f32_to_i16(&resample_linear(&f32s, rate)))
}

/// Transcribe a 48 kHz utterance using configured `ollama_stt_model`.
pub async fn transcribe_utterance(pcm_48k: &[i16]) -> Result<String, String> {
    let settings = crate::ollama_module::read_assistant_settings();
    if !settings.voice_enabled || settings.stt_model.trim().is_empty() {
        return Err("voice STT is off".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default();
    let wav = utterance_wav_16k(pcm_48k);
    transcribe_wav(&client, &settings.base_url, &settings.stt_model, wav).await
}

/// POST a 16 kHz WAV to Ollama's OpenAI-compatible transcriptions endpoint.
pub async fn transcribe_wav(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    wav: Vec<u8>,
) -> Result<String, String> {
    if model.trim().is_empty() {
        return Err("no STT model configured (ollama_stt_model)".into());
    }
    let base = normalize_ollama_base_url(base_url);
    let url = format!("{base}/v1/audio/transcriptions");
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("utterance.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("model", model.trim().to_owned())
        .part("file", part);
    let resp = client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("STT HTTP error: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("STT {status}: {body}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("STT parse: {e}"))?;
    let text = v
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    Ok(text)
}

pub fn should_auto_reply_transcript(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() < 8 {
        return false;
    }
    t.split_whitespace().count() >= 2
}

#[cfg(target_os = "windows")]
fn windows_synthesize(text: &str) -> Result<Vec<i16>, String> {
    use windows::core::{Interface, HSTRING};
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::{Buffer, InputStreamOptions};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let synth = SpeechSynthesizer::new().map_err(|e| format!("SpeechSynthesizer: {e}"))?;
    let op = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))
        .map_err(|e| format!("synthesize: {e}"))?;
    let stream = op.get().map_err(|e| format!("synthesize wait: {e}"))?;
    let size = stream.Size().map_err(|e| format!("stream size: {e}"))?;
    if size == 0 || size > 20 * 1024 * 1024 {
        return Err(format!("unexpected TTS stream size {size}"));
    }
    let buf = Buffer::Create(size as u32).map_err(|e| format!("tts buffer: {e}"))?;
    stream
        .ReadAsync(&buf, size as u32, InputStreamOptions::None)
        .map_err(|e| format!("tts read: {e}"))?
        .get()
        .map_err(|e| format!("tts read wait: {e}"))?;
    let reader = windows::Storage::Streams::DataReader::FromBuffer(&buf)
        .map_err(|e| format!("tts reader: {e}"))?;
    let mut bytes = vec![0u8; size as usize];
    reader
        .ReadBytes(&mut bytes)
        .map_err(|e| format!("tts bytes: {e}"))?;
    let _ = Interface::cast::<windows::core::IUnknown>(&stream);
    info!("[voice] TTS produced {} bytes", bytes.len());
    pcm16_from_wav(&bytes)
}

/// Debug helper used by tools: seconds of 48 kHz PCM.
pub fn pcm_seconds(pcm: &[i16]) -> f32 {
    pcm.len() as f32 / SAMPLE_RATE as f32
}

pub fn speak_status_json(pcm: &[i16]) -> serde_json::Value {
    json!({
        "ok": true,
        "queued": true,
        "seconds": (pcm_seconds(pcm) * 10.0).round() / 10.0,
        "samples": pcm.len(),
    })
}

/// Synthesize `text` on a blocking thread and inject it into the live call.
pub fn spawn_speak(call_tx: mpsc::Sender<CallCommand>, text: String) {
    if text.trim().is_empty() {
        return;
    }
    let settings = crate::ollama_module::read_assistant_settings();
    if !settings.voice_enabled {
        return;
    }
    info!("[voice] speaking {} chars", text.chars().count());
    tokio::task::spawn_blocking(move || match synthesize_pcm(&text) {
        Ok(pcm) => {
            let _ = call_tx.blocking_send(CallCommand::SetAgentVoice(true));
            if let Err(e) = call_tx.blocking_send(CallCommand::EnqueueSpeakPcm { pcm }) {
                warn!("[voice] enqueue TTS failed: {e}");
            }
        }
        Err(e) => warn!("[voice] TTS failed: {e}"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_48k_mono() {
        let src: Vec<i16> = (0..480).map(|i| (i * 17) as i16).collect();
        let wav = write_wav_pcm16(&src, 48_000, 1);
        let back = pcm16_from_wav(&wav).expect("parse");
        assert_eq!(back, src);
    }

    #[test]
    fn downsample_3_triples_to_one() {
        let in48: Vec<f32> = (0..9).map(|i| i as f32).collect();
        let out = downsample_3(&in48);
        assert_eq!(out.len(), 3);
        assert!((out[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn utterance_ends_after_silence() {
        let mut c = UtteranceCollector::new();
        let loud = vec![2000i16; SAMPLES_PER_FRAME];
        let quiet = vec![0i16; SAMPLES_PER_FRAME];
        for _ in 0..5 {
            assert!(c.push("p", &loud).is_none());
        }
        let mut done = None;
        for _ in 0..END_SILENCE_FRAMES {
            done = c.push("p", &quiet);
            if done.is_some() {
                break;
            }
        }
        let pcm = done.expect("utterance");
        assert!(pcm.len() >= SAMPLES_PER_FRAME * 4);
    }

    #[test]
    fn short_transcripts_are_ignored() {
        assert!(!should_auto_reply_transcript("uh"));
        assert!(!should_auto_reply_transcript("ok"));
        assert!(should_auto_reply_transcript("hello there everyone"));
    }

    #[test]
    fn fade_does_not_change_length() {
        let mut pcm: Vec<i16> = (0..2000).map(|i| 1000 + i as i16).collect();
        let n = pcm.len();
        apply_edge_fade(&mut pcm);
        assert_eq!(pcm.len(), n);
        assert_eq!(pcm[0], 0);
    }
}
