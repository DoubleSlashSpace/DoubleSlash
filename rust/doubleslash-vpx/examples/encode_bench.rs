//! Realtime encode/decode benchmark for VP8 and VP9.
//!
//! Answers the question the codec default depends on: can this device encode
//! each codec fast enough for a live call, and what does each buy in quality
//! per bit? Built for a phone (`cargo ndk ... --example encode_bench`, pushed
//! with adb) but runs anywhere.
//!
//! The content is synthetic — a textured scene under a slow sub-pixel pan, a
//! separately moving foreground, and sensor noise — because a benchmark needs
//! identical input on every run. Real camera video will differ in the details;
//! this is built to stress the same things (motion search, texture, noise), not
//! to be flattering.
//!
//! Usage: `encode_bench [frames] [codec filter] [resolution filter] [clip]`,
//! e.g. `encode_bench 300 vp9 720`. Filters are substrings; `-` or omitted runs
//! everything. `clip` is a path prefix: `<clip>_360p.yuv` and `<clip>_720p.yuv`
//! are read as raw I420 at the preset sizes instead of generating the scene —
//! for real footage, such as the Xiph video-conferencing sequences.

use std::time::{Duration, Instant};

use doubleslash_vpx::{Codec, EncoderConfig, VpxDecoder, VpxEncoder};

/// Frames excluded from timing: the opening keyframe and the rate controller
/// settling are not what a call spends its time doing.
const WARMUP: usize = 30;

struct Preset {
    name: &'static str,
    width: u32,
    height: u32,
    bitrate_bps: u32,
}

/// The client's "balanced" and "high" presets, at 30 fps.
const PRESETS: [Preset; 2] = [
    Preset {
        name: "360p",
        width: 640,
        height: 360,
        bitrate_bps: 600_000,
    },
    Preset {
        name: "720p",
        width: 1280,
        height: 720,
        bitrate_bps: 1_500_000,
    },
];

const FPS: u32 = 30;

struct Case {
    codec: Codec,
    cpu_used: i32,
    threads: u32,
    /// Fraction of the preset bitrate. VP9 is also run below 1.0 to test the
    /// claim that it matches VP8's quality with fewer bits.
    bitrate_scale: f64,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for threads in [1, 2, 4] {
        // VP8 at the speed calls use today.
        out.push(Case {
            codec: Codec::Vp8,
            cpu_used: 8,
            threads,
            bitrate_scale: 1.0,
        });
        for cpu_used in [7, 8, 9] {
            out.push(Case {
                codec: Codec::Vp9,
                cpu_used,
                threads,
                bitrate_scale: 1.0,
            });
        }
        // A bitrate ladder, to find where VP9 matches VP8's quality. Single
        // thread only: tiling moves quality by a few hundredths of a dB, so
        // repeating the ladder per thread count would add runs and no answer.
        if threads == 1 {
            for bitrate_scale in [0.6, 0.75, 0.9] {
                out.push(Case {
                    codec: Codec::Vp9,
                    cpu_used: 8,
                    threads,
                    bitrate_scale,
                });
            }
        }
    }
    out
}

// ── Synthetic scene ─────────────────────────────────────────────────────────

/// Deterministic hash noise in [0, 1).
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((y as u32).wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65536.0
}

/// Smooth value noise at one scale.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Several octaves: large shapes plus fine texture, like a room.
fn texture(x: f32, y: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0 / 96.0;
    for octave in 0..5 {
        sum += amp * value_noise(x * freq, y * freq, seed + octave);
        // Slower falloff than classic fBm keeps fine detail — fabric, hair,
        // foliage — which is what costs bits in a real camera picture.
        amp *= 0.7;
        freq *= 2.0;
    }
    sum
}

/// A plane larger than the frame, sampled with a moving sub-pixel offset.
struct Plane {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Plane {
    fn build(w: usize, h: usize, seed: u32, scale: f32, contrast: f32) -> Self {
        let mut px = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let t = texture(x as f32 * scale, y as f32 * scale, seed);
                // Hard edges too, not only smooth noise: shelves, frames,
                // window bars are what motion search locks onto.
                let edge = if ((x / 97 + y / 61) % 5) == 0 {
                    0.18
                } else {
                    0.0
                };
                let v = 128.0 + (t - 0.5 + edge) * 255.0 * contrast;
                px[y * w + x] = v.clamp(16.0, 235.0) as u8;
            }
        }
        Self { w, h, px }
    }

    /// Bilinear sample, clamped at the edges.
    fn sample(&self, x: f32, y: f32) -> u8 {
        let x = x.clamp(0.0, (self.w - 2) as f32);
        let y = y.clamp(0.0, (self.h - 2) as f32);
        let (xi, yi) = (x as usize, y as usize);
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let p = |xx: usize, yy: usize| self.px[yy * self.w + xx] as f32;
        let top = p(xi, yi) * (1.0 - fx) + p(xi + 1, yi) * fx;
        let bottom = p(xi, yi + 1) * (1.0 - fx) + p(xi + 1, yi + 1) * fx;
        (top * (1.0 - fy) + bottom * fy) as u8
    }
}

struct Scene {
    width: usize,
    height: usize,
    background: [Plane; 3],
    subject: [Plane; 3],
    noise_state: u32,
}

impl Scene {
    fn new(width: usize, height: usize) -> Self {
        // Built at 720p's scale for both sizes, so 360p is the same scene at a
        // lower resolution rather than a different, coarser one.
        let scale = 1280.0 / width as f32;
        let (bw, bh) = (width * 2, height * 2);
        let (cw, ch) = (bw / 2, bh / 2);
        Self {
            width,
            height,
            background: [
                Plane::build(bw, bh, 1, scale, 0.9),
                Plane::build(cw, ch, 20, scale * 2.0, 0.25),
                Plane::build(cw, ch, 40, scale * 2.0, 0.25),
            ],
            subject: [
                Plane::build(width, height, 60, scale, 0.7),
                Plane::build(width / 2, height / 2, 80, scale * 2.0, 0.35),
                Plane::build(width / 2, height / 2, 90, scale * 2.0, 0.35),
            ],
            noise_state: 0x1234_5678,
        }
    }

    fn noise(&mut self) -> i32 {
        // LCG: cheap, deterministic, and only needs to look like sensor grain.
        self.noise_state = self
            .noise_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.noise_state >> 24) % 11) as i32 - 5
    }

    /// Frame `n` as I420.
    fn frame(&mut self, n: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (w, h) = (self.width, self.height);
        let t = n as f32;
        let unit = w as f32 / 1280.0;
        // A slow drifting pan, sub-pixel so motion search has real work.
        let pan_x = (t * 2.2 * unit) % (w as f32 * 0.9);
        let pan_y = (t / 45.0).sin() * 24.0 * unit + h as f32 * 0.4;
        // A foreground subject bobbing and swaying across it.
        let cx = w as f32 * (0.5 + 0.12 * (t / 37.0).sin());
        let cy = h as f32 * (0.55 + 0.05 * (t / 11.0).sin());
        let (rx, ry) = (w as f32 * 0.16, h as f32 * 0.38);

        let mut planes = [vec![0u8; w * h], vec![0u8; w * h / 4], vec![0u8; w * h / 4]];
        for (i, plane) in planes.iter_mut().enumerate() {
            let div = if i == 0 { 1.0 } else { 2.0 };
            let (pw, ph) = (
                w / if i == 0 { 1 } else { 2 },
                h / if i == 0 { 1 } else { 2 },
            );
            for y in 0..ph {
                for x in 0..pw {
                    let (fx, fy) = (x as f32 * div, y as f32 * div);
                    let dx = (fx - cx) / rx;
                    let dy = (fy - cy) / ry;
                    let v = if dx * dx + dy * dy < 1.0 {
                        self.subject[i].sample(
                            (fx - cx + rx) / div + (t / 9.0).sin() * 2.0,
                            (fy - cy + ry) / div,
                        )
                    } else {
                        self.background[i].sample((fx + pan_x) / div, (fy + pan_y) / div)
                    };
                    let v = if i == 0 {
                        (v as i32 + self.noise()).clamp(0, 255) as u8
                    } else {
                        v
                    };
                    plane[y * pw + x] = v;
                }
            }
        }
        let [y, u, v] = planes;
        (y, u, v)
    }
}

// ── Measurement ─────────────────────────────────────────────────────────────

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let d = *x as f64 - *y as f64;
            d * d
        })
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        return 99.0;
    }
    10.0 * (255.0 * 255.0 / mse).log10()
}

fn percentile(sorted: &[Duration], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i].as_secs_f64() * 1000.0
}

fn run(case: &Case, preset: &Preset, frames: &[(Vec<u8>, Vec<u8>, Vec<u8>)]) -> anyhow::Result<()> {
    let bitrate = (preset.bitrate_bps as f64 * case.bitrate_scale) as u32;
    let config = EncoderConfig {
        cpu_used: case.cpu_used,
        threads: case.threads,
        ..EncoderConfig::realtime(case.codec, preset.width, preset.height, bitrate, FPS, 4)
    };
    let mut enc = VpxEncoder::new(case.codec, config)?;
    let mut dec = VpxDecoder::new(case.codec)?;

    let mut encode_times = Vec::with_capacity(frames.len());
    let mut decode_times = Vec::with_capacity(frames.len());
    let mut bytes = 0usize;
    let mut dropped = 0usize;
    let mut psnr_sum = 0.0;
    let mut psnr_n = 0usize;

    for (i, (y, u, v)) in frames.iter().enumerate() {
        let started = Instant::now();
        let (packet, _) = enc.encode(y, u, v)?;
        let encode_time = started.elapsed();

        if i >= WARMUP {
            encode_times.push(encode_time);
            bytes += packet.len();
        }
        if packet.is_empty() {
            if i >= WARMUP {
                dropped += 1;
            }
            continue;
        }

        let started = Instant::now();
        let decoded = dec.decode(&packet)?;
        let decode_time = started.elapsed();
        if let Some(frame) = decoded {
            if i >= WARMUP {
                decode_times.push(decode_time);
                psnr_sum += psnr(&frame.y, y);
                psnr_n += 1;
            }
        }
    }

    let measured = frames.len() - WARMUP;
    encode_times.sort();
    decode_times.sort();
    let mean = |t: &[Duration]| {
        t.iter().map(Duration::as_secs_f64).sum::<f64>() * 1000.0 / t.len().max(1) as f64
    };
    let enc_mean = mean(&encode_times);
    let kbps = bytes as f64 * 8.0 * FPS as f64 / measured as f64 / 1000.0;
    let budget_ms = 1000.0 / FPS as f64;
    // A frame over budget is one the capture thread cannot keep up with.
    let late = encode_times
        .iter()
        .filter(|t| t.as_secs_f64() * 1000.0 > budget_ms)
        .count();

    println!(
        "{:<4} {:<5} cpu{:<2} t{} {:>4.0}%br | enc mean {:>6.2} p50 {:>6.2} p95 {:>6.2} ms  late {:>3}/{} ({:>5.0} fps max) | dec mean {:>5.2} ms | {:>5.0} kbps | PSNR-Y {:>5.2} dB | dropped {}",
        preset.name,
        case.codec.name(),
        case.cpu_used,
        case.threads,
        case.bitrate_scale * 100.0,
        enc_mean,
        percentile(&encode_times, 0.5),
        percentile(&encode_times, 0.95),
        late,
        measured,
        1000.0 / enc_mean.max(0.001),
        mean(&decode_times),
        kbps,
        psnr_sum / psnr_n.max(1) as f64,
        dropped,
    );
    Ok(())
}

type Frame = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Up to `limit` frames of raw I420 at the preset's size.
fn read_clip(path: &str, preset: &Preset, limit: usize) -> anyhow::Result<Vec<Frame>> {
    let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;
    let (w, h) = (preset.width as usize, preset.height as usize);
    let (luma, chroma) = (w * h, w * h / 4);
    Ok(bytes
        .chunks_exact(luma + 2 * chroma)
        .take(limit)
        .map(|f| {
            (
                f[..luma].to_vec(),
                f[luma..luma + chroma].to_vec(),
                f[luma + chroma..].to_vec(),
            )
        })
        .collect())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let frames_n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
    let filter = |i: usize| match args.get(i).map(|s| s.to_lowercase()) {
        Some(f) if f != "-" => f,
        _ => String::new(),
    };
    let codec_filter = filter(2);
    let res_filter = filter(3);
    let clip = args.get(4).cloned();
    anyhow::ensure!(frames_n > WARMUP, "need more than {WARMUP} frames");

    println!(
        "encode_bench: {} frames at {FPS} fps, first {WARMUP} excluded from timing",
        frames_n
    );
    for preset in PRESETS.iter().filter(|p| p.name.contains(&res_filter)) {
        let frames = match &clip {
            Some(prefix) => {
                let path = format!("{prefix}_{}.yuv", preset.name);
                let frames = read_clip(&path, preset, frames_n)?;
                println!("-- {path}: {} frames", frames.len());
                anyhow::ensure!(frames.len() > WARMUP, "{path} is too short");
                frames
            }
            None => {
                eprintln!("generating {} frames of {}...", frames_n, preset.name);
                let mut scene = Scene::new(preset.width as usize, preset.height as usize);
                (0..frames_n).map(|n| scene.frame(n)).collect()
            }
        };

        for case in cases()
            .iter()
            .filter(|c| c.codec.name().to_lowercase().contains(&codec_filter))
        {
            run(case, preset, &frames)?;
            // A short rest between runs, so one config's heat is not charged
            // to the next.
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    Ok(())
}
