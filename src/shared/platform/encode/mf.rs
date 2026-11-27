//! The Windows encoder: Media Foundation's Sink Writer. H.264 on the GPU's encoder where the
//! writer finds one (`MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS`; Microsoft's software encoder
//! otherwise), AAC 128 kbps, fragmented MP4 (a crash leaves a playable file); audio only is a
//! plain MPEG-4 `.m4a`. The writer adds the colour converter itself (RGB32 in).
use std::path::Path;
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{GUID, HSTRING, Result as Hr};

use super::{Plan, VideoFmt, bitrate};

/// Media Foundation's time unit: 100 ns.
const HNS: i64 = 10_000_000;

pub(super) struct Imp {
    /// Released BEFORE Media Foundation is shut down (`Drop`).
    writer: Option<IMFSinkWriter>,
    video: Option<(u32, VideoFmt)>,
    audio: Option<u32>,
    /// `finish` ran: the writer is closed (else `Drop` still shuts Media Foundation down).
    finished: bool,
}

impl Drop for Imp {
    fn drop(&mut self) {
        if !self.finished {
            crate::log!("[opencircle] recording: the writer was dropped before it was finished");
        }
        // the writer first (its COM object needs Media Foundation to release), then the shutdown
        drop(self.writer.take());
        // SAFETY: pairs the MFStartup of `open`, once.
        let _ = unsafe { MFShutdown() };
    }
}

// SAFETY: the Sink Writer is free-threaded; the session uses it from one thread at a time.
unsafe impl Send for Imp {}

/// COM for the thread that writes (the Sink Writer is free-threaded: the multithreaded apartment).
pub(super) fn thread_init() {
    // SAFETY: plain COM init; "already initialised in another mode" is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

pub(super) fn plan(video: bool) -> Result<Plan, String> {
    Ok(match video {
        true => Plan {
            ext: "mp4",
            h264: true,
        },
        false => Plan {
            ext: "m4a",
            h264: false,
        },
    })
}

fn media(video: bool) -> Hr<IMFMediaType> {
    let t = unsafe { MFCreateMediaType()? };
    let major = if video {
        MFMediaType_Video
    } else {
        MFMediaType_Audio
    };
    unsafe { t.SetGUID(&MF_MT_MAJOR_TYPE, &major)? };
    Ok(t)
}

fn pack(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

fn guid(t: &IMFMediaType, key: &GUID, value: &GUID) -> Hr<()> {
    unsafe { t.SetGUID(key, value) }
}

fn u32_(t: &IMFMediaType, key: &GUID, value: u32) -> Hr<()> {
    unsafe { t.SetUINT32(key, value) }
}

fn open(path: &Path, video: Option<VideoFmt>, audio: bool) -> Hr<Imp> {
    // SAFETY: plain Media Foundation use; every object is released by its wrapper's drop.
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 3)?;
        let attrs = attrs.ok_or_else(windows::core::Error::empty)?;
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        let container = match video.is_some() {
            true => MFTranscodeContainerType_FMPEG4,
            false => MFTranscodeContainerType_MPEG4,
        };
        attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &container)?;
        let url = HSTRING::from(path.as_os_str());
        let writer = MFCreateSinkWriterFromURL(&url, None, &attrs)?;

        let video = match video {
            Some(v) => {
                let out = media(true)?;
                guid(&out, &MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
                u32_(&out, &MF_MT_AVG_BITRATE, bitrate(v))?;
                u32_(
                    &out,
                    &MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )?;
                out.SetUINT64(&MF_MT_FRAME_SIZE, pack(v.width, v.height))?;
                out.SetUINT64(&MF_MT_FRAME_RATE, pack(v.fps, 1))?;
                out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                let index = writer.AddStream(&out)?;
                let input = media(true)?;
                guid(&input, &MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
                u32_(
                    &input,
                    &MF_MT_INTERLACE_MODE,
                    MFVideoInterlace_Progressive.0 as u32,
                )?;
                // the frames as they are pushed: the writer's video processor scales them to the output size
                input.SetUINT64(&MF_MT_FRAME_SIZE, pack(v.from.0, v.from.1))?;
                input.SetUINT64(&MF_MT_FRAME_RATE, pack(v.fps, 1))?;
                input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                // a positive stride: the rows run top to bottom
                u32_(&input, &MF_MT_DEFAULT_STRIDE, v.from.0 * 4)?;
                writer.SetInputMediaType(index, &input, None)?;
                Some((index, v))
            }
            None => None,
        };
        let audio = match audio {
            true => {
                let out = media(false)?;
                guid(&out, &MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
                u32_(&out, &MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                u32_(&out, &MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
                u32_(&out, &MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                u32_(&out, &MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 16_000)?;
                u32_(&out, &MF_MT_AUDIO_BLOCK_ALIGNMENT, 1)?;
                let index = writer.AddStream(&out)?;
                let input = media(false)?;
                guid(&input, &MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                u32_(&input, &MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                u32_(&input, &MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
                u32_(&input, &MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                u32_(&input, &MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
                u32_(&input, &MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 192_000)?;
                writer.SetInputMediaType(index, &input, None)?;
                Some(index)
            }
            false => None,
        };
        writer.BeginWriting()?;
        Ok(Imp {
            writer: Some(writer),
            video,
            audio,
            finished: false,
        })
    }
}

/// A sample of `bytes` at `at` lasting `len` (both in 100 ns).
fn sample(bytes: &[u8], at: i64, len: i64) -> Hr<IMFSample> {
    // SAFETY: the buffer's memory is locked while it is written, `bytes.len()` fits it.
    unsafe {
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        let s = MFCreateSample()?;
        s.AddBuffer(&buffer)?;
        s.SetSampleTime(at)?;
        s.SetSampleDuration(len)?;
        Ok(s)
    }
}

fn why(e: windows::core::Error) -> String {
    format!("Media Foundation: {e}")
}

impl Imp {
    pub(super) fn open(path: &Path, video: Option<VideoFmt>, audio: bool) -> Result<Imp, String> {
        open(path, video, audio).map_err(|e| {
            // started in `open`: every way out closes Media Foundation again
            // SAFETY: pairs the MFStartup of `open` (a failed one is harmless to shut down).
            let _ = unsafe { MFShutdown() };
            why(e)
        })
    }

    pub(super) fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
        let Some((index, v)) = self.video else {
            return Ok(());
        };
        let at = (pts.as_nanos() / 100) as i64;
        let len = HNS / i64::from(v.fps);
        // RGB32 is B, G, R, x in memory: the capture's R, G, B, A with red and blue swapped
        let mut bgra = rgba.to_vec();
        for px in bgra.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let s = sample(&bgra, at, len).map_err(why)?;
        // SAFETY: a live writer and sample.
        unsafe {
            self.writer
                .as_ref()
                .ok_or("the writer is closed")?
                .WriteSample(index, &s)
        }
        .map_err(why)
    }

    pub(super) fn audio(&mut self, pcm: &[f32], pts: Duration) -> Result<(), String> {
        let Some(index) = self.audio else {
            return Ok(());
        };
        let bytes: Vec<u8> = pcm
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        let at = (pts.as_nanos() / 100) as i64;
        let len = (pcm.len() as i64 / 2) * HNS / 48_000;
        let s = sample(&bytes, at, len).map_err(why)?;
        // SAFETY: a live writer and sample.
        unsafe {
            self.writer
                .as_ref()
                .ok_or("the writer is closed")?
                .WriteSample(index, &s)
        }
        .map_err(why)
    }

    /// Timestamps come from the session (pauses already left out): nothing to stop here.
    pub(super) fn pause(&mut self, _paused: bool) {}

    pub(super) fn finish(mut self) -> Result<(), String> {
        // SAFETY: a live writer; the shutdown is `Drop`'s (once, whatever way this ends).
        let done = match self.writer.as_ref() {
            Some(w) => unsafe { w.Finalize() }.map_err(why),
            None => Err("the writer is closed".to_string()),
        };
        self.finished = true;
        done
    }
}
