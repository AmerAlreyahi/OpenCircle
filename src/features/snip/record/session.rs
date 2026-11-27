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
}

enum Msg {
    Audio(Chan, Vec<f32>, Instant),
    Pause(bool),
    Stop,
}

struct Frame(Arc<Vec<u8>>, Duration);

pub struct Session {
    shared: Arc<Shared>,
    origin: Instant,
    clock: Arc<Mutex<Clock>>,
    control: Sender<Msg>,
    faults: Receiver<Fault>,
    threads: Vec<JoinHandle<()>>,
    sink: JoinHandle<Result<(), String>>,
}

impl Session {
    /// Starts at once: `video` none = audio only. `inputs` are the sound sources in use.
    pub fn start(
        cfg: Config,
        video: Option<OpenSource>,
        inputs: Vec<(Chan, Box<dyn AudioIn>)>,
        sink: Box<dyn Sink>,
    ) -> Session {
        let shared = Arc::new(Shared::default());
        let origin = Instant::now();
        let clock = Arc::new(Mutex::new(Clock::start(Duration::ZERO)));
        let (control, control_rx) = mpsc::channel();
        let (fault_tx, faults) = mpsc::channel();
        let (frames_tx, frames_rx) = mpsc::sync_channel::<Frame>(cfg.queue.max(1));
        let mut threads = Vec::new();
        if let Some(open) = video {
            let (sh, cl, ft) = (shared.clone(), clock.clone(), fault_tx.clone());
            let (fps, bytes) = (cfg.fps, cfg.frame_bytes);
            // (opened on its own thread: a desktop's permission dialog must not stall the UI)
            threads.push(thread::spawn(move || {
                capture(open(), (fps, bytes), origin, &sh, &cl, frames_tx, &ft)
            }));
        }
        let (has_mic, has_sound) = (
            inputs.iter().any(|(c, _)| *c == Chan::Mic),
            inputs.iter().any(|(c, _)| *c == Chan::Sound),
        );
        for (chan, input) in inputs {
            let (sh, tx, ft) = (shared.clone(), control.clone(), fault_tx.clone());
            threads.push(thread::spawn(move || listen(chan, input, &sh, tx, &ft)));
        }
        let sh = shared.clone();
        let sink = thread::spawn(move || {
            write(
                cfg,
                sink,
                origin,
                &sh,
                (has_mic, has_sound),
                frames_rx,
                control_rx,
                fault_tx,
            )
        });
        Session {
            shared,
            origin,
            clock,
            control,
            faults,
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

/// The capture thread: a frame every 1/fps, the newest one again while nothing new came.
fn capture(
    mut src: Box<dyn FrameSource>,
    (fps, frame_bytes): (u32, Option<usize>),
    origin: Instant,
    shared: &Shared,
    clock: &Mutex<Clock>,
    queue: SyncSender<Frame>,
    faults: &Sender<Fault>,
) {
    let period = Duration::from_secs(1) / fps;
    let mut next = Instant::now();
    let mut last: Option<Arc<Vec<u8>>> = None;
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
            next = now + period; // fell behind: no burst to catch up
        }
        if shared.paused.load(Ordering::SeqCst) {
            continue;
        }
        let Some(frame) = &last else { continue };
        let pts = clock.lock().unwrap().elapsed(now.duration_since(origin));
        match queue.try_send(Frame(frame.clone(), pts)) {
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
    tx: Sender<Msg>,
    faults: &Sender<Fault>,
) {
    while !shared.stop.load(Ordering::SeqCst) {
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
    let mut mixer = new_mixer();
    let mut mixed = Vec::new();
    let mut last_check = Instant::now();
    let mut raised = false;
    let mut failed: Option<String> = None;
    let audio = |sink: &mut Box<dyn Sink>, mixed: &mut Vec<f32>, mixer: &mut Mixer, dead: bool| {
        mixer.set_mic_muted(shared.mic_muted.load(Ordering::Relaxed));
        mixer.set_system_muted(shared.sound_muted.load(Ordering::Relaxed));
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
                Msg::Audio(Chan::Mic, s, at) => mixer.push_mic(&s, at),
                Msg::Audio(Chan::Sound, s, at) => mixer.push_system(&s, at),
                Msg::Pause(true) => sink.pause(true),
                Msg::Pause(false) => {
                    // the pause is not in the sound: a mixer that starts again from now
                    mixer = new_mixer();
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
        pauses: Mutex<Vec<bool>>,
        finished: AtomicBool,
    }

    struct FakeSink {
        log: Arc<Log>,
        slow: Duration,
    }

    impl Sink for FakeSink {
        fn video(&mut self, rgba: &[u8], pts: Duration) -> Result<(), String> {
            thread::sleep(self.slow);
            self.log.frames.lock().unwrap().push((rgba[0], pts));
            Ok(())
        }
        fn audio(&mut self, pcm: &[f32], _pts: Duration) -> Result<(), String> {
            self.log
                .audio_frames
                .fetch_add(pcm.len() / 2, Ordering::SeqCst);
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
        assert_eq!(stats.dropped, 0);
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
}
