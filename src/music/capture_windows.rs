//! Windows: WASAPI through cpal. "What's playing" = an input stream on the
//! default output device, which cpal opens as loopback; the microphone = the
//! default input device.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{Clip, Downmix, Error};
use crate::config::SongSource;

pub fn record(
    source: SongSource,
    want: usize,
    deadline: Instant,
    cancel: &AtomicBool,
    heard: &mut dyn FnMut(u32),
) -> Result<Clip, Error> {
    let host = cpal::default_host();
    let fail = |e: &dyn std::fmt::Display| Error::Capture(e.to_string());
    let (device, config) = match source {
        SongSource::System => {
            let d = host
                .default_output_device()
                .ok_or_else(|| fail(&"no speakers or headphones"))?;
            let c = d.default_output_config().map_err(|e| fail(&e))?;
            (d, c)
        }
        SongSource::Microphone => {
            let d = host
                .default_input_device()
                .ok_or_else(|| fail(&"no microphone"))?;
            let c = d.default_input_config().map_err(|e| fail(&e))?;
            (d, c)
        }
    };
    let mix = Arc::new(Mutex::new(Downmix::new(
        config.sample_rate(),
        config.channels(),
        want,
    )));
    // A dropped buffer now and then (xrun) doesn't hurt recognition: not logged.
    let error = |e: cpal::Error| {
        if e.kind() != cpal::ErrorKind::Xrun {
            crate::log!("[opencircle] song search audio: {e}");
        }
    };
    let m = mix.clone();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.config(),
            move |d: &[f32], _: &cpal::InputCallbackInfo| m.lock().unwrap().push(d.iter().copied()),
            error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.config(),
            move |d: &[i16], _: &cpal::InputCallbackInfo| {
                m.lock()
                    .unwrap()
                    .push(d.iter().map(|&s| f32::from(s) / 32768.0))
            },
            error,
            None,
        ),
        other => return Err(fail(&format!("unsupported sample format {other}"))),
    }
    .map_err(|e| fail(&e))?;
    stream.play().map_err(|e| fail(&e))?;
    // Loopback gets no packets while nothing plays: the deadline ends it then.
    let started = Instant::now();
    let mut said = 0;
    while Instant::now() < deadline
        && !cancel.load(Ordering::Relaxed)
        && !mix.lock().unwrap().full()
    {
        std::thread::sleep(Duration::from_millis(100));
        let secs = started.elapsed().as_secs() as u32;
        if secs > said {
            said = secs;
            heard(secs);
        }
    }
    drop(stream);
    let samples = std::mem::take(&mut mix.lock().unwrap().out);
    Ok(Clip(samples))
}
