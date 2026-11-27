//! Windows: WASAPI through cpal. System sound = an input stream on a render
//! (output) device, which cpal opens as loopback; microphone = an input device.
//! Any device mix format (44.1 / 48 / 96 kHz, mono / stereo / 5.1) is converted
//! to 48 kHz stereo `f32` PCM via [`super::resample::Resampler`].

use std::str::FromStr;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::channel::{AudioReceiver, CALLBACK_CHANNEL_CAP, audio_channel};
use super::resample::Resampler;
use super::{Error, Kind, Packet, RATE, RawDevice};

#[allow(dead_code)] // called by `audio::devices`
pub(super) fn list_devices(kind: &Kind) -> Result<(Vec<RawDevice>, Option<String>), Error> {
    let host = cpal::default_host();
    let fail = |e: cpal::Error| Error::Unavailable(e.to_string());
    let (default_id, iter): (Option<String>, Vec<cpal::Device>) = match kind {
        Kind::Mic(_) => (
            host.default_input_device()
                .and_then(|d| d.id().ok())
                .map(|id| id.to_string()),
            host.input_devices().map_err(fail)?.collect(),
        ),
        Kind::System => {
            // Only the OS default output can be opened today (`Input::open(Kind::System)`).
            // Upgrade path: change `Kind::System` to `Kind::System(Option<String>)`,
            // enumerate `host.output_devices()` here, and resolve by id in `WasapiInput::open`.
            let dev = host.default_output_device();
            let id = dev
                .as_ref()
                .and_then(|d| d.id().ok())
                .map(|id| id.to_string());
            (id, dev.into_iter().collect())
        }
    };

    let mut raw = Vec::with_capacity(iter.len());
    for dev in iter {
        let Ok(id) = dev.id() else {
            continue;
        };
        let name = dev
            .description()
            .ok()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|| dev.to_string());
        raw.push(RawDevice {
            id: id.to_string(),
            name,
            is_default: false,
        });
    }
    Ok((raw, default_id))
}

pub(super) struct WasapiInput {
    _stream: cpal::Stream,
    rx: AudioReceiver,
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
        let (mut data_tx, rx) = audio_channel(CALLBACK_CHANNEL_CAP);
        let err_tx = data_tx.error_handle();
        // A dropped buffer now and then (xrun) is non-fatal: don't fail the stream.
        let error = move |e: cpal::Error| {
            if e.kind() != cpal::ErrorKind::Xrun {
                crate::log!("[opencircle] audio capture error: {e}");
                err_tx.set_error(e.to_string());
            }
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config.config(),
                move |d: &[f32], _: &cpal::InputCallbackInfo| {
                    data_tx.push_resampled(&mut resampler, d.iter().copied(), Instant::now());
                },
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                config.config(),
                move |d: &[i16], _: &cpal::InputCallbackInfo| {
                    data_tx.push_resampled(
                        &mut resampler,
                        d.iter().map(|&s| f32::from(s) / 32768.0),
                        Instant::now(),
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                config.config(),
                move |d: &[u16], _: &cpal::InputCallbackInfo| {
                    data_tx.push_resampled(
                        &mut resampler,
                        d.iter().map(|&s| (f32::from(s) - 32768.0) / 32768.0),
                        Instant::now(),
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream(
                config.config(),
                move |d: &[i32], _: &cpal::InputCallbackInfo| {
                    data_tx.push_resampled(
                        &mut resampler,
                        d.iter().map(|&s| (f64::from(s) / 2_147_483_648.0) as f32),
                        Instant::now(),
                    );
                },
                error,
                None,
            ),
            cpal::SampleFormat::F64 => device.build_input_stream(
                config.config(),
                move |d: &[f64], _: &cpal::InputCallbackInfo| {
                    data_tx.push_resampled(
                        &mut resampler,
                        d.iter().map(|&s| s as f32),
                        Instant::now(),
                    );
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
        self.rx.read(wait)
    }
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
