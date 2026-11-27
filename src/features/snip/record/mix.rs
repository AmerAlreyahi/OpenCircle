//! Audio mixing for recording: combines microphone and system sound (interleaved
//! stereo 48 kHz `f32` PCM) into one clamped stereo track, turns muted tracks
//! into zeros, fills timestamp gaps with exact silence (for silent WASAPI
//! loopback or device switches), and reports RMS / peak levels for the R1 bar
//! waveform chip.

#![allow(dead_code)] // wired into the recording session in R-04/R-05

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::shared::platform::audio::{CHANNELS, RATE};

/// Default timestamp jitter ignored before inserting silence into a track.
/// Real audio callbacks jitter by a few milliseconds; gaps at or above this
/// threshold are filled to their exact frame length.
pub const DEFAULT_GAP_TOLERANCE: Duration = Duration::from_millis(20);

/// How far behind `now` a track may be before [`Mixer::drain_up_to`] pads it
/// with silence. Packet stamps are estimates and a live source normally lags
/// `now` by one packet (20 ms) plus server latency, so padding right at the
/// tolerance would replace real audio that is merely in flight.
pub const PAD_LATENCY_MARGIN: Duration = Duration::from_millis(100);

/// Converts `duration` at sample rate `rate` (Hz) into an exact rounded frame count.
pub fn frames_for_duration(duration: Duration, rate: u32) -> usize {
    let nanos = duration.as_nanos();
    let frames = (nanos.saturating_mul(u128::from(rate)) + 500_000_000) / 1_000_000_000;
    frames as usize
}

/// Sanitises one sample to `-1.0..=1.0`, mapping non-finite values to `0.0`.
#[inline]
fn clamp_sample(s: f32) -> f32 {
    if s.is_finite() {
        s.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Root-mean-square amplitude of `samples` in `0.0..=1.0` (`0.0` when empty).
/// On a sine wave `A * sin(...)` over complete cycles, returns `A / sqrt(2)`.
pub fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|&s| {
            let v = f64::from(clamp_sample(s));
            v * v
        })
        .sum();
    ((sum_sq / samples.len() as f64).sqrt() as f32).clamp(0.0, 1.0)
}

/// Peak absolute amplitude of `samples` in `0.0..=1.0` (`0.0` when empty).
pub fn peak_level(samples: &[f32]) -> f32 {
    samples
        .iter()
        .map(|&s| clamp_sample(s).abs())
        .fold(0.0f32, f32::max)
}

/// Sums two samples and clamps the result to `-1.0..=1.0` (non-finite -> `0.0`).
#[inline]
fn sum_and_clamp(a: f32, b: f32) -> f32 {
    (clamp_sample(a) + clamp_sample(b)).clamp(-1.0, 1.0)
}

struct Track {
    buf: VecDeque<f32>,
    next_frame: u64,
    muted: bool,
    level: f32,
}

impl Track {
    fn new() -> Self {
        Self {
            buf: VecDeque::new(),
            next_frame: 0,
            muted: false,
            level: 0.0,
        }
    }

    fn frames_buffered(&self) -> usize {
        self.buf.len() / CHANNELS as usize
    }

    fn push(&mut self, samples: &[f32], at_frame: u64, gap_tol_frames: u64) {
        let usable = (samples.len() / CHANNELS as usize) * CHANNELS as usize;
        let total_frames = (usable / CHANNELS as usize) as u64;
        if total_frames == 0 {
            return;
        }

        let slice = if at_frame < self.next_frame {
            // Stamps are estimates, so a small overlap is jitter: keep every
            // sample and append. Only an overlap of at least the tolerance is a
            // real duplicate (e.g. a packet arriving after `pad_to`) and is trimmed.
            let overlap = self.next_frame - at_frame;
            if overlap < gap_tol_frames {
                &samples[..usable]
            } else if overlap >= total_frames {
                return;
            } else {
                &samples[overlap as usize * CHANNELS as usize..usable]
            }
        } else {
            let gap = at_frame - self.next_frame;
            if gap > 0 && gap >= gap_tol_frames {
                let zeros = gap as usize * CHANNELS as usize;
                self.buf.extend(std::iter::repeat_n(0.0f32, zeros));
                self.next_frame += gap;
            }
            &samples[..usable]
        };

        let frames = (slice.len() / CHANNELS as usize) as u64;
        if self.muted {
            self.buf.extend(std::iter::repeat_n(0.0f32, slice.len()));
            self.level = 0.0;
        } else {
            self.buf.extend(slice.iter().map(|&s| clamp_sample(s)));
            self.level = rms_level(slice);
        }
        self.next_frame += frames;
    }

    fn pad_to(&mut self, target_frame: u64, gap_tol_frames: u64) {
        let gap = target_frame.saturating_sub(self.next_frame);
        if gap > 0 && gap >= gap_tol_frames {
            let zeros = gap as usize * CHANNELS as usize;
            self.buf.extend(std::iter::repeat_n(0.0f32, zeros));
            self.next_frame += gap;
            self.level = 0.0;
        }
    }
}

/// Combines microphone and/or system stereo PCM streams into one continuous,
/// clamped stereo output track with mute, timestamp gap silence fill and level
/// metering.
pub struct Mixer {
    rate: u32,
    has_mic: bool,
    has_system: bool,
    gap_tol_frames: u64,
    origin: Option<Instant>,
    mic: Track,
    system: Track,
    mixed_frames: u64,
    level: f32,
    peak: f32,
}

impl Mixer {
    /// Creates a mixer at sample rate `rate` (normally [`RATE`] = 48 kHz) for
    /// the enabled sources (`has_mic`, `has_system`).
    pub fn new(rate: u32, has_mic: bool, has_system: bool) -> Self {
        let rate = rate.max(1);
        let gap_tol_frames = frames_for_duration(DEFAULT_GAP_TOLERANCE, rate) as u64;
        Self {
            rate,
            has_mic,
            has_system,
            gap_tol_frames,
            origin: None,
            mic: Track::new(),
            system: Track::new(),
            mixed_frames: 0,
            level: 0.0,
            peak: 0.0,
        }
    }

    /// Creates a mixer at the standard recording rate (48 kHz).
    pub fn stereo_48k(has_mic: bool, has_system: bool) -> Self {
        Self::new(RATE, has_mic, has_system)
    }

    /// Sets the minimum timestamp gap that triggers silence insertion.
    pub fn with_gap_tolerance(mut self, tolerance: Duration) -> Self {
        self.gap_tol_frames = frames_for_duration(tolerance, self.rate) as u64;
        self
    }

    /// Anchors the timeline origin (for example the start of the recording).
    /// If not called, the first pushed packet's timestamp becomes the origin.
    pub fn start(&mut self, origin: Instant) {
        self.origin = Some(origin);
    }

    /// Mutes or unmutes the microphone track (muted = writes zeros, meter = 0).
    pub fn set_mic_muted(&mut self, muted: bool) {
        self.mic.muted = muted;
        if muted {
            self.mic.level = 0.0;
        }
    }

    pub fn is_mic_muted(&self) -> bool {
        self.mic.muted
    }

    /// Mutes or unmutes the system sound track (muted = writes zeros, meter = 0).
    pub fn set_system_muted(&mut self, muted: bool) {
        self.system.muted = muted;
        if muted {
            self.system.level = 0.0;
        }
    }

    pub fn is_system_muted(&self) -> bool {
        self.system.muted
    }

    /// Feeds an interleaved stereo microphone packet stamped with its start `at`.
    pub fn push_mic(&mut self, samples: &[f32], at: Instant) {
        if !self.has_mic {
            return;
        }
        let at_frame = self.frame_at(at);
        self.mic.push(samples, at_frame, self.gap_tol_frames);
    }

    /// Feeds an interleaved stereo system-sound packet stamped with its start `at`.
    pub fn push_system(&mut self, samples: &[f32], at: Instant) {
        if !self.has_system {
            return;
        }
        let at_frame = self.frame_at(at);
        self.system.push(samples, at_frame, self.gap_tol_frames);
    }

    /// Drains all currently aligned stereo frames into `out`, returning the
    /// number of stereo frames appended.
    pub fn drain(&mut self, out: &mut Vec<f32>) -> usize {
        let frames = match (self.has_mic, self.has_system) {
            (true, true) => self
                .mic
                .frames_buffered()
                .min(self.system.frames_buffered()),
            (true, false) => self.mic.frames_buffered(),
            (false, true) => self.system.frames_buffered(),
            (false, false) => 0,
        };
        self.drain_frames(frames, out)
    }

    /// Pads any enabled track that has fallen behind `now` by more than
    /// [`PAD_LATENCY_MARGIN`] with silence, then drains all aligned frames. Keeps
    /// dual-track recording flowing when system loopback is silent.
    pub fn drain_up_to(&mut self, now: Instant, out: &mut Vec<f32>) -> usize {
        if self.origin.is_none() {
            return 0;
        }
        let margin = frames_for_duration(PAD_LATENCY_MARGIN, self.rate) as u64;
        let target = self.frame_at(now).saturating_sub(margin);
        if self.has_mic {
            self.mic.pad_to(target, self.gap_tol_frames);
        }
        if self.has_system {
            self.system.pad_to(target, self.gap_tol_frames);
        }
        self.drain(out)
    }

    /// Flushes all remaining buffered frames at the end of a recording, padding
    /// the shorter enabled track with zeros.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> usize {
        let frames = match (self.has_mic, self.has_system) {
            (true, true) => self
                .mic
                .frames_buffered()
                .max(self.system.frames_buffered()),
            (true, false) => self.mic.frames_buffered(),
            (false, true) => self.system.frames_buffered(),
            (false, false) => 0,
        };
        self.drain_frames(frames, out)
    }

    fn drain_frames(&mut self, frames: usize, out: &mut Vec<f32>) -> usize {
        if frames == 0 {
            return 0;
        }
        let samples = frames * CHANNELS as usize;
        let start = out.len();
        out.reserve(samples);
        for _ in 0..samples {
            let m = if self.has_mic {
                self.mic.buf.pop_front().unwrap_or(0.0)
            } else {
                0.0
            };
            let s = if self.has_system {
                self.system.buf.pop_front().unwrap_or(0.0)
            } else {
                0.0
            };
            out.push(sum_and_clamp(m, s));
        }
        self.record_drained(&out[start..], frames);
        frames
    }

    /// Most recent mixed output RMS level in `0.0..=1.0` (for the R1 waveform chip).
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Most recent mixed output peak amplitude in `0.0..=1.0`.
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Most recent microphone RMS level in `0.0..=1.0` (`0.0` while muted).
    pub fn mic_level(&self) -> f32 {
        self.mic.level
    }

    /// Most recent system-sound RMS level in `0.0..=1.0` (`0.0` while muted).
    pub fn system_level(&self) -> f32 {
        self.system.level
    }

    /// Total stereo frames drained out of the mixer so far.
    pub fn mixed_frames(&self) -> u64 {
        self.mixed_frames
    }

    fn frame_at(&mut self, at: Instant) -> u64 {
        let origin = *self.origin.get_or_insert(at);
        let elapsed = at.saturating_duration_since(origin);
        frames_for_duration(elapsed, self.rate) as u64
    }

    fn record_drained(&mut self, drained: &[f32], frames: usize) {
        self.mixed_frames += frames as u64;
        self.level = rms_level(drained);
        self.peak = peak_level(drained);
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;

    fn stereo_sine(amp: f32, hz: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn jittered_late_mic_packets_lose_no_samples_and_long_gap_is_filled() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let pkt = 960usize; // 20 ms at 48 kHz
        let mut mixer = Mixer::stereo_48k(true, false);
        mixer.start(t0);
        let (mut out, mut sent) = (Vec::new(), 0usize);
        // 50 packets (1 s), stamps jittered +-5 ms, each arriving 30 ms late.
        let mut queue: Vec<(u64, Instant)> = Vec::new();
        for i in 0..50u64 {
            let jitter = if i % 2 == 0 { 5 } else { 0 };
            let stamp = t0 + ms(i * 20 + jitter) - if i % 2 == 0 { ms(10) } else { ms(0) };
            queue.push((i * 20 + 30, stamp));
        }
        for tick in (0..=1100u64).step_by(10) {
            while queue.first().is_some_and(|(due, _)| *due <= tick) {
                let (_, stamp) = queue.remove(0);
                mixer.push_mic(&vec![0.5; pkt * 2], stamp);
                sent += pkt;
            }
            mixer.drain_up_to(t0 + ms(tick), &mut out);
        }
        mixer.flush(&mut out);
        assert_eq!(
            out.iter().filter(|&&v| v == 0.5).count(),
            sent * 2,
            "no audio replaced"
        );
        let elapsed = frames_for_duration(ms(1000), RATE);
        let tol = frames_for_duration(DEFAULT_GAP_TOLERANCE, RATE) * 3;
        assert!(
            (out.len() / 2).abs_diff(elapsed)
                <= tol.max(frames_for_duration(PAD_LATENCY_MARGIN, RATE))
        );

        // 500 ms of silence from the mic is filled once older than the margin.
        let mut m = Mixer::stereo_48k(true, false);
        m.start(t0);
        let mut o = Vec::new();
        m.push_mic(&vec![0.5; pkt * 2], t0);
        m.drain_up_to(t0 + ms(500), &mut o);
        let want = frames_for_duration(ms(500) - PAD_LATENCY_MARGIN, RATE);
        assert!(o.len() / 2 >= want, "gap filled to now - margin");
    }

    #[test]
    fn mixer_flush_sums_clamps_sanitises_and_pads_shorter_track() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, true);
        mixer.start(t0);
        mixer.push_mic(&[0.3, 0.8, -0.7, f32::NAN], t0);
        mixer.push_system(&[0.2, 0.7, -0.6, 0.25, -0.5, 0.5], t0);

        let mut out = Vec::new();
        let frames = mixer.flush(&mut out);
        assert_eq!(frames, 3);
        assert_eq!(out.len(), 6);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert_eq!(out[1], 1.0, "positive sum clamped");
        assert_eq!(out[2], -1.0, "negative sum clamped");
        assert!((out[3] - 0.25).abs() < 1e-6, "NaN treated as 0");
        assert!((out[4] - -0.5).abs() < 1e-6, "tail from longer track kept");
        assert!((out[5] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mixer_sums_and_clamps_mic_and_system() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, true);
        mixer.start(t0);
        mixer.push_mic(&[0.6, -0.8, 0.2, -0.1], t0);
        mixer.push_system(&[0.7, -0.5, 0.3, 0.4], t0);

        let mut out = Vec::new();
        let frames = mixer.drain(&mut out);
        assert_eq!(frames, 2);
        assert_eq!(mixer.mixed_frames(), 2);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], -1.0);
        assert!((out[2] - 0.5).abs() < 1e-6);
        assert!((out[3] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn mute_writes_zeros_and_clears_meter() {
        let t0 = Instant::now();
        // Mic-only muted writes exact stereo zeros.
        let mut mic_only = Mixer::stereo_48k(true, false);
        mic_only.start(t0);
        mic_only.set_mic_muted(true);
        assert!(mic_only.is_mic_muted());
        mic_only.push_mic(&[0.9, -0.9, 0.5, -0.5], t0);
        assert_eq!(mic_only.mic_level(), 0.0);

        let mut out = Vec::new();
        assert_eq!(mic_only.drain(&mut out), 2);
        assert_eq!(out, vec![0.0, 0.0, 0.0, 0.0]);
        assert_eq!(mic_only.level(), 0.0);

        // Dual-track with mic muted passes system audio through untouched.
        let mut dual = Mixer::stereo_48k(true, true);
        dual.start(t0);
        dual.set_mic_muted(true);
        dual.push_mic(&[0.8, 0.8, -0.8, -0.8], t0);
        dual.push_system(&[0.25, -0.25, 0.5, -0.5], t0);
        assert_eq!(dual.mic_level(), 0.0);
        assert!(dual.system_level() > 0.0);

        let mut dual_out = Vec::new();
        assert_eq!(dual.drain(&mut dual_out), 2);
        assert_eq!(dual_out, vec![0.25, -0.25, 0.5, -0.5]);
    }

    #[test]
    fn silence_gap_is_filled_to_exact_length() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, false);
        mixer.start(t0);

        // 10 ms (480 stereo frames) at t0.
        let chunk_frames = 480usize;
        let first = vec![0.5f32; chunk_frames * 2];
        mixer.push_mic(&first, t0);

        // 100 ms silence gap -> next chunk starts at t0 + 110 ms.
        let gap_ms = 100u64;
        let expected_gap_frames = frames_for_duration(Duration::from_millis(gap_ms), RATE);
        assert_eq!(expected_gap_frames, 4_800);

        let second = vec![-0.25f32; chunk_frames * 2];
        mixer.push_mic(&second, t0 + Duration::from_millis(10 + gap_ms));

        let mut out = Vec::new();
        let frames = mixer.drain(&mut out);
        assert_eq!(frames, chunk_frames + expected_gap_frames + chunk_frames);
        assert_eq!(out.len(), frames * 2);

        let first_end = chunk_frames * 2;
        let gap_end = (chunk_frames + expected_gap_frames) * 2;
        assert!(out[..first_end].iter().all(|&s| (s - 0.5).abs() < 1e-6));
        assert!(
            out[first_end..gap_end].iter().all(|&s| s == 0.0),
            "exact silence in gap"
        );
        assert!(out[gap_end..].iter().all(|&s| (s - -0.25).abs() < 1e-6));
    }

    #[test]
    fn leading_gap_from_origin_is_filled_with_silence() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(false, true);
        mixer.start(t0);

        // System sound starts 250 ms after recording origin (12,000 stereo frames).
        let pkt = vec![0.4f32; 200];
        mixer.push_system(&pkt, t0 + Duration::from_millis(250));

        let mut out = Vec::new();
        let frames = mixer.drain(&mut out);
        assert_eq!(frames, 12_000 + 100);
        assert!(out[..24_000].iter().all(|&s| s == 0.0));
        assert!(out[24_000..].iter().all(|&s| (s - 0.4).abs() < 1e-6));
    }

    #[test]
    fn small_jitter_below_tolerance_inserts_no_silence() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, false);
        mixer.start(t0);

        let chunk = vec![0.3f32; 480 * 2]; // 10 ms
        mixer.push_mic(&chunk, t0);
        // Arrives 5 ms late (< 20 ms default tolerance): treated as contiguous.
        mixer.push_mic(&chunk, t0 + Duration::from_millis(15));

        let mut out = Vec::new();
        let frames = mixer.drain(&mut out);
        assert_eq!(frames, 960);
    }

    #[test]
    fn drain_up_to_pads_silent_loopback_while_mic_streams() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, true);
        mixer.start(t0);

        // Mic delivers 200 ms (9,600 frames); system loopback sends nothing.
        let mic = vec![0.5f32; 9_600 * 2];
        mixer.push_mic(&mic, t0);

        let mut out = Vec::new();
        assert_eq!(mixer.drain(&mut out), 0, "waiting without now");
        let drained = mixer.drain_up_to(t0 + Duration::from_millis(300), &mut out);
        assert_eq!(drained, 9_600);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn late_and_overlapping_packets_are_trimmed_to_elapsed_time() {
        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, true);
        mixer.start(t0);

        // 200 ms of mic audio; system loopback is silent and gets padded to 200 ms.
        let mic_200 = vec![0.5f32; 9_600 * 2];
        mixer.push_mic(&mic_200, t0);
        let mut out = Vec::new();
        assert_eq!(
            mixer.drain_up_to(t0 + Duration::from_millis(300), &mut out),
            9_600
        );

        // 1) Completely late system packet covering [50 ms, 100 ms) arrives after pad_to(200 ms):
        // must be dropped completely so no phantom time is appended.
        let stale = vec![0.9f32; 2_400 * 2];
        mixer.push_system(&stale, t0 + Duration::from_millis(50));
        out.clear();
        assert_eq!(
            mixer.flush(&mut out),
            0,
            "fully late packet must be dropped"
        );
        assert_eq!(mixer.mixed_frames(), 9_600);

        // 2) Partially overlapping system packet covering [180 ms, 230 ms) (20 ms stale + 30 ms new):
        // first 20 ms (960 frames of 0.9) must be trimmed; only the 30 ms tail (1,440 frames of 0.25)
        // joins the 30 ms of mic audio at [200 ms, 230 ms).
        let mut partial = vec![0.9f32; 960 * 2];
        partial.extend(std::iter::repeat_n(0.25f32, 1_440 * 2));
        mixer.push_system(&partial, t0 + Duration::from_millis(180));
        mixer.push_mic(&vec![0.1f32; 1_440 * 2], t0 + Duration::from_millis(200));

        out.clear();
        let drained = mixer.drain(&mut out);
        assert_eq!(drained, 1_440, "only non-overlapping 30 ms tail kept");
        assert_eq!(
            mixer.mixed_frames(),
            frames_for_duration(Duration::from_millis(230), RATE) as u64,
            "total mixed length must equal elapsed time"
        );
        assert!(
            out.iter().all(|&s| (s - 0.35).abs() < 1e-6),
            "no stale overlapped 0.9 samples may leak into output"
        );

        // 3) Direct overlapping push on a single track trims the overlap without duplication.
        let mut single =
            Mixer::stereo_48k(true, false).with_gap_tolerance(Duration::from_millis(5)); // overlap >= tolerance is trimmed
        single.start(t0);
        single.push_mic(&vec![0.4f32; 960 * 2], t0); // [0, 20 ms)
        let mut overlap_pkt = vec![0.9f32; 480 * 2]; // [10 ms, 20 ms) overlap
        overlap_pkt.extend(std::iter::repeat_n(-0.3f32, 480 * 2)); // [20 ms, 30 ms) new
        single.push_mic(&overlap_pkt, t0 + Duration::from_millis(10));

        out.clear();
        let frames = single.drain(&mut out);
        assert_eq!(
            frames, 1_440,
            "30 ms elapsed -> 1,440 frames, no duplicates"
        );
        assert!(out[..960 * 2].iter().all(|&s| (s - 0.4).abs() < 1e-6));
        assert!(out[960 * 2..].iter().all(|&s| (s - -0.3).abs() < 1e-6));
    }

    #[test]
    fn meter_reports_known_rms_and_peak_on_sine() {
        // 1 kHz sine at 48 kHz for 100 ms = 100 exact cycles (4,800 frames).
        let amp = 0.8f32;
        let sine = stereo_sine(amp, 1_000.0, RATE, 4_800);
        let expected_rms = amp * FRAC_1_SQRT_2;

        let rms = rms_level(&sine);
        let peak = peak_level(&sine);
        assert!(
            (rms - expected_rms).abs() < 1e-4,
            "rms={rms} expected={expected_rms}"
        );
        assert!((peak - amp).abs() < 1e-4, "peak={peak} expected={amp}");

        let t0 = Instant::now();
        let mut mixer = Mixer::stereo_48k(true, false);
        mixer.start(t0);
        mixer.push_mic(&sine, t0);
        assert!((mixer.mic_level() - expected_rms).abs() < 1e-4);

        let mut out = Vec::new();
        assert_eq!(mixer.drain(&mut out), 4_800);
        assert!((mixer.level() - expected_rms).abs() < 1e-4);
        assert!((mixer.peak() - amp).abs() < 1e-4);

        // Silence drops meter back to zero.
        let silence = vec![0.0f32; 480 * 2];
        mixer.push_mic(&silence, t0 + Duration::from_millis(100));
        assert_eq!(mixer.mic_level(), 0.0);
        out.clear();
        mixer.drain(&mut out);
        assert_eq!(mixer.level(), 0.0);
        assert_eq!(mixer.peak(), 0.0);
    }
}
