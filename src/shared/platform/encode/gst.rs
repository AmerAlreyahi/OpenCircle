//! The Linux encoder: GStreamer `appsrc` -> convert -> encoder -> muxer -> `filesink`.
//! The sources are live and stamp each buffer as it arrives, and a paused pipeline's clock
//! stands still, so a pause leaves no gap. L3 / L4 (docs/internal/reports-060): `x264enc` needs
//! no `h264parse` for `mp4mux`; `mp4mux` fragments only with `first-moov-then-finalise` on
//! 1.20 (a crash leaves a playable file); `vp8enc` wants an explicit bitrate and threads.
use std::path::Path;
use std::time::Duration;

use super::{Plan, VideoFmt, bitrate};
use crate::shared::platform::gst::{Feed, Gst};

/// How long closing the file may take.
const CLOSE: Duration = Duration::from_secs(20);

pub(super) struct Imp {
    feed: Feed,
    video: bool,
    audio: bool,
}

/// What the installed plugins can write, or which are missing (named: the words are shown).
fn plan_with(has: impl Fn(&str) -> bool, video: bool) -> Result<Plan, String> {
    let all = |names: &[&str]| names.iter().all(|n| has(n));
    match video {
        false if all(&["opusenc", "oggmux"]) => {
            return Ok(Plan {
                ext: "ogg",
                h264: false,
            });
        }
        true if all(&["x264enc", "mp4mux", "opusenc"]) => {
            return Ok(Plan {
                ext: "mp4",
                h264: true,
            });
        }
        true if all(&["vp8enc", "webmmux", "opusenc"]) => {
            return Ok(Plan {
                ext: "webm",
                h264: false,
            });
        }
        _ => {}
    }
    // which ones: the picture's encoder and container (the one nearest to working), the sound's
    let wanted: &[&str] = match (video, has("x264enc") || !has("vp8enc")) {
        (false, _) => &["opusenc", "oggmux"],
        (true, true) => &["x264enc", "mp4mux", "opusenc"],
        (true, false) => &["vp8enc", "webmmux", "opusenc"],
    };
    let missing: Vec<&str> = wanted.iter().copied().filter(|n| !has(n)).collect();
    Err(format!(
        "Recording needs GStreamer plugins: {}",
        missing.join(", ")
    ))
}

pub(super) fn thread_init() {}

pub(super) fn plan(video: bool) -> Result<Plan, String> {
    let gst = Gst::get().ok_or("Recording needs GStreamer plugins: libgstreamer")?;
    plan_with(|e| gst.has(e), video)
}

/// The pipeline for `path` (its extension picks the muxer).
fn pipeline(path: &Path, video: Option<VideoFmt>, audio: bool, threads: u32) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let file = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mux = match ext {
        "mp4" => "mp4mux name=mux fragment-duration=1000 fragment-mode=first-moov-then-finalise",
        "webm" => "webmmux name=mux",
        _ => "oggmux name=mux",
    };
    let mut line = format!("{mux} ! filesink location=\"{file}\"");
    if let Some(v) = video {
        let kbps = bitrate(v) / 1000;
        // over the encoders' limit: scaled (not cut) to what is written
        let scale = match v.from == (v.width, v.height) {
            true => String::new(),
            false => format!(
                "videoscale ! video/x-raw,width={},height={} ! ",
                v.width, v.height
            ),
        };
        let enc = match ext {
            "mp4" => format!(
                "x264enc speed-preset=ultrafast tune=zerolatency bitrate={kbps} key-int-max={}",
                v.fps * 2
            ),
            _ => format!(
                "vp8enc deadline=1 cpu-used=8 threads={threads} target-bitrate={} keyframe-max-dist={}",
                bitrate(v),
                v.fps * 2
            ),
        };
        line += &format!(
            " appsrc name=vsrc is-live=false do-timestamp=false format=time block=true \
             caps=video/x-raw,format=RGBA,width={},height={},framerate={}/1 ! queue ! videoconvert ! \
             {scale}video/x-raw,format=I420 ! queue ! {enc} ! mux.",
            v.from.0, v.from.1, v.fps
        );
    }
    if audio {
        line += " appsrc name=asrc is-live=false do-timestamp=false format=time block=true \
             caps=audio/x-raw,format=F32LE,layout=interleaved,rate=48000,channels=2 ! queue ! \
             audioconvert ! audioresample ! opusenc bitrate=128000 ! mux.";
    }
    line
}

impl Imp {
    pub(super) fn open(path: &Path, video: Option<VideoFmt>, audio: bool) -> Result<Imp, String> {
        let gst = Gst::get().ok_or("Recording needs GStreamer (your system's video plugins)")?;
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4)) as u32;
        let line = pipeline(path, video, audio, threads);
        let names: &[&'static str] = match (video.is_some(), audio) {
            (true, true) => &["vsrc", "asrc"],
            (true, false) => &["vsrc"],
            _ => &["asrc"],
        };
        let feed = gst.feed(&line, names)?;
        Ok(Imp {
            feed,
            video: video.is_some(),
            audio,
        })
    }

    pub(super) fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
        match self.video {
            true => self.feed.push("vsrc", rgba, pts),
            false => Ok(()),
        }
    }

    pub(super) fn audio(&mut self, pcm: &[f32], pts: Duration) -> Result<(), String> {
        if !self.audio {
            return Ok(());
        }
        // SAFETY: f32 has no padding; the same bytes, read as bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(pcm.as_ptr() as *const u8, std::mem::size_of_val(pcm))
        };
        self.feed.push("asrc", bytes, pts)
    }

    /// Timestamps come from the session (pauses already left out): nothing to stop here.
    pub(super) fn pause(&mut self, _paused: bool) {}

    pub(super) fn finish(self) -> Result<(), String> {
        self.feed.finish(CLOSE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::platform::encode::FileSink;

    fn hd() -> VideoFmt {
        VideoFmt::of(1280, 720, 30)
    }

    #[test]
    fn x264_and_mp4_when_installed_else_vp8_and_webm_else_a_plain_message() {
        let all = |_: &str| true;
        assert_eq!(
            plan_with(all, true),
            Ok(Plan {
                ext: "mp4",
                h264: true
            })
        );
        let no_x264 = |e: &str| e != "x264enc";
        assert_eq!(
            plan_with(no_x264, true),
            Ok(Plan {
                ext: "webm",
                h264: false
            })
        );
        assert_eq!(
            plan_with(no_x264, false),
            Ok(Plan {
                ext: "ogg",
                h264: false
            })
        );
        let nothing = |_: &str| false;
        // R04-71: the missing plugins are NAMED, in plain words
        assert_eq!(
            plan_with(nothing, true).unwrap_err(),
            "Recording needs GStreamer plugins: x264enc, mp4mux, opusenc"
        );
        let only_vp8_no_mux = |e: &str| matches!(e, "vp8enc" | "opusenc");
        assert_eq!(
            plan_with(only_vp8_no_mux, true).unwrap_err(),
            "Recording needs GStreamer plugins: webmmux"
        );
        assert!(plan_with(nothing, false).is_err());
        let no_opus = |e: &str| e != "opusenc";
        assert!(
            plan_with(no_opus, true).is_err(),
            "no sound codec, no recording"
        );
    }

    #[test]
    fn the_mp4_pipeline_is_fragmented_and_needs_no_h264parse() {
        let p = pipeline(Path::new("/t/rec.mp4"), Some(hd()), true, 4);
        assert!(p.contains("fragment-mode=first-moov-then-finalise"), "{p}");
        assert!(p.contains("x264enc") && !p.contains("h264parse"), "{p}");
        assert!(
            p.contains("opusenc") && p.contains("location=\"/t/rec.mp4\""),
            "{p}"
        );
        assert!(p.contains("width=1280,height=720,framerate=30/1"), "{p}");
    }

    /// R04-72: a picture over the limit is scaled in the pipeline (videoscale), not cut.
    #[test]
    fn a_big_picture_is_scaled_in_the_pipeline() {
        let big = VideoFmt::of(7680, 4320, 30);
        let p = pipeline(Path::new("/t/r.mp4"), Some(big), false, 4);
        assert!(p.contains("width=7680,height=4320"), "{p}");
        assert!(
            p.contains("videoscale ! video/x-raw,width=3840,height=2160"),
            "{p}"
        );
        let plain = pipeline(Path::new("/t/r.mp4"), Some(hd()), false, 4);
        assert!(!plain.contains("videoscale"), "{plain}");
        assert!(
            plain.contains("do-timestamp=false"),
            "the session's own times"
        );
    }

    #[test]
    fn the_webm_pipeline_has_an_explicit_bitrate_and_threads() {
        let p = pipeline(Path::new("/t/rec.webm"), Some(hd()), true, 3);
        assert!(p.contains("webmmux") && p.contains("threads=3"), "{p}");
        assert!(
            p.contains(&format!("target-bitrate={}", bitrate(hd()))),
            "{p}"
        );
    }

    #[test]
    fn audio_only_is_opus_in_ogg_and_a_quote_in_the_path_cannot_break_out() {
        let p = pipeline(Path::new("/t/a\"b.ogg"), None, true, 1);
        assert!(p.starts_with("oggmux") && !p.contains("vsrc"), "{p}");
        assert!(p.contains("location=\"/t/a\\\"b.ogg\""), "{p}");
    }

    /// 3 s of a moving pattern and a tone, in real time, then the file is read back with
    /// GStreamer's own discoverer (`--ignored`: needs GStreamer's plugins; run on the VMs).
    #[test]
    #[ignore = "needs GStreamer's encoders; run with --ignored"]
    fn a_real_recording_of_three_seconds_has_its_frames_and_sound() {
        // every container this PC has the plugins for: MP4 (x264), WebM (VP8), Ogg (audio only)
        let gst = Gst::get().expect("GStreamer");
        assert!(
            plan(true).is_ok() && plan(false).is_ok(),
            "this PC can record"
        );
        let cases = [
            (true, "mp4", gst.has("x264enc") && gst.has("mp4mux")),
            (true, "webm", gst.has("vp8enc") && gst.has("webmmux")),
            (false, "ogg", gst.has("oggmux")),
        ];
        for (video, ext, there) in cases {
            eprintln!("{ext}: elements present: {there}");
            if !there {
                continue;
            }
            let dir = std::env::temp_dir().join(format!("oc-enc-{}-{ext}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("rec.{ext}"));
            let fmt = VideoFmt::of(640, 360, 30);
            let mut sink = FileSink::open(&path, video.then_some(fmt), true).unwrap();
            let start = std::time::Instant::now();
            let mut tone = 0u64;
            for n in 0..90u32 {
                let pts = Duration::from_millis(u64::from(n) * 1000 / 30);
                if video {
                    let mut frame = vec![255u8; 640 * 360 * 4];
                    for (i, px) in frame.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        px[0] = ((i % 640) as u32 * 255 / 640) as u8;
                        px[1] = (n * 2) as u8;
                    }
                    sink.video(&frame, pts).unwrap();
                }
                // 1600 stereo frames = 33 ms of a 440 Hz tone
                let mut pcm = Vec::with_capacity(3200);
                for _ in 0..1600 {
                    let s = (tone as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.3;
                    pcm.extend([s, s]);
                    tone += 1;
                }
                sink.audio(&pcm, pts).unwrap();
                let due = Duration::from_millis(u64::from(n + 1) * 1000 / 30);
                std::thread::sleep(due.saturating_sub(start.elapsed()));
            }
            sink.finish().unwrap();
            let out = std::process::Command::new("gst-discoverer-1.0")
                .arg(&path)
                .output();
            let text =
                String::from_utf8_lossy(&out.expect("gst-discoverer-1.0").stdout).into_owned();
            eprintln!("{text}");
            assert!(text.contains("audio"), "{text}");
            if video {
                // 4:2:0 (players refuse 4:4:4 H.264) and one frame for every 1/30 s
                assert!(!text.contains("4:4:4"), "{text}");
                let probe = std::process::Command::new("ffprobe")
                    .args(["-v", "error", "-count_packets", "-select_streams", "v:0"])
                    .args(["-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
                    .arg(&path)
                    .output()
                    .expect("ffprobe");
                let frames: u32 = String::from_utf8_lossy(&probe.stdout)
                    .trim()
                    .parse()
                    .unwrap();
                assert!(
                    (80..=95).contains(&frames),
                    "{frames} frames in 3 s at 30 fps"
                );
            }
            assert!(
                text.contains("Duration: 0:00:03") || text.contains("Duration: 0:00:02.9"),
                "{text}"
            );
            assert_eq!(text.contains("video"), video, "{text}");
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
