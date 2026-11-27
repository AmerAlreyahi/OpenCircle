//! Audio resampling and channel downmixing:
//! - [`Resampler`]: streaming linear resampler + mono/stereo/5.1/7.1 downmix to
//!   interleaved stereo `f32` PCM (for WASAPI capture -> 48 kHz stereo).
//! - [`Downmix`]: box-lowpass mono `i16` downmixer (used by song search to turn
//!   48 kHz stereo into 12 kHz mono without aliasing).

use std::f32::consts::FRAC_1_SQRT_2;

/// Downmixes one interleaved input frame of `channels` samples to stereo `[L, R]`.
///
/// Channel layouts follow standard SMPTE / WAVEFORMATEXTENSIBLE / PulseAudio order:
/// - 1 (mono): `[m, m]`
/// - 2 (stereo): `[l, r]`
/// - 3 (3.0 / 2.1): FL, FR, FC -> center at -3 dB into both
/// - 4 (quad): FL, FR, BL, BR -> surrounds at -3 dB
/// - 6 (5.1): FL, FR, FC, LFE, BL, BR -> ITU-R BS.775 (`L = FL + 0.707*FC + 0.707*BL`,
///   `R = FR + 0.707*FC + 0.707*BR`, LFE omitted)
/// - 8 (7.1): FL, FR, FC, LFE, BL, BR, SL, SR -> center and surrounds at -3 dB
/// - other N: even channels averaged into L, odd channels averaged into R
#[inline]
pub fn frame_to_stereo(frame: &[f32]) -> [f32; 2] {
    match frame.len() {
        0 => [0.0, 0.0],
        1 => [frame[0].clamp(-1.0, 1.0), frame[0].clamp(-1.0, 1.0)],
        2 => [frame[0].clamp(-1.0, 1.0), frame[1].clamp(-1.0, 1.0)],
        3 => {
            let c = frame[2] * FRAC_1_SQRT_2;
            [
                (frame[0] + c).clamp(-1.0, 1.0),
                (frame[1] + c).clamp(-1.0, 1.0),
            ]
        }
        4 => [
            (frame[0] + frame[2] * FRAC_1_SQRT_2).clamp(-1.0, 1.0),
            (frame[1] + frame[3] * FRAC_1_SQRT_2).clamp(-1.0, 1.0),
        ],
        6 => {
            let c = frame[2] * FRAC_1_SQRT_2;
            let l = frame[0] + c + frame[4] * FRAC_1_SQRT_2;
            let r = frame[1] + c + frame[5] * FRAC_1_SQRT_2;
            [l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)]
        }
        8 => {
            let c = frame[2] * FRAC_1_SQRT_2;
            let l = frame[0] + c + (frame[4] + frame[6]) * FRAC_1_SQRT_2;
            let r = frame[1] + c + (frame[5] + frame[7]) * FRAC_1_SQRT_2;
            [l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)]
        }
        _ => {
            let (mut l, mut r, mut nl, mut nr) = (0.0f32, 0.0f32, 0u32, 0u32);
            for (i, &s) in frame.iter().enumerate() {
                if i % 2 == 0 {
                    l += s;
                    nl += 1;
                } else {
                    r += s;
                    nr += 1;
                }
            }
            let l = if nl > 0 { l / nl as f32 } else { 0.0 };
            let r = if nr > 0 { r / nr as f32 } else { 0.0 };
            [l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)]
        }
    }
}

/// Downmixes a slice of interleaved `channels`-channel samples into stereo `[L, R]`
/// appended to `out`. Any trailing incomplete frame is ignored.
#[allow(dead_code)] // public helper tested and available for callers
pub fn downmix_to_stereo(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let ch = channels.max(1) as usize;
    let frames = interleaved.len() / ch;
    out.reserve(frames * 2);
    for frame in interleaved.chunks_exact(ch) {
        let [l, r] = frame_to_stereo(frame);
        out.push(l);
        out.push(r);
    }
}

/// Streaming linear resampler + channel downmixer to interleaved stereo `f32`.
///
/// Uses reduced integer rational phase (`in_step / out_step`) so long streams
/// (44.1 -> 48 kHz, 96 -> 48 kHz, etc.) never accumulate floating-point drift.
#[allow(dead_code)] // used on Windows (WASAPI) and in tests
pub struct Resampler {
    channels: usize,
    in_step: u64,
    out_step: u64,
    /// Phase in units of `1 / out_step` of an input frame, relative to `prev`.
    phase: u64,
    prev: [f32; 2],
    has_prev: bool,
    /// Leftover samples when a pushed chunk is not a multiple of `channels`.
    partial: Vec<f32>,
}

#[allow(dead_code)]
impl Resampler {
    /// Creates a resampler from `(in_rate, in_channels)` to stereo at `out_rate`.
    pub fn new(in_rate: u32, in_channels: u16, out_rate: u32) -> Self {
        let in_r = u64::from(in_rate.max(1));
        let out_r = u64::from(out_rate.max(1));
        let g = gcd(in_r, out_r);
        let channels = in_channels.max(1) as usize;
        Self {
            channels,
            in_step: in_r / g,
            out_step: out_r / g,
            phase: 0,
            prev: [0.0, 0.0],
            has_prev: false,
            partial: Vec::with_capacity(channels),
        }
    }

    /// Pushes interleaved input samples in `-1.0..=1.0`, appending interleaved
    /// stereo `f32` output frames to `out`.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>, out: &mut Vec<f32>) {
        let ch = self.channels;
        for s in samples {
            self.partial.push(s);
            if self.partial.len() < ch {
                continue;
            }
            let curr = frame_to_stereo(&self.partial);
            self.partial.clear();
            self.push_stereo_frame(curr, out);
        }
    }

    /// Flushes any final fractional output frame at end-of-stream (holding the
    /// last input frame) so an exact integer-second input produces the exact
    /// target frame count (for example 44,100 -> 48,000 frames).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if self.in_step == self.out_step || !self.has_prev {
            return;
        }
        while self.phase < self.out_step {
            out.push(self.prev[0]);
            out.push(self.prev[1]);
            self.phase += self.in_step;
        }
    }

    fn push_stereo_frame(&mut self, curr: [f32; 2], out: &mut Vec<f32>) {
        if self.in_step == self.out_step {
            out.push(curr[0]);
            out.push(curr[1]);
            return;
        }
        if !self.has_prev {
            self.prev = curr;
            self.has_prev = true;
            out.push(curr[0]);
            out.push(curr[1]);
            self.phase = self.in_step;
            return;
        }
        while self.phase < self.out_step {
            let t = self.phase as f32 / self.out_step as f32;
            let l = self.prev[0] + (curr[0] - self.prev[0]) * t;
            let r = self.prev[1] + (curr[1] - self.prev[1]) * t;
            out.push(l.clamp(-1.0, 1.0));
            out.push(r.clamp(-1.0, 1.0));
            self.phase += self.in_step;
        }
        self.prev = curr;
        self.phase -= self.out_step;
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

/// Turns any capture format into mono `i16` at `out_rate`: channels are averaged,
/// and each output sample is the mean of the input frames in its period (a box
/// low-pass, so 48 kHz down to 12 kHz does not alias much).
pub struct Downmix {
    channels: usize,
    /// Input frames per output sample.
    step: f64,
    /// Input frames since the last output sample, and their sum.
    pos: f64,
    sum: f64,
    n: u32,
    pub out: Vec<i16>,
}

impl Downmix {
    pub fn new(in_rate: u32, channels: u16, out_rate: u32, capacity: usize) -> Self {
        Self {
            channels: channels.max(1) as usize,
            step: f64::from(in_rate) / f64::from(out_rate.max(1)),
            pos: 0.0,
            sum: 0.0,
            n: 0,
            out: Vec::with_capacity(capacity),
        }
    }

    /// Interleaved samples in -1..1.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        let mut frame = 0.0;
        let mut i = 0;
        for s in samples {
            frame += s;
            i += 1;
            if i < self.channels {
                continue;
            }
            self.sum += f64::from(frame) / self.channels as f64;
            self.n += 1;
            self.pos += 1.0;
            (frame, i) = (0.0, 0);
            if self.pos >= self.step && self.out.len() < self.out.capacity() {
                let v = (self.sum / f64::from(self.n)).clamp(-1.0, 1.0);
                self.out.push((v * 32767.0) as i16);
                self.pos -= self.step;
                (self.sum, self.n) = (0.0, 0);
            }
        }
    }

    pub fn full(&self) -> bool {
        self.out.len() >= self.out.capacity()
    }
}
