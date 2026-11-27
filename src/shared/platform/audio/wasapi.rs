//! Windows: WASAPI through cpal. System sound = an input stream on a render
//! (output) device, which cpal opens as loopback; microphone = an input device.
//! Any device mix format (44.1 / 48 / 96 kHz, mono / stereo / 5.1) is converted
//! to 48 kHz stereo `f32` PCM via [`super::resample::Resampler`].

use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::resample::Resampler;
use super::{CHANNELS, Error, Kind, Packet, RATE, frames_duration};

/// Bounded channel between the real-time audio callback and the reader thread.
/// At ~10 ms per WASAPI callback, 64 packets ≈ 640 ms of cushion without unbounded RAM.
const CHANNEL_CAP: usize = 64;

pub(super) struct WasapiInput {
    _stream: cpal::Stream,
    rx: Receiver<Result<Packet, Error>>,
}

impl WasapiInput {
    pub(super) fn open(kind: Kind) -> Result<Self, Error> {
        let host = cpal::default_host();
        let fail = |e: &dyn std::fmt::Display| Error::Unavailable(e.to_string());
        let (device, config) = match kind {
            Kind::System => {
                let d = host
                    .default_output_device()
                    .ok_or_else(|| fail(&"no speakers or headphones"))?;
                let c = d.default_output_config().map_err(|e| fail(&e))?;
                (d, c)
            }
            Kind::Mic(None) => {
                let d = host
                    .default_input_device()
                    .ok_or_else(|| fail(&"no microphone"))?;
                let c = d.default_input_config().map_err(|e| fail(&e))?;
                (d, c)
            }
            Kind::Mic(Some(id)) if id.is_empty() => {
                let d = host
                    .default_input_device()
                    .ok_or_else(|| fail(&"no microphone"))?;
                let c = d.default_input_config().map_err(|e| fail(&e))?;
                (d, c)
            }
            Kind::Mic(Some(id)) => {
                let d = find_input_device(&host, &id).ok_or(Error::NotFound(id))?;
                let c = d.default_input_config().map_err(|e| fail(&e))?;
                (d, c)
            }
        };

        let mut resampler = Resampler::new(config.sample_rate(), config.channels(), RATE);
        let (tx, rx) = mpsc::sync_channel::<Result<Packet, Error>>(CHANNEL_CAP);
        let err_tx = tx.clone();
        // A dropped buffer now and then (xrun) is non-fatal: don't fail the stream.
        let error = move |e: cpal::Error| {
            if e.kind() != cpal::ErrorKind::Xrun {
                crate::log!("[opencircle] audio capture error: {e}");
                let _ = err_tx.try_send(Err(Error::Unavailable(e.to_string())));
            }
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config.config(),
                move |d: &[f32], _: &cpal::InputCallbackInfo| {
                    send_resampled(&mut resampler, d.iter().copied(), &tx);
                },
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                config.config(),
                move |d: &[i16], _: &cpal::InputCallbackInfo| {
                    send_resampled(
                        &mut resampler,
                        d.iter().map(|&s| f32::from(s) / 32768.0),
                        &tx,
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                config.config(),
                move |d: &[u16], _: &cpal::InputCallbackInfo| {
                    send_resampled(
                        &mut resampler,
                        d.iter().map(|&s| (f32::from(s) - 32768.0) / 32768.0),
                        &tx,
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream(
                config.config(),
                move |d: &[i32], _: &cpal::InputCallbackInfo| {
                    send_resampled(
                        &mut resampler,
                        d.iter().map(|&s| (f64::from(s) / 2_147_483_648.0) as f32),
                        &tx,
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::F64 => device.build_input_stream(
                config.config(),
                move |d: &[f64], _: &cpal::InputCallbackInfo| {
                    send_resampled(&mut resampler, d.iter().map(|&s| s as f32), &tx);
                },
                error,
                None,
            ),
            other => {
                return Err(Error::Unsupported(format!(
                    "unsupported sample format {other}"
                )));
            }
        }
        .map_err(|e| fail(&e))?;

        stream.play().map_err(|e| fail(&e))?;
        Ok(Self {
            _stream: stream,
            rx,
        })
    }

    pub(super) fn read(&mut self, wait: Duration) -> Result<Option<Packet>, Error> {
        match self.rx.recv_timeout(wait) {
            Ok(Ok(pkt)) => Ok(Some(pkt)),
            Ok(Err(e)) => Err(e),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                Err(Error::Unavailable("audio stream closed".into()))
            }
        }
    }
}

fn send_resampled(
    resampler: &mut Resampler,
    samples: impl IntoIterator<Item = f32>,
    tx: &SyncSender<Result<Packet, Error>>,
) {
    let mut out = Vec::new();
    resampler.push(samples, &mut out);
    if out.is_empty() {
        return;
    }
    let frames = out.len() / CHANNELS as usize;
    let now = Instant::now();
    let at = now.checked_sub(frames_duration(frames)).unwrap_or(now);
    let _ = tx.try_send(Ok(Packet { samples: out, at }));
}

fn find_input_device(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    if let Ok(parsed) = cpal::DeviceId::from_str(id)
        && let Some(dev) = host.device_by_id(&parsed)
        && dev.supports_input()
    {
        return Some(dev);
    }
    host.input_devices()
        .ok()?
        .find(|d| d.id().ok().is_some_and(|did| did.to_string() == id))
}
