//! Bounded callback-to-reader channel for push-based audio backends (WASAPI):
//! - fixed-size chunks + a pre-allocated reusable scratch buffer keep the
//!   real-time audio callback free of per-callback `Vec` allocations
//! - a sticky error flag + message slot ensures a device-lost error is never
//!   dropped when the bounded channel is full, so `read` always reports
//!   [`Error::Unavailable`] instead of timing out forever.

#![cfg_attr(not(windows), allow(dead_code))] // used by `wasapi.rs` on Windows and by tests everywhere

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::resample::Resampler;
use super::{CHANNELS, Error, Packet, RATE, frames_duration};

/// Stereo samples per fixed chunk transferred from a real-time audio callback
/// (960 stereo frames = 20 ms at 48 kHz = 7,680 bytes).
pub(super) const CALLBACK_CHUNK_SAMPLES: usize = (RATE as usize / 1000) * CHANNELS as usize * 20;

/// Bounded channel depth between the real-time audio callback and the reader
/// thread. At ~10-20 ms per callback, 64 chunks provide ~0.64-1.28 s of cushion
/// with bounded memory.
pub(super) const CALLBACK_CHANNEL_CAP: usize = 64;

struct SharedError {
    failed: AtomicBool,
    message: Mutex<Option<String>>,
}

impl SharedError {
    fn new() -> Self {
        Self {
            failed: AtomicBool::new(false),
            message: Mutex::new(None),
        }
    }

    fn set(&self, msg: String) {
        if let Ok(mut slot) = self.message.lock()
            && slot.is_none()
        {
            *slot = Some(msg);
        }
        self.failed.store(true, Ordering::Release);
    }

    fn get(&self) -> Option<Error> {
        if !self.failed.load(Ordering::Acquire) {
            return None;
        }
        let msg = self
            .message
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .unwrap_or_else(|| "audio capture failed".into());
        Some(Error::Unavailable(msg))
    }
}

/// Fixed-size inline chunk sent from the real-time callback (`len > 0` for
/// audio; `len == 0` is a best-effort wakeup sentinel when a sticky error fires).
struct AudioChunk {
    samples: [f32; CALLBACK_CHUNK_SAMPLES],
    len: usize,
    at: Instant,
}

impl AudioChunk {
    fn wake() -> Self {
        Self {
            samples: [0.0; CALLBACK_CHUNK_SAMPLES],
            len: 0,
            at: Instant::now(),
        }
    }
}

/// Callback-side sender: resamples into a pre-allocated reusable scratch buffer
/// and sends fixed-size [`AudioChunk`]s without per-callback heap allocations.
pub(super) struct AudioSender {
    tx: SyncSender<AudioChunk>,
    scratch: Vec<f32>,
    err: Arc<SharedError>,
}

/// Error-callback handle sharing the sticky error slot with [`AudioReceiver`].
pub(super) struct ErrorHandle {
    tx: SyncSender<AudioChunk>,
    err: Arc<SharedError>,
}

/// Reader-side receiver checking the sticky error slot before and after waiting.
pub(super) struct AudioReceiver {
    rx: Receiver<AudioChunk>,
    err: Arc<SharedError>,
}

pub(super) fn audio_channel(cap: usize) -> (AudioSender, AudioReceiver) {
    let (tx, rx) = mpsc::sync_channel(cap);
    let err = Arc::new(SharedError::new());
    (
        AudioSender {
            tx,
            // Pre-allocate 40 ms so typical 10-20 ms callbacks never reallocate.
            scratch: Vec::with_capacity(CALLBACK_CHUNK_SAMPLES * 2),
            err: Arc::clone(&err),
        },
        AudioReceiver { rx, err },
    )
}

impl AudioSender {
    pub(super) fn error_handle(&self) -> ErrorHandle {
        ErrorHandle {
            tx: self.tx.clone(),
            err: Arc::clone(&self.err),
        }
    }

    pub(super) fn push_resampled(
        &mut self,
        resampler: &mut Resampler,
        samples: impl IntoIterator<Item = f32>,
        now: Instant,
    ) {
        self.scratch.clear();
        resampler.push(samples, &mut self.scratch);
        if self.scratch.is_empty() {
            return;
        }
        let total_frames = self.scratch.len() / CHANNELS as usize;
        let start_at = now
            .checked_sub(frames_duration(total_frames))
            .unwrap_or(now);
        let mut frame_offset = 0usize;
        for part in self.scratch.chunks(CALLBACK_CHUNK_SAMPLES) {
            let mut chunk = [0.0f32; CALLBACK_CHUNK_SAMPLES];
            chunk[..part.len()].copy_from_slice(part);
            let at = start_at + frames_duration(frame_offset);
            let _ = self.tx.try_send(AudioChunk {
                samples: chunk,
                len: part.len(),
                at,
            });
            frame_offset += part.len() / CHANNELS as usize;
        }
    }

    #[cfg(test)]
    fn scratch_ptr_and_cap(&self) -> (*const f32, usize) {
        (self.scratch.as_ptr(), self.scratch.capacity())
    }
}

impl ErrorHandle {
    pub(super) fn set_error(&self, msg: String) {
        self.err.set(msg);
        // Best-effort wake if the reader is blocked in recv_timeout; if the
        // channel is full, the sticky flag still guarantees delivery on read.
        let _ = self.tx.try_send(AudioChunk::wake());
    }
}

impl AudioReceiver {
    pub(super) fn read(&self, wait: Duration) -> Result<Option<Packet>, Error> {
        if let Some(err) = self.err.get() {
            return Err(err);
        }
        match self.rx.recv_timeout(wait) {
            Ok(chunk) if chunk.len > 0 => Ok(Some(Packet {
                samples: chunk.samples[..chunk.len].to_vec(),
                at: chunk.at,
            })),
            Ok(_) => Err(self
                .err
                .get()
                .unwrap_or_else(|| Error::Unavailable("audio capture failed".into()))),
            Err(RecvTimeoutError::Timeout) => match self.err.get() {
                Some(err) => Err(err),
                None => Ok(None),
            },
            Err(RecvTimeoutError::Disconnected) => Err(self
                .err
                .get()
                .unwrap_or_else(|| Error::Unavailable("audio stream closed".into()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticky_error_reports_unavailable_even_when_channel_is_full() {
        let (mut tx, rx) = audio_channel(2);
        let err_tx = tx.error_handle();
        let mut resampler = Resampler::new(RATE, CHANNELS, RATE);
        let now = Instant::now();
        let block = vec![0.25f32; 480 * CHANNELS as usize];

        // Fill the bounded channel completely (capacity 2) plus one overflow.
        for i in 0..3 {
            tx.push_resampled(
                &mut resampler,
                block.iter().copied(),
                now + Duration::from_millis(i * 10),
            );
        }

        // Device-lost fires while the channel is full; try_send drops the message,
        // so only the sticky error slot keeps the failure alive.
        err_tx.set_error("device disconnected".into());

        let first = rx.read(Duration::from_millis(5));
        assert_eq!(first, Err(Error::Unavailable("device disconnected".into())));
        // Must remain sticky on subsequent reads (never fall back to Ok(None)).
        let second = rx.read(Duration::from_millis(5));
        assert_eq!(
            second,
            Err(Error::Unavailable("device disconnected".into()))
        );
    }

    #[test]
    fn callback_reuses_scratch_buffer_and_splits_oversized_packets() {
        let (mut tx, rx) = audio_channel(8);
        let mut resampler = Resampler::new(RATE, CHANNELS, RATE);
        let before = tx.scratch_ptr_and_cap();
        let t0 = Instant::now();

        // Repeated 10 ms callbacks reuse the exact same scratch allocation.
        let ten_ms = vec![0.5f32; 480 * CHANNELS as usize];
        for i in 0..20 {
            tx.push_resampled(
                &mut resampler,
                ten_ms.iter().copied(),
                t0 + Duration::from_millis((i + 1) * 10),
            );
            let pkt = rx
                .read(Duration::from_millis(5))
                .expect("read ok")
                .expect("packet");
            assert_eq!(pkt.samples.len(), ten_ms.len());
            assert_eq!(pkt.samples[0], 0.5);
        }
        assert_eq!(
            tx.scratch_ptr_and_cap(),
            before,
            "scratch buffer must be reused without reallocating"
        );

        // An oversized 30 ms callback (1,440 stereo frames) splits cleanly across
        // fixed-size 20 ms chunks with advancing timestamps.
        let thirty_ms: Vec<f32> = (0..(1_440 * CHANNELS as usize))
            .map(|i| {
                if i < CALLBACK_CHUNK_SAMPLES {
                    0.25
                } else {
                    -0.5
                }
            })
            .collect();
        tx.push_resampled(&mut resampler, thirty_ms, t0 + Duration::from_millis(300));

        let p1 = rx.read(Duration::from_millis(5)).unwrap().unwrap();
        let p2 = rx.read(Duration::from_millis(5)).unwrap().unwrap();
        assert_eq!(p1.samples.len(), CALLBACK_CHUNK_SAMPLES);
        assert_eq!(p2.samples.len(), 480 * CHANNELS as usize);
        assert!(p1.samples.iter().all(|&s| s == 0.25));
        assert!(p2.samples.iter().all(|&s| s == -0.5));
        assert_eq!(p2.at.duration_since(p1.at), Duration::from_millis(20));
        assert_eq!(
            tx.scratch_ptr_and_cap(),
            before,
            "30 ms fits in pre-allocated 40 ms scratch without reallocating"
        );
    }
}
