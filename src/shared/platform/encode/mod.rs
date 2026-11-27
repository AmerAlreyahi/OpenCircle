//! Writing a recording to a file (R-04): one interface, the OS encoder behind it.
//! - Windows (`mf`): Media Foundation's Sink Writer, H.264 on the GPU's encoder (Microsoft's
//!   software one without), AAC, fragmented MP4; audio only is `.m4a`.
//! - Linux (`gst`): GStreamer through the dlopen loader (`platform::gst`): x264 + MP4 where
//!   installed, else VP8 + WebM (explicit bitrate), Opus; audio only is `.ogg`.
//!
//! The session (`snip::record::session`) owns a `Box<dyn Sink>` on its own thread; frames are
//! RGBA, audio is interleaved stereo 48 kHz `f32`.
use std::path::Path;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod gst;
#[cfg(windows)]
mod mf;

#[cfg(target_os = "linux")]
use gst as imp;
#[cfg(windows)]
use mf as imp;

/// The picture: every frame pushed is `from.0 * from.1 * 4` RGBA bytes, `fps` of them a second;
/// it is written `width` x `height` (scaled, never cut, when `from` is over the encoders' limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFmt {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub from: (u32, u32),
}

impl VideoFmt {
    /// A picture of frames `w` x `h` (even sides: the caller rounds), written at `fit` of it.
    pub fn of(w: u32, h: u32, fps: u32) -> VideoFmt {
        let (width, height) = fit(w, h);
        VideoFmt {
            width,
            height,
            fps,
            from: (w, h),
        }
    }

    /// Bytes of one frame as pushed.
    pub fn frame_bytes(&self) -> usize {
        self.from.0 as usize * self.from.1 as usize * 4
    }
}

/// What a recording is written as, chosen by `plan` (the file's extension says which).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub ext: &'static str,
    /// H.264 (else VP8 / audio only).
    pub h264: bool,
}

/// Roughly 0.08 bit per pixel per frame (1080p30 about 5 Mbps), in bits per second.
pub fn bitrate(v: VideoFmt) -> u32 {
    let bps = u64::from(v.width) * u64::from(v.height) * u64::from(v.fps) * 8 / 100;
    bps.clamp(500_000, 40_000_000) as u32
}

/// The longest side written (hardware H.264 tops out near 4096).
pub const MAX_SIDE: u32 = 3840;

/// Even sides (the encoders' 4:2:0 needs them), at most `MAX_SIDE` on the long edge, scaled with
/// the same ratio: the size a recording of `w` x `h` is written at.
pub fn fit(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    let (w, h) = match long > MAX_SIDE {
        true => (
            (u64::from(w) * u64::from(MAX_SIDE) / u64::from(long)) as u32,
            (u64::from(h) * u64::from(MAX_SIDE) / u64::from(long)) as u32,
        ),
        false => (w, h),
    };
    (w.max(2) & !1, h.max(2) & !1)
}

/// Per-thread setup the encoder's OS needs on the thread that writes (Windows: COM).
pub fn thread_init() {
    imp::thread_init();
}

/// How a recording would be written on this PC (`video`: a screen recording, else audio only),
/// or why it cannot: the words are shown to the user.
pub fn plan(video: bool) -> Result<Plan, String> {
    imp::plan(video)
}

/// A file being written. `video` is none for audio only, `audio` none for a silent recording.
pub struct FileSink(imp::Imp);

impl FileSink {
    pub fn open(
        path: &Path,
        video: Option<VideoFmt>,
        audio: bool,
    ) -> Result<Box<dyn Sink>, String> {
        if video.is_none() && !audio {
            return Err("nothing to record".into());
        }
        if let Some(v) = video
            && (v.width < 2 || v.height < 2 || v.width % 2 != 0 || v.height % 2 != 0 || v.fps == 0)
        {
            return Err(format!(
                "a {}x{} picture cannot be encoded",
                v.width, v.height
            ));
        }
        Ok(Box::new(FileSink(imp::Imp::open(path, video, audio)?)))
    }
}

/// Where frames and sound go. `pts` is the time into the recording, pauses not counted.
pub trait Sink: Send {
    fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String>;
    fn audio(&mut self, pcm: &[f32], pts: Duration) -> Result<(), String>;
    /// The recording is paused (`true`) or goes on (nothing is pushed in between).
    fn pause(&mut self, _paused: bool) {}
    /// Writes the rest and closes the file.
    fn finish(self: Box<Self>) -> Result<(), String>;
}

impl Sink for FileSink {
    fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
        self.0.video(rgba, pts)
    }
    fn audio(&mut self, pcm: &[f32], pts: Duration) -> Result<(), String> {
        self.0.audio(pcm, pts)
    }
    fn pause(&mut self, paused: bool) {
        self.0.pause(paused);
    }
    fn finish(self: Box<Self>) -> Result<(), String> {
        self.0.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bitrate_follows_the_pixels_and_stays_in_range() {
        let hd = |fps| VideoFmt::of(1920, 1080, fps);
        assert_eq!(bitrate(hd(30)), 4_976_640);
        assert!(bitrate(hd(60)) > bitrate(hd(30)));
        assert_eq!(bitrate(VideoFmt::of(64, 64, 30)), 500_000);
    }

    /// R04-70: a size is made even; one over the limit is SCALED with its ratio (never cut).
    #[test]
    fn a_size_is_made_even_and_scaled_not_cut() {
        assert_eq!(fit(1921, 1081), (1920, 1080));
        assert_eq!(fit(7680, 4320), (3840, 2160), "same ratio");
        assert_eq!(fit(3440, 1440), (3440, 1440), "under the limit: as it is");
        assert_eq!(fit(5120, 1440), (3840, 1080));
        let v = VideoFmt::of(7680, 4320, 30);
        assert_eq!((v.width, v.height, v.from), (3840, 2160, (7680, 4320)));
        assert_eq!(v.frame_bytes(), 7680 * 4320 * 4);
    }

    #[test]
    fn odd_pictures_and_empty_recordings_are_refused_before_any_encoder_starts() {
        let odd = VideoFmt {
            width: 101,
            height: 100,
            fps: 30,
            from: (101, 100),
        };
        assert!(FileSink::open(Path::new("/x/a.mp4"), Some(odd), true).is_err());
        assert!(FileSink::open(Path::new("/x/a.mp4"), None, false).is_err());
    }
}
