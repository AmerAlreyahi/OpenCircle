//! The real recorder (R-04): what the state machine's `Effect`s do for a live recording. It owns
//! the capture session (`session`) and the temp file (`files`), and answers as the fake did
//! (`Event::StartFailed` when the start fails, `Event::Flushed` when the file is closed).
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::files;
use super::session::{AudioIn, Chan, Config, FrameSource, OpenSource, Session};
use super::state::Fault;
use super::zones::{Hook, Zones};
use crate::shared::platform::audio::{self, Kind};
use crate::shared::platform::capture::frames::Frames;
use crate::shared::platform::capture::window_rect::Follow;
use crate::shared::platform::encode::{self, FileSink, VideoFmt};

/// Free space a recording needs to begin (R-01: >= 2 GB).
pub const START_FREE: u64 = 2 * 1024 * 1024 * 1024;

/// What to record.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Request {
    pub audio_only: bool,
    /// The area in physical desktop px (video).
    pub region: Option<(i32, i32, u32, u32)>,
    /// The microphone: none = off, `Some(None)` = the OS default, `Some(Some(id))` = that one.
    pub mic: Option<Option<String>>,
    /// The PC's sound is on (the input is open either way, muted when off).
    pub sound: bool,
    /// The microphone starts muted.
    pub mic_muted: bool,
    pub fps: u32,
    /// Areas and followed windows hidden in every frame (the list is shared: the bar edits it
    /// while the recording is paused).
    pub zones: Zones,
}

/// Why a start failed: what the card says, and the words for the log and the Encoder card.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub fault: Fault,
    pub note: String,
}

/// How a recording ended: its file when there is one worth keeping, and what went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Flushed {
    pub file: Option<PathBuf>,
    pub error: Option<String>,
}

/// A frame source over the platform's screen frames.
struct Screen(Frames);

impl FrameSource for Screen {
    fn next(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        self.0.next(wait).map_err(|e| e.to_string())
    }
    fn fast(&self) -> bool {
        self.0.fast()
    }
}

/// A sound input of the platform.
struct Sound(audio::Input);

impl AudioIn for Sound {
    fn read(&mut self, wait: Duration) -> Result<Option<(Vec<f32>, std::time::Instant)>, String> {
        match self.0.read(wait) {
            Ok(p) => Ok(p.map(|p| (p.samples, p.at))),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Where a started recording is.
struct Live {
    session: Session,
    path: PathBuf,
}

#[derive(Default)]
pub struct Real {
    live: Option<Live>,
    /// The file of the last recording (kept after `flush`, until saved or deleted).
    pub file: Option<PathBuf>,
}

impl Real {
    /// Opens the encoder, the screen and the sound and starts at once.
    pub fn begin(&mut self, req: &Request) -> Result<(), Failure> {
        let refused = |why: String| {
            crate::log!("[opencircle] recording could not start: {why}");
            Failure {
                fault: Fault::Refused,
                note: why,
            }
        };
        let plan = encode::plan(!req.audio_only).map_err(|why| {
            crate::log!("[opencircle] recording could not start: {why}");
            Failure {
                fault: Fault::Encoder,
                note: why,
            }
        })?;
        let dir = files::temp_dir();
        std::fs::create_dir_all(&dir).map_err(|e| refused(e.to_string()))?;
        let free = crate::shared::platform::sys::disk::free_bytes(&dir);
        if free.is_some_and(|f| f < START_FREE) {
            return Err(refused("less than 2 GB free".into()));
        }
        let path = files::new_temp(&dir, plan.ext);
        let inputs = open_inputs(req, &|kind| {
            audio::Input::open(kind).map(|i| Box::new(Sound(i)) as Box<dyn AudioIn>)
        })?;
        let (video, source) = match (req.audio_only, req.region) {
            // (even sides: the picture the capture is asked for is what is pushed)
            (false, Some((x, y, w, h))) => {
                let (w, h) = (w & !1, h & !1);
                let fmt = VideoFmt::of(w, h, req.fps.max(1));
                let open: OpenSource =
                    Box::new(move || Box::new(Screen(Frames::open((x, y, w, h)))));
                (Some(fmt), Some(open))
            }
            (false, None) => return Err(refused("no area to record".into())),
            (true, _) => (None, None),
        };
        let sink = FileSink::open(&path, video, !inputs.is_empty()).map_err(refused)?;
        let probe = path.clone();
        let mut cfg = Config::new(
            req.fps,
            Box::new(move || crate::shared::platform::sys::disk::free_bytes(&probe)),
        );
        cfg.frame_bytes = video.map(|v| v.frame_bytes());
        if let (Some(v), Some((x, y, _, _))) = (video, req.region) {
            let mut follow = Follow::default();
            cfg.zones = Some(Hook {
                zones: req.zones.clone(),
                origin: (x, y),
                size: v.from,
                seed: crate::features::snip::annotate::blur::seed(),
                lookup: Box::new(move |handle| follow.rect(handle)),
                cache: Default::default(),
            });
        }
        // (the start state: nothing of a muted source is ever heard, not even the first packets)
        (cfg.mic_muted, cfg.sound_muted) = (req.mic_muted, !req.sound);
        let session = Session::start(cfg, source, inputs, sink);
        self.live = Some(Live { session, path });
        self.file = None;
        Ok(())
    }

    pub fn pause(&self) {
        if let Some(l) = &self.live {
            l.session.pause();
        }
    }

    pub fn resume(&self) {
        if let Some(l) = &self.live {
            l.session.resume();
        }
    }

    pub fn mute_mic(&self, muted: bool) {
        if let Some(l) = &self.live {
            l.session.mute_mic(muted);
        }
    }

    pub fn mute_sound(&self, muted: bool) {
        if let Some(l) = &self.live {
            l.session.mute_sound(muted);
        }
    }

    /// Switches the live microphone input to `id` (`None` = OS default).
    pub fn switch_mic(&self, id: Option<String>) -> Result<(), String> {
        let Some(l) = &self.live else {
            return Ok(());
        };
        let input = audio::Input::open(Kind::Mic(id)).map_err(|e| e.to_string())?;
        l.session.replace_mic(Box::new(Sound(input)));
        Ok(())
    }

    /// True when an unmuted microphone has been delivering continuous all-zero packets.
    pub fn mic_silent(&self) -> bool {
        self.live.as_ref().is_some_and(|l| l.session.mic_silent())
    }

    pub fn level(&self) -> f32 {
        self.live.as_ref().map_or(0.0, |l| l.session.level())
    }

    /// Why the input ended, if it did (disk low, stream ended, microphone gone, write failed).
    pub fn fault(&self) -> Option<Fault> {
        self.live.as_ref().and_then(|l| l.session.fault())
    }

    /// Stops, writes the rest and closes the file ON A WORKER (the UI never waits for the
    /// encoder, a desktop's dialog or a slow disk): the answer comes on the receiver.
    pub fn flush_async(&mut self) -> Option<Receiver<Flushed>> {
        let live = self.live.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(finish(live));
        });
        Some(rx)
    }

    /// Stops what runs and deletes the temp file (the bin; leaving the error card): the stop is
    /// on a worker, the UI does not wait.
    pub fn discard(&mut self) {
        // marked first (cheap): a worker killed before it deletes leaves a file the next start
        // sweeps, not one it offers
        if let Some(path) = self.file.take() {
            files::mark_discarded(&path);
            delete(&path);
            files::unmark(&path);
        }
        if let Some(live) = self.live.take() {
            files::mark_discarded(&live.path);
            std::thread::spawn(move || {
                let path = live.path.clone();
                let _ = live.session.stop();
                delete(&path);
                files::unmark(&path);
            });
        }
    }
}

type Inputs = Vec<(Chan, Box<dyn AudioIn>)>;

/// The sound inputs of a recording, opened with `open`: the microphone when asked for, and the
/// system sound WHATEVER the switch says (it starts muted when the switch is off): turning it
/// on mid-recording then needs no new input.
fn open_inputs(
    req: &Request,
    open: &dyn Fn(Kind) -> Result<Box<dyn AudioIn>, audio::Error>,
) -> Result<Inputs, Failure> {
    let mut inputs = Vec::new();
    if let Some(id) = &req.mic {
        let input = open(Kind::Mic(id.clone())).map_err(|e| {
            crate::log!("[opencircle] recording: no microphone ({e})");
            Failure {
                fault: Fault::Mic,
                note: e.to_string(),
            }
        })?;
        inputs.push((Chan::Mic, input));
    }
    match open(Kind::System) {
        Ok(input) => inputs.push((Chan::Sound, input)),
        Err(e) => crate::log!("[opencircle] recording: no system sound ({e})"),
    }
    Ok(inputs)
}

/// Stops the session and says how it ended.
fn finish(live: Live) -> Flushed {
    let error = match live.session.stop() {
        Ok(stats) => {
            crate::log!(
                "[opencircle] recording done: {} frames, {} dropped, {:?}",
                stats.frames,
                stats.dropped,
                stats.elapsed
            );
            None
        }
        Err(e) => {
            crate::log!("[opencircle] recording ended with an error: {e}");
            Some(e)
        }
    };
    let kept = std::fs::metadata(&live.path).is_ok_and(|m| m.len() > 0);
    Flushed {
        file: kept.then_some(live.path),
        error,
    }
}

fn delete(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        crate::log!(
            "[opencircle] recording: could not delete {}: {e}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Silent;
    impl AudioIn for Silent {
        fn read(&mut self, _: Duration) -> Result<Option<(Vec<f32>, std::time::Instant)>, String> {
            Ok(None)
        }
    }

    /// R04B-8: the system input is opened even with the sound switch off (muted at the start),
    /// and the microphone only when asked for.
    #[test]
    fn r04b_8_the_system_input_opens_whatever_the_switch_says() {
        for (sound, mic) in [(false, None), (false, Some(None)), (true, None)] {
            let req = Request {
                sound,
                mic: mic.clone(),
                ..Request::default()
            };
            let asked = std::cell::RefCell::new(Vec::new());
            let open = |k: Kind| -> Result<Box<dyn AudioIn>, audio::Error> {
                asked.borrow_mut().push(k);
                Ok(Box::new(Silent))
            };
            let inputs = open_inputs(&req, &open).ok().unwrap();
            assert!(asked.borrow().contains(&Kind::System), "sound {sound}");
            assert_eq!(asked.borrow().contains(&Kind::Mic(None)), mic.is_some());
            assert_eq!(inputs.iter().filter(|(c, _)| *c == Chan::Sound).count(), 1);
        }
    }

    /// R04C live (Windows PC or VM; play a tone first): an audio-only recording of 5 s with the
    /// microphone and the PC's sound; the file's path goes to `OC_REC_OUT` (else is printed).
    /// `cargo test live_recording_keeps_the_sound -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real sound devices; run on a PC or the Windows VM"]
    fn live_recording_keeps_the_sound() {
        let mut real = Real::default();
        // OC_VIDEO=1: a 640 x 360 area of the screen too (the sound goes into the MP4)
        let video = std::env::var_os("OC_VIDEO").is_some();
        let req = Request {
            audio_only: !video,
            region: video.then_some((0, 0, 640, 360)),
            // OC_MIC_INDEX: the n-th listed microphone BY ITS ID (the bar's picker), else the default
            mic: Some(std::env::var("OC_MIC_INDEX").ok().and_then(|n| {
                let list = audio::devices(Kind::Mic(None)).ok()?;
                Some(list.get(n.parse::<usize>().ok()?)?.id.clone())
            })),
            sound: true,
            fps: 30,
            ..Request::default()
        };
        real.begin(&req).map_err(|f| f.note).expect("begin");
        std::thread::sleep(Duration::from_secs(5));
        let done = real.flush_async().expect("live").recv().expect("flushed");
        assert_eq!(done.error, None);
        let file = done.file.expect("a file");
        match std::env::var_os("OC_REC_OUT") {
            Some(to) => drop(std::fs::copy(&file, to).expect("copy")),
            None => println!("recording: {}", file.display()),
        }
    }
}
