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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::platform::audio::RATE;

    fn sine_interleaved(hz: f32, rate: u32, channels: u16, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                std::iter::repeat_n(v, channels as usize)
            })
            .collect()
    }

    fn zero_crossings_channel(stereo: &[f32], ch: usize) -> usize {
        let vals: Vec<f32> = stereo.as_chunks::<2>().0.iter().map(|f| f[ch]).collect();
        vals.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn resample_44100_to_48000_exact_length_and_pitch() {
        let input = sine_interleaved(1000.0, 44_100, 2, 44_100);
        let mut r = Resampler::new(44_100, 2, RATE);
        let mut out = Vec::new();
        r.push(input, &mut out);
        r.flush(&mut out);

        assert_eq!(out.len(), RATE as usize * 2, "1 s at 44.1k -> 48k stereo");
        let crossings_l = zero_crossings_channel(&out, 0) as f32;
        let crossings_r = zero_crossings_channel(&out, 1) as f32;
        assert!(
            (crossings_l - 2000.0).abs() <= 4.0,
            "L crossings: {crossings_l}"
        );
        assert!(
            (crossings_r - 2000.0).abs() <= 4.0,
            "R crossings: {crossings_r}"
        );
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.45 && peak <= 0.51, "peak preserved: {peak}");
    }

    #[test]
    fn resample_48000_passthrough_is_exact() {
        let input = sine_interleaved(440.0, 48_000, 2, 48_000);
        let mut r = Resampler::new(48_000, 2, RATE);
        let mut out = Vec::new();
        r.push(input.iter().copied(), &mut out);
        r.flush(&mut out);

        assert_eq!(out.len(), input.len());
        assert_eq!(out, input);
    }

    #[test]
    fn resample_96000_to_48000_exact_length_and_pitch() {
        let input = sine_interleaved(1000.0, 96_000, 2, 96_000);
        let mut r = Resampler::new(96_000, 2, RATE);
        let mut out = Vec::new();
        r.push(input, &mut out);
        r.flush(&mut out);

        assert_eq!(out.len(), RATE as usize * 2, "1 s at 96k -> 48k stereo");
        let crossings = zero_crossings_channel(&out, 0) as f32;
        assert!((crossings - 2000.0).abs() <= 4.0, "crossings: {crossings}");
    }

    #[test]
    fn downmix_mono_duplicates_to_both_channels() {
        let mut r = Resampler::new(RATE, 1, RATE);
        let mut out = Vec::new();
        r.push([0.25, -0.75, 1.5], &mut out);
        r.flush(&mut out);
        assert_eq!(out, vec![0.25, 0.25, -0.75, -0.75, 1.0, 1.0]);
    }

    #[test]
    fn downmix_5_1_to_stereo_follows_itu_bs775() {
        let eps = 1e-6;
        // [FL, FR, FC, LFE, BL, BR]
        let fl = frame_to_stereo(&[0.8, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!((fl[0] - 0.8).abs() < eps && fl[1].abs() < eps);

        let fr = frame_to_stereo(&[0.0, 0.6, 0.0, 0.0, 0.0, 0.0]);
        assert!(fr[0].abs() < eps && (fr[1] - 0.6).abs() < eps);

        let fc = frame_to_stereo(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!((fc[0] - FRAC_1_SQRT_2).abs() < eps && (fc[1] - FRAC_1_SQRT_2).abs() < eps);

        let lfe = frame_to_stereo(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        assert!(lfe[0].abs() < eps && lfe[1].abs() < eps, "LFE omitted");

        let bl = frame_to_stereo(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        assert!((bl[0] - FRAC_1_SQRT_2).abs() < eps && bl[1].abs() < eps);

        let br = frame_to_stereo(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(br[0].abs() < eps && (br[1] - FRAC_1_SQRT_2).abs() < eps);

        // Full-scale sum clamps to [-1, 1].
        let clipped = frame_to_stereo(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(clipped, [1.0, 1.0]);

        // Multi-frame 5.1 -> stereo through the production Resampler path.
        let mut r = Resampler::new(RATE, 6, RATE);
        let mut stereo = Vec::new();
        r.push(
            [
                0.5, 0.0, 0.0, 0.0, 0.0, 0.0, // frame 0: FL only
                0.0, 0.5, 0.0, 0.0, 0.0, 0.0, // frame 1: FR only
            ],
            &mut stereo,
        );
        r.flush(&mut stereo);
        assert_eq!(stereo, vec![0.5, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn resample_5_1_at_44100_is_chunk_invariant() {
        // 1 s of 5.1 at 44.1 kHz
        let input = sine_interleaved(500.0, 44_100, 6, 44_100);

        let mut whole = Resampler::new(44_100, 6, RATE);
        let mut out_whole = Vec::new();
        whole.push(input.iter().copied(), &mut out_whole);
        whole.flush(&mut out_whole);

        let mut chunked = Resampler::new(44_100, 6, RATE);
        let mut out_chunked = Vec::new();
        for chunk in input.chunks(137) {
            chunked.push(chunk.iter().copied(), &mut out_chunked);
        }
        chunked.flush(&mut out_chunked);

        assert_eq!(out_whole.len(), RATE as usize * 2);
        assert_eq!(out_whole, out_chunked);
    }
}
