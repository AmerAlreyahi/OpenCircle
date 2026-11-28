//! One recording's threads (R-04): the capture thread (a frame every 1/fps, the last one again
//! while the screen is still: constant rate) -> a bounded queue -> the sink thread (mixes the
//! sound, writes the file). Pure of the OS: frames come from a `FrameSource`, sound from
//! `AudioIn`s and the file goes to a `Sink`, so tests use fakes.
//!
//! - The queue holds `Config::queue` frames; a full one drops the new frame and counts it
//!   (memory stays flat). Stop writes every queued frame first.
//! - Every `Config::disk_every` the free space at the file is read; below `Config::disk_min` a
//!   `Fault::DiskLow` is raised once: the App stops and keeps the file.
//! - A source that errors raises `Fault::StreamEnded` (a window closed, the screen went off).
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::clock::Clock;
use super::mix::Mixer;
use super::state::Fault;
use super::zones::Hook;
use crate::shared::platform::capture::window_rect::WinRect;
use crate::shared::platform::encode::Sink;

/// Frames of the screen, RGBA, as the screen shows them.
pub trait FrameSource {
    /// The newest frame within `wait`; none when nothing new was shown. An error ends the recording.
    fn next(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String>;
    /// Frames come by themselves (`next` waits for them); else `next` grabs at once.
    fn fast(&self) -> bool;
}

/// Opens the frame source on the capture thread (a source may not be `Send`).
pub type OpenSource = Box<dyn FnOnce() -> Box<dyn FrameSource> + Send>;

/// Which sound a packet is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chan {
    Mic,
    Sound,
}

/// A sound input: interleaved stereo 48 kHz packets with the time they were taken.
pub trait AudioIn: Send {
    fn read(&mut self, wait: Duration) -> Result<Option<(Vec<f32>, Instant)>, String>;
}

pub struct Config {
    pub fps: u32,
    /// Frames the queue holds between capture and the file.
    pub queue: usize,
    /// Free bytes at the file below which the recording stops.
    pub disk_min: u64,
    pub disk_every: Duration,
    /// The free bytes where the file is (none: unknown, never "low").
    pub free: Box<dyn Fn() -> Option<u64> + Send>,
    /// Bytes of every frame a source gives (width x height x 4): another size is a fault
    /// (a stream that changed size under us), never fed to the encoder.
    pub frame_bytes: Option<usize>,
    /// The mute state the recording STARTS in (a muted source never lets one sample in).
    pub mic_muted: bool,
    pub sound_muted: bool,
    /// Blur zones (R-06): hidden in every frame BEFORE it is queued for the encoder.
    pub zones: Option<Hook>,
    /// Continuous all-zero frames from an unmuted microphone before raising the silent-mic notice
    /// (default: 3 s at 48 kHz; `0` disables).
    pub mic_silent_frames: u64,
}

impl Config {
    pub fn new(fps: u32, free: Box<dyn Fn() -> Option<u64> + Send>) -> Config {
        Config {
            fps: fps.max(1),
            queue: 6,
            disk_min: 500 * 1024 * 1024,
            disk_every: Duration::from_secs(10),
            free,
            frame_bytes: None,
            mic_muted: false,
            sound_muted: false,
            zones: None,
            mic_silent_frames: 3 * u64::from(crate::shared::platform::audio::RATE),
        }
    }
}

/// What a finished recording says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub frames: u32,
    pub dropped: u32,
    pub elapsed: Duration,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    mic_muted: AtomicBool,
    sound_muted: AtomicBool,
    frames: AtomicU32,
    dropped: AtomicU32,
    /// The mixed sound's level (`f32` bits), for the bar's waveform.
    level: AtomicU32,
    /// Frames of sound written (the time of the next sample).
    audio_frames: AtomicU64,
    /// True when an unmuted microphone has delivered continuous all-zero packets for `mic_silent_frames`.
    mic_silent: AtomicBool,
}

enum Msg {
    Audio(Chan, Vec<f32>, Instant),
    MicSwitched,
    Pause(bool),
    Stop,
}

struct Frame(Arc<Vec<u8>>, Duration);

pub struct Session {
    shared: Arc<Shared>,
    origin: Instant,
    clock: Arc<Mutex<Clock>>,
    control: Sender<Msg>,
    fault_tx: Sender<Fault>,
    faults: Receiver<Fault>,
    mic_stop: Mutex<Arc<AtomicBool>>,
    mic_thread: Mutex<Option<JoinHandle<()>>>,
    threads: Vec<JoinHandle<()>>,
    sink: JoinHandle<Result<(), String>>,
}

impl Session {
    /// Starts at once: `video` none = audio only. `inputs` are the sound sources in use.
    pub fn start(
        mut cfg: Config,
        video: Option<OpenSource>,
        inputs: Vec<(Chan, Box<dyn AudioIn>)>,
        sink: Box<dyn Sink>,
    ) -> Session {
        let shared = Arc::new(Shared::default());
        shared.mic_muted.store(cfg.mic_muted, Ordering::SeqCst);
        shared.sound_muted.store(cfg.sound_muted, Ordering::SeqCst);
        let origin = Instant::now();
        let clock = Arc::new(Mutex::new(Clock::start(Duration::ZERO)));
        let (control, control_rx) = mpsc::channel();
        let (fault_tx, faults) = mpsc::channel();
        let (frames_tx, frames_rx) = mpsc::sync_channel::<Frame>(cfg.queue.max(1));
        let mut threads = Vec::new();
        if let Some(open) = video {
            let (sh, cl, ft) = (shared.clone(), clock.clone(), fault_tx.clone());
            let (fps, bytes) = (cfg.fps, cfg.frame_bytes);
            let hook = cfg.zones.take();
            // (opened on its own thread: a desktop's permission dialog must not stall the UI)
            threads.push(thread::spawn(move || {
                capture(open(), (fps, bytes), hook, origin, &sh, &cl, frames_tx, &ft)
            }));
        }
        let (has_mic, has_sound) = (
            inputs.iter().any(|(c, _)| *c == Chan::Mic),
            inputs.iter().any(|(c, _)| *c == Chan::Sound),
        );
        let mic_stop = Arc::new(AtomicBool::new(false));
        let mut mic_thread = None;
        for (chan, input) in inputs {
            let (sh, tx, ft) = (shared.clone(), control.clone(), fault_tx.clone());
            if chan == Chan::Mic {
                let stop = mic_stop.clone();
                mic_thread = Some(thread::spawn(move || {
                    listen(chan, input, &sh, Some(stop), tx, &ft)
                }));
            } else {
                threads.push(thread::spawn(move || {
                    listen(chan, input, &sh, None, tx, &ft)
                }));
            }
        }
        let sh = shared.clone();
        let writer_faults = fault_tx.clone();
        let sink = thread::spawn(move || {
            write(
                cfg,
                sink,
                origin,
                &sh,
                (has_mic, has_sound),
                frames_rx,
                control_rx,
                writer_faults,
            )
        });
        Session {
            shared,
            origin,
            clock,
            control,
            fault_tx,
            faults,
            mic_stop: Mutex::new(mic_stop),
            mic_thread: Mutex::new(mic_thread),
            threads,
            sink,
        }
    }

    pub fn pause(&self) {
        self.clock.lock().unwrap().pause(self.origin.elapsed());
        self.shared.paused.store(true, Ordering::SeqCst);
        let _ = self.control.send(Msg::Pause(true));
    }

    pub fn resume(&self) {
        self.clock.lock().unwrap().resume(self.origin.elapsed());
        self.shared.paused.store(false, Ordering::SeqCst);
        let _ = self.control.send(Msg::Pause(false));
    }

    pub fn mute_mic(&self, muted: bool) {
        self.shared.mic_muted.store(muted, Ordering::SeqCst);
        if muted {
            self.shared.mic_silent.store(false, Ordering::Relaxed);
        }
    }

    /// Replaces the live microphone input mid-recording: stops the previous mic listener thread
    /// cleanly and starts reading from `input` without interrupting video or system sound.
    pub fn replace_mic(&self, input: Box<dyn AudioIn>) {
        let next_stop = Arc::new(AtomicBool::new(false));
        {
            let mut guard = self.mic_stop.lock().unwrap();
            guard.store(true, Ordering::SeqCst);
            *guard = next_stop.clone();
        }
        let prev = self.mic_thread.lock().unwrap().take();
        if let Some(t) = prev {
            let _ = t.join();
        }
        self.shared.mic_silent.store(false, Ordering::Relaxed);
        let _ = self.control.send(Msg::MicSwitched);
        let (sh, tx, ft) = (
            self.shared.clone(),
            self.control.clone(),
            self.fault_tx.clone(),
        );
        let handle = thread::spawn(move || {
            listen(Chan::Mic, input, &sh, Some(next_stop), tx, &ft);
        });
        *self.mic_thread.lock().unwrap() = Some(handle);
    }

    /// True when the unmuted microphone has delivered continuous all-zero packets.
    pub fn mic_silent(&self) -> bool {
        self.shared.mic_silent.load(Ordering::Relaxed)
    }

    pub fn mute_sound(&self, muted: bool) {
        self.shared.sound_muted.store(muted, Ordering::SeqCst);
    }

    /// The time recorded so far (pauses not counted).
    pub fn elapsed(&self) -> Duration {
        self.clock.lock().unwrap().elapsed(self.origin.elapsed())
    }

    /// The mixed sound's level, 0..1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// The first problem that ended the recording's input, if one came (disk low, stream ended).
    pub fn fault(&self) -> Option<Fault> {
        self.faults.try_recv().ok()
    }

    /// Stops the inputs, writes every queued frame and the file's end. Returns what was recorded.
    pub fn stop(self) -> Result<Stats, String> {
        let elapsed = self.elapsed();
        self.shared.stop.store(true, Ordering::SeqCst);
        self.mic_stop.lock().unwrap().store(true, Ordering::SeqCst);
        if let Some(t) = self.mic_thread.lock().unwrap().take() {
            let _ = t.join();
        }
        for t in self.threads {
            let _ = t.join();
        }
        let _ = self.control.send(Msg::Stop);
        let done = self
            .sink
            .join()
            .map_err(|_| "the writer stopped".to_string())?;
        done?;
        Ok(Stats {
            frames: self.shared.frames.load(Ordering::Relaxed),
            dropped: self.shared.dropped.load(Ordering::Relaxed),
            elapsed,
        })
    }
}

/// The raw frame, the zones it was hidden for, and its hidden copy.
type Hidden = (Arc<Vec<u8>>, Vec<WinRect>, Arc<Vec<u8>>);

/// The capture thread: a frame every 1/fps, the newest one again while nothing new came. With
/// zones the frame kept is the raw one and every frame queued is its hidden copy (made again
/// only when the picture or a zone moved), so a zone added in a pause hides the very next frame.
#[allow(clippy::too_many_arguments)]
fn capture(
    mut src: Box<dyn FrameSource>,
    (fps, frame_bytes): (u32, Option<usize>),
    mut hook: Option<Hook>,
    origin: Instant,
    shared: &Shared,
    clock: &Mutex<Clock>,
    queue: SyncSender<Frame>,
    faults: &Sender<Fault>,
) {
    let period = Duration::from_secs(1) / fps;
    let mut next = Instant::now();
    let mut last: Option<Arc<Vec<u8>>> = None;
    // the hidden copy of `last` and the zones it was made for
    let mut hidden: Option<Hidden> = None;
    while !shared.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        let wait = next.saturating_duration_since(now);
        let got = match src.fast() {
            true => src.next(wait.max(Duration::from_millis(1))),
            false => {
                thread::sleep(wait);
                src.next(Duration::ZERO)
            }
        };
        match got {
            Ok(Some(f)) if frame_bytes.is_some_and(|n| f.len() != n) => {
                crate::log!(
                    "[opencircle] recording: a frame of {} bytes, {} expected",
                    f.len(),
                    frame_bytes.unwrap_or(0)
                );
                let _ = faults.send(Fault::StreamEnded);
                return;
            }
            Ok(Some(f)) => last = Some(Arc::new(f)),
            Ok(None) => {}
            Err(e) => {
                crate::log!("[opencircle] recording: the screen frames ended ({e})");
                let _ = faults.send(Fault::StreamEnded);
                return;
            }
        }
        let now = Instant::now();
        if now < next {
            continue;
        }
        next += period;
        if next < now {
            // fell behind (a slow capture or a big zone): no burst to catch up, and the frames
            // that were due are counted as lost
            let lost = now.duration_since(next).as_nanos() / period.as_nanos().max(1) + 1;
            shared.dropped.fetch_add(lost as u32, Ordering::Relaxed);
            next = now + period;
        }
        if shared.paused.load(Ordering::SeqCst) {
            continue;
        }
        let Some(raw) = &last else { continue };
        let frame = match &mut hook {
            None => raw.clone(),
            Some(h) => {
                let rects = h.rects();
                match &hidden {
                    _ if rects.is_empty() => raw.clone(),
                    Some((from, was, done)) if Arc::ptr_eq(from, raw) && *was == rects => {
                        done.clone()
                    }
                    _ => {
                        let done = Arc::new(h.hidden(raw, &rects));
                        hidden = Some((raw.clone(), rects, done.clone()));
                        done
                    }
                }
            }
        };
        let pts = clock.lock().unwrap().elapsed(now.duration_since(origin));
        match queue.try_send(Frame(frame, pts)) {
            Ok(()) => {
                shared.frames.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_)) => {
                shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => return,
        }
    }
}

/// One sound input's thread: packets to the writer (none while paused).
fn listen(
    chan: Chan,
    mut input: Box<dyn AudioIn>,
    shared: &Shared,
    local_stop: Option<Arc<AtomicBool>>,
    tx: Sender<Msg>,
    faults: &Sender<Fault>,
) {
    while !shared.stop.load(Ordering::SeqCst)
        && !local_stop
            .as_ref()
            .is_some_and(|s| s.load(Ordering::SeqCst))
    {
        match input.read(Duration::from_millis(50)) {
            Ok(Some((samples, at))) if !shared.paused.load(Ordering::SeqCst) => {
                if tx.send(Msg::Audio(chan, samples, at)).is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(e) => {
                crate::log!("[opencircle] recording: {chan:?} stopped ({e})");
                if chan == Chan::Mic {
                    let _ = faults.send(Fault::Mic);
                }
                return;
            }
        }
    }
}

/// The sink thread: frames and the mixed sound into the file, the disk watch, the end.
#[allow(clippy::too_many_arguments)]
fn write(
    cfg: Config,
    mut sink: Box<dyn Sink>,
    origin: Instant,
    shared: &Shared,
    (has_mic, has_sound): (bool, bool),
    frames: Receiver<Frame>,
    control: Receiver<Msg>,
    faults: Sender<Fault>,
) -> Result<(), String> {
    crate::shared::platform::encode::thread_init();
    let new_mixer = || {
        let mut m = Mixer::stereo_48k(has_mic, has_sound);
        m.start(Instant::now());
        m
    };
    // the shared mute flags go into the mixer BEFORE any sample is pushed: off means off
    let sync = |mixer: &mut Mixer| {
        mixer.set_mic_muted(shared.mic_muted.load(Ordering::SeqCst));
        mixer.set_system_muted(shared.sound_muted.load(Ordering::SeqCst));
    };
    let mut mixer = new_mixer();
    sync(&mut mixer);
    let mut mixed = Vec::new();
    let mut last_check = Instant::now();
    let mut raised = false;
    let mut failed: Option<String> = None;
    let audio = |sink: &mut Box<dyn Sink>, mixed: &mut Vec<f32>, mixer: &mut Mixer, dead: bool| {
        sync(mixer);
        mixed.clear();
        let n = mixer.drain_up_to(Instant::now(), mixed);
        shared
            .level
            .store(mixer.level().to_bits(), Ordering::Relaxed);
        if n == 0 || dead {
            return Ok(());
        }
        let at = shared.audio_frames.fetch_add(n as u64, Ordering::SeqCst);
        sink.audio(mixed, Duration::from_secs_f64(at as f64 / 48_000.0))
    };
    let mut stopping = false;
    let mut has_mic_now = has_mic;
    let mut mic_silent_frames: u64 = 0;
    let mut mic_pkts: u64 = 0;
    let mut mic_peak: f32 = 0.0;
    let mut sound_pkts: u64 = 0;
    let mut sound_peak: f32 = 0.0;
    // a write that failed: said once at once (the bar stops and keeps the file), nothing more is
    // written (the file is as valid as it got)
    let fail = |failed: &mut Option<String>, e: String| {
        crate::log!("[opencircle] recording: the file could not be written ({e})");
        if failed.is_none() {
            let _ = faults.send(Fault::WriteFailed);
        }
        failed.get_or_insert(e);
    };
    while !stopping {
        let mut idle = true;
        // one frame, then the sound: neither starves the other
        if let Ok(Frame(rgba, pts)) = frames.try_recv() {
            idle = false;
            if failed.is_none()
                && let Err(e) = sink.video(&rgba, pts)
            {
                fail(&mut failed, e);
            }
        }
        while let Ok(msg) = control.try_recv() {
            idle = false;
            match msg {
                Msg::Audio(Chan::Mic, s, at) => {
                    mic_pkts += 1;
                    let pkt_peak = s
                        .iter()
                        .filter(|v| v.is_finite())
                        .map(|v| v.abs())
                        .fold(0.0f32, f32::max);
                    mic_peak = mic_peak.max(pkt_peak);
                    if !shared.mic_muted.load(Ordering::SeqCst)
                        && !shared.paused.load(Ordering::SeqCst)
                    {
                        if pkt_peak > 1e-6 {
                            mic_silent_frames = 0;
                            shared.mic_silent.store(false, Ordering::Relaxed);
                        } else {
                            mic_silent_frames += (s.len() / 2) as u64;
                            if cfg.mic_silent_frames > 0
                                && mic_silent_frames >= cfg.mic_silent_frames
                            {
                                shared.mic_silent.store(true, Ordering::Relaxed);
                            }
                        }
                    } else {
                        mic_silent_frames = 0;
                        shared.mic_silent.store(false, Ordering::Relaxed);
                    }
                    sync(&mut mixer);
                    mixer.push_mic(&s, at)
                }
                Msg::Audio(Chan::Sound, s, at) => {
                    sound_pkts += 1;
                    let pkt_peak = s
                        .iter()
                        .filter(|v| v.is_finite())
                        .map(|v| v.abs())
                        .fold(0.0f32, f32::max);
                    sound_peak = sound_peak.max(pkt_peak);
                    sync(&mut mixer);
                    mixer.push_system(&s, at)
                }
                Msg::MicSwitched => {
                    has_mic_now = true;
                    mixer.enable_mic();
                    mic_silent_frames = 0;
                    shared.mic_silent.store(false, Ordering::Relaxed);
                }
                Msg::Pause(true) => {
                    mic_silent_frames = 0;
                    shared.mic_silent.store(false, Ordering::Relaxed);
                    sink.pause(true);
                }
                Msg::Pause(false) => {
                    // the pause is not in the sound: a mixer that starts again from now
                    mic_silent_frames = 0;
                    shared.mic_silent.store(false, Ordering::Relaxed);
                    mixer = Mixer::stereo_48k(has_mic_now, has_sound);
                    mixer.start(Instant::now());
                    sync(&mut mixer);
                    sink.pause(false);
                }
                Msg::Stop => stopping = true,
            }
        }
        if !shared.paused.load(Ordering::SeqCst)
            && let Err(e) = audio(&mut sink, &mut mixed, &mut mixer, failed.is_some())
        {
            fail(&mut failed, e);
        }
        if last_check.elapsed() >= cfg.disk_every {
            last_check = Instant::now();
            if !raised && (cfg.free)().is_some_and(|free| free < cfg.disk_min) {
                raised = true;
                let _ = faults.send(Fault::DiskLow);
            }
        }
        if idle && !stopping {
            thread::sleep(Duration::from_millis(5));
        }
    }
    // Stop: every queued frame, then the last of the sound
    while let Ok(Frame(rgba, pts)) = frames.try_recv() {
        if failed.is_none()
            && let Err(e) = sink.video(&rgba, pts)
        {
            fail(&mut failed, e);
        }
    }
    let _ = origin;
    mixed.clear();
    if mixer.flush(&mut mixed) > 0 && failed.is_none() {
        let at = shared
            .audio_frames
            .fetch_add((mixed.len() / 2) as u64, Ordering::SeqCst);
        if let Err(e) = sink.audio(&mixed, Duration::from_secs_f64(at as f64 / 48_000.0)) {
            fail(&mut failed, e);
        }
    }
    if has_mic_now || has_sound {
        crate::log!(
            "[opencircle] recording audio: mic {mic_pkts} pkts (peak {mic_peak:.4}), sound {sound_pkts} pkts (peak {sound_peak:.4})"
        );
    }
    let closed = sink.finish();
    match (failed, closed) {
        (_, Err(e)) => Err(e),
        (Some(e), _) => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A source of numbered frames, one new every `every` calls.
    struct Fake {
        n: u8,
        calls: u32,
        every: u32,
        fail_after: Option<u32>,
    }

    impl FrameSource for Fake {
        fn next(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
            thread::sleep(wait.min(Duration::from_millis(2)));
            self.calls += 1;
            if self.fail_after.is_some_and(|f| self.calls > f) {
                return Err("window closed".into());
            }
            if self.calls % self.every == 1 % self.every {
                self.n += 1;
                return Ok(Some(vec![self.n; 16]));
            }
            Ok(None)
        }
        fn fast(&self) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct Log {
        frames: Mutex<Vec<(u8, Duration)>>,
        audio_frames: AtomicUsize,
        /// The loudest sample the sink was given (`f32` bits).
        peak: AtomicU32,
        pauses: Mutex<Vec<bool>>,
        finished: AtomicBool,
        /// The pictures the sink was given (R-06).
        pictures: Mutex<Vec<Vec<u8>>>,
    }

    struct FakeSink {
        log: Arc<Log>,
        slow: Duration,
    }

    impl Sink for FakeSink {
        fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
            thread::sleep(self.slow);
            self.log.frames.lock().unwrap().push((rgba[0], pts));
            if rgba.len() > 16 {
                self.log.pictures.lock().unwrap().push(rgba.to_vec());
            }
            Ok(())
        }
        fn audio(&mut self, pcm: &[f32], _pts: Duration) -> Result<(), String> {
            self.log
                .audio_frames
                .fetch_add(pcm.len() / 2, Ordering::SeqCst);
            let loud = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let seen = f32::from_bits(self.log.peak.load(Ordering::SeqCst));
            self.log
                .peak
                .store(seen.max(loud).to_bits(), Ordering::SeqCst);
            Ok(())
        }
        fn pause(&mut self, paused: bool) {
            self.log.pauses.lock().unwrap().push(paused);
        }
        fn finish(self: Box<Self>) -> Result<(), String> {
            self.log.finished.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Tone;
    impl AudioIn for Tone {
        fn read(&mut self, wait: Duration) -> Result<Option<(Vec<f32>, Instant)>, String> {
            thread::sleep(wait.min(Duration::from_millis(10)));
            Ok(Some((vec![0.1; 960], Instant::now())))
        }
    }

    fn cfg(free: u64) -> Config {
        let mut c = Config::new(50, Box::new(move || Some(free)));
        c.disk_every = Duration::from_millis(20);
        c
    }

    fn run(
        cfg: Config,
        src: Fake,
        slow: Duration,
        inputs: Vec<(Chan, Box<dyn AudioIn>)>,
    ) -> (Session, Arc<Log>) {
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow,
        });
        (
            Session::start(cfg, Some(Box::new(move || Box::new(src))), inputs, sink),
            log,
        )
    }

    fn fake(every: u32) -> Fake {
        Fake {
            n: 0,
            calls: 0,
            every,
            fail_after: None,
        }
    }

    /// R04-20: a frame every 1/fps, the last one again while the screen is still.
    #[test]
    fn r04_20_a_still_screen_repeats_the_last_frame_at_a_constant_rate() {
        let (s, log) = run(cfg(u64::MAX), fake(1_000_000), Duration::ZERO, vec![]);
        thread::sleep(Duration::from_millis(400));
        let stats = s.stop().unwrap();
        let frames = log.frames.lock().unwrap();
        assert!((15..=22).contains(&frames.len()), "{} frames", frames.len());
        assert!(
            frames.iter().all(|(n, _)| *n == 1),
            "the one frame, repeated"
        );
        assert!(frames.windows(2).all(|w| w[1].1 > w[0].1), "times rise");
        // `dropped` also counts ticks lost to falling behind (R-06): a busy CI runner can miss a
        // tick or two; a still screen must never drop more than that.
        assert!(stats.dropped <= 2, "{} dropped", stats.dropped);
        assert!(log.finished.load(Ordering::SeqCst));
    }

    /// R04-21: the queue is bounded: a slow file drops (and counts) frames, and Stop
    /// still writes every frame that was queued.
    #[test]
    fn r04_21_a_slow_sink_drops_counted_frames_and_stop_flushes_the_queue() {
        let (s, log) = run(cfg(u64::MAX), fake(1), Duration::from_millis(60), vec![]);
        thread::sleep(Duration::from_millis(500));
        let queued = s.shared.frames.load(Ordering::Relaxed);
        let stats = s.stop().unwrap();
        assert!(stats.dropped > 5, "{stats:?}");
        let written = log.frames.lock().unwrap().len() as u32;
        assert!(
            written >= queued,
            "stop flushed: {written} written of {queued} queued"
        );
        assert_eq!(written, stats.frames, "every counted frame was written");
    }

    /// R04-22: low disk raises DiskLow once; the file is still closed (what we have is kept).
    #[test]
    fn r04_22_a_low_disk_raises_one_fault_and_the_file_is_still_closed() {
        let (s, log) = run(cfg(1024), fake(1), Duration::ZERO, vec![]);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(s.fault(), Some(Fault::DiskLow));
        thread::sleep(Duration::from_millis(100));
        assert_eq!(s.fault(), None, "once");
        s.stop().unwrap();
        assert!(log.finished.load(Ordering::SeqCst));
    }

    /// R04-23: a source that errors raises StreamEnded; stopping afterwards keeps the file.
    #[test]
    fn r04_23_a_stream_that_ends_is_a_fault_and_the_file_is_kept() {
        let src = Fake {
            fail_after: Some(10),
            ..fake(1)
        };
        let (s, log) = run(cfg(u64::MAX), src, Duration::ZERO, vec![]);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(s.fault(), Some(Fault::StreamEnded));
        s.stop().unwrap();
        assert!(log.finished.load(Ordering::SeqCst));
    }

    /// R04-24: while paused no frame is written and the time stands still; resume goes on.
    #[test]
    fn r04_24_pause_writes_nothing_and_the_clock_stands_still() {
        let (s, log) = run(cfg(u64::MAX), fake(1), Duration::ZERO, vec![]);
        thread::sleep(Duration::from_millis(150));
        s.pause();
        thread::sleep(Duration::from_millis(60));
        let (n, at) = (log.frames.lock().unwrap().len(), s.elapsed());
        thread::sleep(Duration::from_millis(200));
        assert_eq!(log.frames.lock().unwrap().len(), n, "nothing while paused");
        assert_eq!(s.elapsed(), at);
        s.resume();
        thread::sleep(Duration::from_millis(150));
        assert!(log.frames.lock().unwrap().len() > n);
        let frames = log.frames.lock().unwrap().clone();
        let last = frames.last().unwrap().1;
        assert!(
            last < Duration::from_millis(600),
            "the pause is not in the times: {last:?}"
        );
        s.stop().unwrap();
        assert_eq!(*log.pauses.lock().unwrap(), [true, false]);
    }

    /// R04-25: sound reaches the sink, a muted microphone writes zeros (the mixer), audio
    /// only works with no video at all.
    #[test]
    fn r04_25_audio_only_writes_the_mixed_sound() {
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let s = Session::start(cfg(u64::MAX), None, vec![(Chan::Mic, Box::new(Tone))], sink);
        thread::sleep(Duration::from_millis(300));
        s.stop().unwrap();
        let n = log.audio_frames.load(Ordering::SeqCst);
        assert!(n > 48_000 / 10, "{n} frames of sound");
        assert!(log.frames.lock().unwrap().is_empty());
    }

    /// A real-time 440 Hz tone in 20 ms packets, as the capture delivers them (`stamp_start`:
    /// stamped with the packet's start, as `AudioSender` does; else with its end).
    struct Sine {
        n: u64,
        amp: f32,
        stamp_start: bool,
    }
    impl AudioIn for Sine {
        fn read(&mut self, _wait: Duration) -> Result<Option<(Vec<f32>, Instant)>, String> {
            thread::sleep(Duration::from_millis(20));
            let now = Instant::now();
            let mut pcm = Vec::with_capacity(1920);
            for _ in 0..960 {
                let v = (self.n as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * self.amp;
                pcm.extend([v, v]);
                self.n += 1;
            }
            let at = match self.stamp_start {
                true => now - Duration::from_millis(20),
                false => now,
            };
            Ok(Some((pcm, at)))
        }
    }

    fn peak_of(inputs: Vec<(Chan, Box<dyn AudioIn>)>, mute: impl Fn(&Session)) -> f32 {
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let s = Session::start(cfg(u64::MAX), None, inputs, sink);
        mute(&s);
        thread::sleep(Duration::from_millis(600));
        s.stop().unwrap();
        f32::from_bits(log.peak.load(Ordering::SeqCst))
    }

    /// R04B-4: a tone from the microphone and/or the system sound reaches the sink as a tone,
    /// not as silence (the Windows test file read exactly -91 dB): through the whole capture ->
    /// mix -> sink path, with packets stamped at their start or at their end, one source or both,
    /// and only a muted source is zeros.
    #[test]
    fn r04b_4_a_tone_through_the_capture_mix_sink_path_is_not_silent() {
        let mic = |start| -> (Chan, Box<dyn AudioIn>) {
            (
                Chan::Mic,
                Box::new(Sine {
                    n: 0,
                    amp: 0.5,
                    stamp_start: start,
                }),
            )
        };
        let sys = |start| -> (Chan, Box<dyn AudioIn>) {
            (
                Chan::Sound,
                Box::new(Sine {
                    n: 0,
                    amp: 0.25,
                    stamp_start: start,
                }),
            )
        };
        for start in [true, false] {
            let m = peak_of(vec![mic(start)], |_| {});
            assert!(m > 0.4, "mic only (stamp at start: {start}): peak {m}");
            let s = peak_of(vec![sys(start)], |_| {});
            assert!(s > 0.2, "system only (stamp at start: {start}): peak {s}");
            let both = peak_of(vec![mic(start), sys(start)], |_| {});
            assert!(
                both > 0.6,
                "both add up (stamp at start: {start}): peak {both}"
            );
        }
        assert_eq!(peak_of(vec![mic(true)], |s| s.mute_mic(true)), 0.0, "muted");
        let only_mic = peak_of(vec![mic(true), sys(true)], |s| s.mute_sound(true));
        assert!(only_mic > 0.4 && only_mic < 0.6, "system muted: {only_mic}");
    }

    /// R04B-7: the system sound is open from the start and muted: turning it on mid-recording
    /// gives real (non-silent) samples from then on, and nothing before.
    #[test]
    fn r04b_7_system_sound_switched_on_mid_recording_is_not_silent() {
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let sys: Box<dyn AudioIn> = Box::new(Sine {
            n: 0,
            amp: 0.5,
            stamp_start: true,
        });
        let s = Session::start(cfg(u64::MAX), None, vec![(Chan::Sound, sys)], sink);
        s.mute_sound(true); // the switch is off at the start
        thread::sleep(Duration::from_millis(400));
        let before = f32::from_bits(log.peak.load(Ordering::SeqCst));
        s.mute_sound(false); // turned on mid-recording
        thread::sleep(Duration::from_millis(500));
        let after = f32::from_bits(log.peak.load(Ordering::SeqCst));
        s.stop().unwrap();
        assert!(before < 0.01, "silent while off: {before}");
        assert!(after > 0.3, "the sound is there once on: {after}");
    }

    /// Packets already waiting when the recording starts.
    struct Waiting(Vec<(Vec<f32>, Instant)>);
    impl AudioIn for Waiting {
        fn read(&mut self, wait: Duration) -> Result<Option<(Vec<f32>, Instant)>, String> {
            match self.0.pop() {
                Some(p) => Ok(Some(p)),
                None => {
                    thread::sleep(wait.min(Duration::from_millis(5)));
                    Ok(None)
                }
            }
        }
    }

    /// R04B-9: off means off. A source that STARTS muted lets not one sample into the file, even
    /// when packets were already waiting at the start (50 starts, peak exactly 0); and the same
    /// packets are heard when it starts unmuted.
    #[test]
    fn r04b_9_a_source_that_starts_muted_never_leaks_a_sample() {
        let run = |chan: Chan, muted: bool| {
            let log = Arc::new(Log::default());
            let sink = Box::new(FakeSink {
                log: log.clone(),
                slow: Duration::ZERO,
            });
            let now = Instant::now();
            let packets = (0..20).map(|_| (vec![0.5; 1920], now)).collect();
            let mut c = cfg(u64::MAX);
            (c.mic_muted, c.sound_muted) =
                (muted && chan == Chan::Mic, muted && chan == Chan::Sound);
            let s = Session::start(c, None, vec![(chan, Box::new(Waiting(packets)))], sink);
            thread::sleep(Duration::from_millis(40));
            s.stop().unwrap();
            f32::from_bits(log.peak.load(Ordering::SeqCst))
        };
        for chan in [Chan::Sound, Chan::Mic] {
            for _ in 0..50 {
                assert_eq!(run(chan, true), 0.0, "{chan:?} muted at the start");
            }
            assert!(run(chan, false) > 0.4, "{chan:?} heard when not muted");
        }
    }

    /// A sink that fails after `ok` frames.
    struct Breaking {
        ok: usize,
        seen: usize,
        log: Arc<Log>,
    }

    impl Sink for Breaking {
        fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
            self.seen += 1;
            match self.seen > self.ok {
                true => Err("disk full".into()),
                false => {
                    self.log.frames.lock().unwrap().push((rgba[0], pts));
                    Ok(())
                }
            }
        }
        fn audio(&mut self, _: &[f32], _: Duration) -> Result<(), String> {
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<(), String> {
            self.log.finished.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// R04-26: a write error raises WriteFailed AT ONCE (once), nothing more is written, the
    /// file is still closed, and stopping says it did not end cleanly.
    #[test]
    fn r04_26_a_write_error_raises_a_fault_and_stop_reports_it() {
        let log = Arc::new(Log::default());
        let sink = Box::new(Breaking {
            ok: 3,
            seen: 0,
            log: log.clone(),
        });
        let s = Session::start(
            cfg(u64::MAX),
            Some(Box::new(move || Box::new(fake(1)))),
            vec![],
            sink,
        );
        thread::sleep(Duration::from_millis(250));
        assert_eq!(s.fault(), Some(Fault::WriteFailed));
        thread::sleep(Duration::from_millis(100));
        assert_eq!(s.fault(), None, "said once");
        assert_eq!(
            log.frames.lock().unwrap().len(),
            3,
            "nothing written after the error"
        );
        assert!(s.stop().is_err(), "the end says it failed");
        assert!(
            log.finished.load(Ordering::SeqCst),
            "the file is closed all the same"
        );
    }

    /// R04-27: a frame of another size than the encoder was opened for is a fault, never fed to it.
    #[test]
    fn r04_27_a_frame_of_the_wrong_size_ends_the_capture() {
        let mut c = cfg(u64::MAX);
        c.frame_bytes = Some(32); // the fake's frames are 16 bytes
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let s = Session::start(c, Some(Box::new(move || Box::new(fake(1)))), vec![], sink);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(s.fault(), Some(Fault::StreamEnded));
        assert!(log.frames.lock().unwrap().is_empty());
        s.stop().unwrap();
    }

    /// R04-28: Stop while paused returns at once (nothing waits for a resume).
    #[test]
    fn r04_28_stop_while_paused_does_not_wait() {
        let (s, log) = run(cfg(u64::MAX), fake(1), Duration::ZERO, vec![]);
        thread::sleep(Duration::from_millis(100));
        s.pause();
        thread::sleep(Duration::from_millis(50));
        let t = Instant::now();
        s.stop().unwrap();
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(log.finished.load(Ordering::SeqCst));
    }

    /// A source of one still picture, `w` x `h`.
    struct Still(Vec<u8>);
    impl FrameSource for Still {
        fn next(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
            thread::sleep(wait.min(Duration::from_millis(2)));
            Ok(Some(self.0.clone()))
        }
        fn fast(&self) -> bool {
            true
        }
    }

    fn zoned(
        zones: &super::super::zones::Zones,
        window: Arc<Mutex<WinRect>>,
    ) -> (Session, Arc<Log>, Vec<u8>) {
        let (w, h) = (200u32, 120u32);
        let picture = super::super::zones::tests::busy(w, h);
        let mut c = cfg(u64::MAX);
        c.zones = Some(Hook {
            zones: zones.clone(),
            origin: (0, 0),
            size: (w, h),
            seed: 3,
            lookup: Box::new(move |_| Some(*window.lock().unwrap())),
            cache: Default::default(),
        });
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let src = Still(picture.clone());
        let s = Session::start(c, Some(Box::new(move || Box::new(src))), vec![], sink);
        (s, log, picture)
    }

    fn differs(a: &[u8], b: &[u8], w: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                n += usize::from(a[i..i + 3] != b[i..i + 3]);
            }
        }
        n
    }

    /// R06-5: the encoder is given hidden frames: inside the zone replaced, outside identical;
    /// a zone added while paused hides every frame written after the resume (the marker: the
    /// pictures written before the pause were all queued before it).
    #[test]
    fn r06_5_the_sink_gets_blurred_frames_and_a_zone_added_in_a_pause_applies_at_once() {
        use super::super::zones::Zone;
        let zones = super::super::zones::Zones::default();
        zones.add(Zone::Area((20, 20, 60, 40)));
        let (s, log, raw) = zoned(&zones, Arc::new(Mutex::new((0, 0, 0, 0))));
        thread::sleep(Duration::from_millis(150));
        s.pause();
        thread::sleep(Duration::from_millis(80));
        let marker = log.pictures.lock().unwrap().len();
        assert!(marker > 3, "frames came before the pause");
        zones.add(Zone::Area((120, 70, 50, 30)));
        s.resume();
        thread::sleep(Duration::from_millis(150));
        s.stop().unwrap();
        let pics = log.pictures.lock().unwrap();
        assert!(pics.len() > marker + 3, "frames came after the resume");
        let two = |p: &Vec<u8>| differs(p, &raw, 200, (120, 70, 170, 100));
        for p in pics.iter() {
            assert!(
                differs(p, &raw, 200, (20, 20, 80, 60)) > 2000,
                "zone 1 hidden"
            );
            assert_eq!(
                differs(p, &raw, 200, (0, 112, 200, 120)),
                0,
                "outside identical"
            );
        }
        assert!(
            pics[marker..].iter().all(|p| two(p) > 1000),
            "zone 2 hides every frame written after the resume"
        );
    }

    /// R06-6: a followed window's zone moves with it, frame by frame (the old place is whole again).
    #[test]
    fn r06_6_a_followed_window_zone_moves_with_the_window() {
        use super::super::zones::Zone;
        let zones = super::super::zones::Zones::default();
        zones.add(Zone::app(1, (10, 10, 50, 40)));
        let window = Arc::new(Mutex::new((10, 10, 50, 40)));
        let (s, log, raw) = zoned(&zones, window.clone());
        thread::sleep(Duration::from_millis(120));
        *window.lock().unwrap() = (110, 60, 50, 40);
        thread::sleep(Duration::from_millis(150));
        s.stop().unwrap();
        let pics = log.pictures.lock().unwrap();
        let last = pics.last().unwrap();
        assert!(
            differs(last, &raw, 200, (110, 60, 160, 100)) > 1000,
            "hidden at the new place"
        );
        assert_eq!(
            differs(last, &raw, 200, (0, 0, 80, 60)),
            0,
            "whole at the old place"
        );
        assert!(
            differs(&pics[0], &raw, 200, (10, 10, 60, 50)) > 1000,
            "hidden at the old place first"
        );
    }

    /// R06-19: a capture that cannot keep up (a big zone) loses frames, and they are counted in
    /// `dropped` (not only the ones a full queue turns away).
    #[test]
    fn r06_19_frames_lost_to_falling_behind_are_counted() {
        struct Slow;
        impl FrameSource for Slow {
            fn next(&mut self, _: Duration) -> Result<Option<Vec<u8>>, String> {
                thread::sleep(Duration::from_millis(90));
                Ok(Some(vec![1; 16]))
            }
            fn fast(&self) -> bool {
                true
            }
        }
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        // 50 fps asks for a frame every 20 ms: each 90 ms grab loses about four
        let s = Session::start(
            cfg(u64::MAX),
            Some(Box::new(|| Box::new(Slow))),
            vec![],
            sink,
        );
        thread::sleep(Duration::from_millis(500));
        let stats = s.stop().unwrap();
        assert!(stats.dropped >= 10, "{stats:?}");
        assert!(stats.frames >= 3);
    }

    /// R04C-1: replacing the live microphone switches the samples written to the sink from the old
    /// device to the new one without stopping the recording.
    #[test]
    fn r04c_1_replace_mic_switches_live_audio_input() {
        struct ConstIn(f32);
        impl AudioIn for ConstIn {
            fn read(&mut self, _: Duration) -> Result<Option<(Vec<f32>, Instant)>, String> {
                thread::sleep(Duration::from_millis(20));
                Ok(Some((vec![self.0; 960 * 2], Instant::now())))
            }
        }
        struct PcmSink(Arc<Mutex<Vec<f32>>>);
        impl Sink for PcmSink {
            fn video(&mut self, _: &[u8], _: Duration) -> Result<(), String> {
                Ok(())
            }
            fn audio(&mut self, pcm: &[f32], _: Duration) -> Result<(), String> {
                self.0.lock().unwrap().extend_from_slice(pcm);
                Ok(())
            }
            fn pause(&mut self, _: bool) {}
            fn finish(self: Box<Self>) -> Result<(), String> {
                Ok(())
            }
        }
        let samples = Arc::new(Mutex::new(Vec::new()));
        let sink = Box::new(PcmSink(samples.clone()));
        let s = Session::start(
            cfg(u64::MAX),
            None,
            vec![(Chan::Mic, Box::new(ConstIn(0.15)))],
            sink,
        );
        thread::sleep(Duration::from_millis(180));
        s.replace_mic(Box::new(ConstIn(0.75)));
        thread::sleep(Duration::from_millis(220));
        s.stop().unwrap();
        let pcm = samples.lock().unwrap();
        let has_old = pcm.iter().any(|&v| (v - 0.15).abs() < 0.02);
        let has_new = pcm.iter().any(|&v| (v - 0.75).abs() < 0.02);
        assert!(has_old, "expected initial mic samples (~0.15)");
        assert!(has_new, "expected switched mic samples (~0.75)");
    }

    /// R04C-2: continuous all-zero packets from an unmuted microphone trigger `mic_silent()`,
    /// muted microphones never trigger it, and switching to a live mic clears it.
    #[test]
    fn r04c_2_silent_mic_is_detected_and_clears_when_sound_arrives() {
        struct ConstIn(f32);
        impl AudioIn for ConstIn {
            fn read(&mut self, _: Duration) -> Result<Option<(Vec<f32>, Instant)>, String> {
                thread::sleep(Duration::from_millis(20));
                Ok(Some((vec![self.0; 960 * 2], Instant::now())))
            }
        }
        let log = Arc::new(Log::default());
        let sink = Box::new(FakeSink {
            log: log.clone(),
            slow: Duration::ZERO,
        });
        let mut c = cfg(u64::MAX);
        c.mic_silent_frames = 4800; // 100 ms
        c.mic_muted = true;
        let s = Session::start(c, None, vec![(Chan::Mic, Box::new(ConstIn(0.0)))], sink);
        thread::sleep(Duration::from_millis(160));
        assert!(!s.mic_silent(), "muted mic must not raise silent notice");
        s.mute_mic(false);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !s.mic_silent() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            s.mic_silent(),
            "unmuted all-zero mic must raise silent notice"
        );
        s.replace_mic(Box::new(ConstIn(0.4)));
        thread::sleep(Duration::from_millis(120));
        assert!(!s.mic_silent(), "live audio clears silent notice");
        s.stop().unwrap();
    }
}
