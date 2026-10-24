//! The helper process (`opencircle ocr-worker`) from the app's side. One child
//! answers many requests and keeps its models loaded between them, for as long
//! as something [`hold`]s it (live mode on, an overlay open); the last hold
//! dropped ends it, so all its memory goes back. Without a hold each call gets
//! its own child (spawn, one request, kill): the resident app never keeps ORT.
//!
//! Wire format, both ways: u32 LE length of the rest, u64 LE request id, then
//! the payload. One request at a time; its reply carries the same id.
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use super::OcrError;
use super::worker::WORKER_ARG;

/// Largest request: a 16k x 16k RGBA image plus its header.
pub(super) const MAX_REQUEST: usize = 16384 * 16384 * 4 + 64;
/// Largest reply (the JSON of a screen's lines or translations).
pub(super) const MAX_REPLY: usize = 16 << 20;
/// Per request. Generous: a cold start (ORT + models from disk) plus a
/// text-dense 4K screen.
const TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn write_frame(w: &mut impl Write, id: u64, parts: &[&[u8]]) -> io::Result<()> {
    let len = 8 + parts.iter().map(|p| p.len()).sum::<usize>();
    let len = u32::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "request too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&id.to_le_bytes())?;
    for part in parts {
        w.write_all(part)?;
    }
    w.flush()
}

/// The next frame (id, payload); `Ok(None)` = the stream ended between frames.
/// A frame over `max` bytes, or cut short, is an error.
pub(super) fn read_frame(r: &mut impl Read, max: usize) -> io::Result<Option<(u64, Vec<u8>)>> {
    let mut head = Vec::with_capacity(12);
    r.by_ref().take(12).read_to_end(&mut head)?;
    if head.is_empty() {
        return Ok(None);
    }
    let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
    if head.len() < 12 {
        return Err(bad("frame cut short".into()));
    }
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if !(8..=max).contains(&len) {
        return Err(bad(format!("frame of {len} bytes")));
    }
    let id = u64::from_le_bytes(head[4..12].try_into().expect("8 bytes"));
    let mut payload = vec![0; len - 8];
    r.read_exact(&mut payload)?;
    Ok(Some((id, payload)))
}

/// One helper child, started on the first call and again after a failure.
pub(crate) struct Session {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    helper: Option<Helper>,
    last_id: u64,
    /// Failed requests in a row, and when the next child may start: a child
    /// that keeps dying is never restarted in a tight loop.
    failures: u32,
    retry_at: Option<Instant>,
}

/// After the second failure in a row the next child waits this long,
/// doubling per failure up to `BACKOFF_MAX`.
const BACKOFF: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

impl Session {
    pub(crate) fn new(program: &Path, args: &[&str], timeout: Duration) -> Session {
        Session {
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            timeout,
            helper: None,
            last_id: 0,
            failures: 0,
            retry_at: None,
        }
    }

    /// The app's helper: `<exe> ocr-worker`.
    pub(crate) fn worker(exe: &Path) -> Session {
        Session::new(exe, &[WORKER_ARG], TIMEOUT)
    }

    /// One request, its reply's payload. A child that died, hangs past the
    /// timeout or answers out of turn is killed and reaped (the error says
    /// which); the next call starts a new one.
    pub(crate) fn call(&mut self, payload: &[&[u8]]) -> Result<Vec<u8>, OcrError> {
        if self.helper.as_mut().is_some_and(|h| !h.alive()) {
            self.helper = None; // died while idle
        }
        if self.helper.is_none()
            && let Some(at) = self.retry_at
            && let Some(wait) = at.checked_duration_since(Instant::now())
        {
            return Err(OcrError::Engine(format!(
                "the helper failed {} times in a row; trying again in {} s",
                self.failures,
                wait.as_secs() + 1
            )));
        }
        let reply = self.request(payload);
        if reply.is_ok() {
            (self.failures, self.retry_at) = (0, None);
        } else {
            self.helper = None;
            self.failures += 1;
            if self.failures >= 2 {
                let wait = BACKOFF.saturating_mul(1 << (self.failures - 2).min(5));
                self.retry_at = Some(Instant::now() + wait.min(BACKOFF_MAX));
            }
        }
        reply
    }

    fn request(&mut self, payload: &[&[u8]]) -> Result<Vec<u8>, OcrError> {
        let helper = match &mut self.helper {
            Some(h) => h,
            None => self
                .helper
                .insert(Helper::spawn(&self.program, &self.args)?),
        };
        self.last_id += 1;
        helper.request(self.last_id, payload, self.timeout)
    }

    /// The running child's process id.
    #[cfg(test)]
    pub(crate) fn pid(&self) -> Option<u32> {
        self.helper.as_ref().map(|h| h.child.id())
    }
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Helper {
    fn spawn(program: &Path, args: &[String]) -> Result<Helper, OcrError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => Ok(Helper {
                child,
                stdin,
                stdout,
            }),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err(OcrError::Engine("helper has no pipes".into()))
            }
        }
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Writes and reads on two threads, so a hung child can't block us past
    /// `timeout`: anything but a whole request and its own reply kills the
    /// child, which closes the pipes and ends both threads.
    fn request(
        &mut self,
        id: u64,
        payload: &[&[u8]],
        timeout: Duration,
    ) -> Result<Vec<u8>, OcrError> {
        let Helper {
            child,
            stdin,
            stdout,
        } = self;
        let deadline = Instant::now() + timeout;
        let ((wrote_tx, wrote_rx), (reply_tx, reply_rx)) = (mpsc::channel(), mpsc::channel());
        let (wrote, got) = std::thread::scope(|s| {
            s.spawn(move || wrote_tx.send(write_frame(stdin, id, payload)));
            s.spawn(move || reply_tx.send(read_frame(stdout, MAX_REPLY)));
            let got = reply_rx.recv_timeout(timeout);
            let left = deadline.saturating_duration_since(Instant::now());
            let wrote = wrote_rx.recv_timeout(left);
            if !matches!((&wrote, &got), (Ok(Ok(())), Ok(Ok(Some((n, _))))) if *n == id) {
                let _ = child.kill();
            }
            (wrote, got)
        });
        let engine = |m: String| Err(OcrError::Engine(m));
        match got {
            Ok(Ok(Some((n, reply)))) if n == id && matches!(wrote, Ok(Ok(()))) => Ok(reply),
            Ok(Ok(Some((n, _)))) if n != id => {
                engine(format!("helper answered request {n}, not {id}"))
            }
            Ok(Ok(Some(_))) => engine("helper answered before reading the request".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                engine(format!("helper timed out after {timeout:?}"))
            }
            _ => match reap(child) {
                Some(status) => engine(format!("helper stopped ({status})")),
                None => engine("helper's reply was lost".into()),
            },
        }
    }
}

/// The exit status of a child that just closed its pipes: it gets a moment to
/// exit by itself (its own status), then is killed.
fn reap(child: &mut Child) -> Option<ExitStatus> {
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    child.wait().ok()
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Its memory goes back at once; wait() so no zombie is left.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static HOLDS: AtomicUsize = AtomicUsize::new(0);
static SHARED: Mutex<Option<Session>> = Mutex::new(None);

/// While any `Hold` lives, helper calls share one warm child (started by the
/// first call, not by the hold). Dropping the last one ends that child.
#[must_use]
pub struct Hold(());

pub fn hold() -> Hold {
    HOLDS.fetch_add(1, Ordering::SeqCst);
    Hold(())
}

impl Drop for Hold {
    fn drop(&mut self) {
        if HOLDS.fetch_sub(1, Ordering::SeqCst) == 1 {
            // Off the caller's (UI) thread: a running request holds the lock.
            std::thread::spawn(|| {
                let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
                if HOLDS.load(Ordering::SeqCst) == 0 {
                    *shared = None;
                }
            });
        }
    }
}

/// One request to the app's helper: the shared warm child while held (one
/// request at a time; others wait), else a child for this call only.
pub(super) fn call(payload: &[&[u8]]) -> Result<Vec<u8>, OcrError> {
    let exe = std::env::current_exe()?;
    if HOLDS.load(Ordering::SeqCst) == 0 {
        return Session::worker(&exe).call(payload);
    }
    let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    let reply = (shared.get_or_insert_with(|| Session::worker(&exe))).call(payload);
    if HOLDS.load(Ordering::SeqCst) == 0 {
        *shared = None; // the last hold went during the request
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Arc;

    #[test]
    fn frames_round_trip_and_stay_strict() {
        let mut buf = Vec::new();
        write_frame(&mut buf, 7, &[b"ab", b"", b"cde"]).unwrap();
        write_frame(&mut buf, u64::MAX, &[]).unwrap();
        let mut r = Cursor::new(buf.clone());
        assert_eq!(
            read_frame(&mut r, 64).unwrap(),
            Some((7, b"abcde".to_vec()))
        );
        assert_eq!(read_frame(&mut r, 64).unwrap(), Some((u64::MAX, vec![])));
        assert_eq!(read_frame(&mut r, 64).unwrap(), None, "clean end");

        let cut = |b: &[u8]| read_frame(&mut Cursor::new(b.to_vec()), 64);
        assert!(cut(&buf[..5]).is_err(), "header cut short");
        assert!(cut(&buf[..15]).is_err(), "payload cut short");
        assert!(
            read_frame(&mut Cursor::new(buf.clone()), 12).is_err(),
            "over max"
        );
        let tiny = [7u32.to_le_bytes().as_slice(), &[0; 8]].concat();
        assert!(cut(&tiny).is_err(), "shorter than its id");
    }

    #[cfg(unix)]
    fn sh(script: &str, timeout_ms: u64) -> Session {
        Session::new(
            Path::new("sh"),
            &["-c", script],
            Duration::from_millis(timeout_ms),
        )
    }

    /// Gone and reaped: not even a zombie (`kill(pid, 0)` finds zombies).
    #[cfg(unix)]
    fn gone(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid as i32, 0) == -1 }
    }

    /// `cat` echoes each request frame back: a reply with the same id.
    #[cfg(unix)]
    #[test]
    fn one_child_answers_many_requests_in_order() {
        let mut s = sh("exec cat", 5000);
        assert_eq!(s.call(&[b"one"]).unwrap(), b"one");
        let pid = s.pid().unwrap();
        let big = vec![9; 3 << 20]; // past any pipe buffer
        assert_eq!(s.call(&[b"x", &big]).unwrap().len(), big.len() + 1);
        assert_eq!(s.call(&[b"two"]).unwrap(), b"two");
        assert_eq!(s.pid(), Some(pid), "the same child");
    }

    #[cfg(unix)]
    #[test]
    fn requests_from_many_threads_are_serialized() {
        let s = Arc::new(Mutex::new(sh("exec cat", 5000)));
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for n in 0..20 {
                        let msg = format!("{t}:{n}");
                        let reply = s.lock().unwrap().call(&[msg.as_bytes()]).unwrap();
                        assert_eq!(reply, msg.as_bytes());
                    }
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        assert_eq!(s.lock().unwrap().last_id, 160);
    }

    #[cfg(unix)]
    #[test]
    fn a_reply_with_another_id_kills_the_child() {
        // Reads the 13-byte request, answers id 99 (octal 143) with "x".
        let mut s = sh(
            r"head -c 13 >/dev/null; printf '\011\000\000\000\143\000\000\000\000\000\000\000x'; exec sleep 30",
            5000,
        );
        let err = s.call(&[b"?"]).unwrap_err().to_string();
        assert!(err.contains("answered request 99, not 1"), "{err}");
        assert!(s.pid().is_none());
    }

    #[cfg(unix)]
    fn marker(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("oc-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The first child hangs; it is killed at the timeout, and the next call
    /// starts a fresh one (an echo, since the marker now exists).
    #[cfg(unix)]
    #[test]
    fn a_hung_child_is_killed_and_the_next_call_respawns() {
        let m = marker("hang");
        let mut s = sh(
            &format!(
                "[ -e {0} ] && exec cat; touch {0}; exec sleep 30",
                m.display()
            ),
            300,
        );
        let t = Instant::now();
        let first = s.call(&[b"a"]);
        let err = first.unwrap_err().to_string();
        assert!(err.contains("timed out"), "{err}");
        assert!(t.elapsed() < Duration::from_secs(3));
        assert_eq!(s.call(&[b"b"]).unwrap(), b"b");
        let _ = std::fs::remove_file(m);
    }

    #[cfg(unix)]
    #[test]
    fn a_crashed_child_is_reported_and_replaced() {
        let m = marker("crash");
        let mut s = sh(
            &format!("[ -e {0} ] && exec cat; touch {0}; exit 3", m.display()),
            5000,
        );
        let err = s.call(&[b"a"]).unwrap_err().to_string();
        assert!(err.contains("helper stopped") && err.contains('3'), "{err}");
        assert_eq!(s.call(&[b"b"]).unwrap(), b"b");
        let pid = s.pid().unwrap();
        // It dies while idle: the next call notices and starts another.
        // SAFETY: signals our own child.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(s.call(&[b"c"]).unwrap(), b"c");
        assert_ne!(s.pid(), Some(pid));
        let _ = std::fs::remove_file(m);
    }

    /// A child that dies at once, every time (the Windows helper without
    /// ONNX Runtime did): retried once at once, then only after a growing
    /// wait, never in a tight loop.
    #[cfg(unix)]
    #[test]
    fn a_child_that_keeps_crashing_backs_off() {
        let m = marker("backoff");
        let mut s = sh(&format!("echo x >> {}; exit 3", m.display()), 5000);
        let starts = || std::fs::read_to_string(&m).map_or(0, |t| t.lines().count());
        for _ in 0..2 {
            assert!(
                s.call(&[b"a"])
                    .unwrap_err()
                    .to_string()
                    .contains("helper stopped")
            );
        }
        for _ in 0..20 {
            let err = s.call(&[b"a"]).unwrap_err().to_string();
            assert!(
                err.contains("failed 2 times in a row; trying again in"),
                "{err}"
            );
        }
        assert_eq!(starts(), 2, "no child started while waiting");
        std::thread::sleep(BACKOFF + Duration::from_millis(100));
        assert!(s.call(&[b"a"]).is_err());
        assert_eq!(starts(), 3, "one more after the wait");
        let err = s.call(&[b"a"]).unwrap_err().to_string();
        assert!(err.contains("failed 3 times"), "the wait grows: {err}");
        let _ = std::fs::remove_file(m);
    }

    #[cfg(unix)]
    #[test]
    fn dropping_the_session_reaps_its_child() {
        let mut s = sh("exec cat", 5000);
        s.call(&[b"a"]).unwrap();
        let pid = s.pid().unwrap();
        assert!(!gone(pid));
        drop(s);
        assert!(gone(pid), "killed and waited for");
    }

    #[cfg(target_os = "linux")]
    type Call<'a> = dyn Fn(&[&[u8]]) -> Result<Vec<u8>, OcrError> + 'a;

    #[cfg(target_os = "linux")]
    struct Via<F>(F);

    #[cfg(target_os = "linux")]
    impl<F: Fn(&[&[u8]]) -> Result<Vec<u8>, OcrError>> crate::translate::pipeline::Deps for Via<F> {
        fn recognize(
            &self,
            rgba: &[u8],
            w: u32,
            h: u32,
        ) -> Result<Vec<super::super::TextBlock>, OcrError> {
            super::super::worker::recognize_via(&self.0, rgba, w, h, super::super::Read::One(0))
        }
        fn translate(
            &self,
            texts: &[String],
            target: &str,
        ) -> Result<Vec<String>, crate::ai::AiError> {
            let pair = crate::mt::pair("en", target).expect("an offline pair");
            super::super::worker::translate_via(&self.0, texts, pair)
                .map_err(|e| crate::ai::AiError::Offline(e.to_string()))
        }
    }

    /// A fresh live session: the first frame, then `changed`; both timed.
    #[cfg(target_os = "linux")]
    fn live_steps(
        deps: &impl crate::translate::pipeline::Deps,
        frame: &[u8],
        changed: &[u8],
    ) -> String {
        let mut live = crate::translate::live::Live::new();
        let cache = crate::translate::pipeline::Cache::new(512);
        let t = Instant::now();
        live.step(frame, 1920, 1080, "ar", &cache, deps).unwrap();
        let first = t.elapsed();
        let t = Instant::now();
        let s = live.step(changed, 1920, 1080, "ar", &cache, deps);
        let (step, s) = (t.elapsed(), s.unwrap().expect("two new lines"));
        format!(
            "first frame {first:.0?}; step with 2 new lines {step:.0?} (ocr {:.0?}, translate {:.0?}, render {:.0?})",
            s.ocr, s.translate, s.render
        )
    }

    /// Cold vs warm with the real models (`~/.local/share/OpenCircle`) and the
    /// real `opencircle ocr-worker`, so build it first:
    /// `cargo build --release && cargo test --release ocr::session::tests::warm -- --ignored --nocapture`
    /// Tap a word = OCR of a 720x160 crop; a live step = the 1080p game frame,
    /// then the same frame with two new lines (OCR of what changed, offline MT
    /// of the new lines, render).
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn warm_helper_with_real_models() {
        let mb = |pid: u32, key: &str| {
            let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
            let line = s.lines().find(|l| l.starts_with(key)).unwrap();
            line.split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u64>()
                .unwrap()
                / 1024
        };
        let exe = std::env::current_exe().unwrap();
        let exe = exe.parent().unwrap().parent().unwrap().join("opencircle");
        let (frame, _) = crate::translate::tests::game_frame();
        let w = 1920;
        // Two new lines: the OCR fixture ("Hello OpenCircle", "Read 42 words
        // here") on an empty part of the frame.
        let png = png::Decoder::new(Cursor::new(include_bytes!("fixture.png")));
        let mut reader = png::Decoder::read_info(png).unwrap();
        let mut tile = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut tile).unwrap();
        let (tw, th) = (info.width as usize, info.height as usize);
        let mut changed = frame.clone();
        for y in 0..th {
            let at = ((300 + y) * w + 700) * 4;
            changed[at..at + tw * 4].copy_from_slice(&tile[y * tw * 4..][..tw * 4]);
        }
        // Tap a word: the region around the dialogue's second line.
        let tap: Vec<u8> = (850..850 + 160)
            .flat_map(|y| frame[(y * w + 200) * 4..][..720 * 4].to_vec())
            .collect();
        let taps = |call: &Call, n| {
            (0..n)
                .map(|_| {
                    let t = Instant::now();
                    let lines = super::super::worker::recognize_via(
                        call,
                        &tap,
                        720,
                        160,
                        super::super::Read::One(0),
                    )
                    .unwrap();
                    assert!(!lines.is_empty());
                    t.elapsed()
                })
                .collect::<Vec<_>>()
        };

        let one_shot = |p: &[&[u8]]| Session::worker(&exe).call(p);
        println!("BEFORE (a child per call)");
        println!("  tap a word: {:.0?}", taps(&one_shot, 3));
        println!("  live: {}", live_steps(&Via(one_shot), &frame, &changed));

        let warm = Mutex::new(Session::worker(&exe));
        let call = |p: &[&[u8]]| warm.lock().unwrap().call(p);
        println!("AFTER (one warm child)");
        println!("  tap a word (the first is cold): {:.0?}", taps(&call, 6));
        let pid = warm.lock().unwrap().pid().unwrap();
        let rss = |what: &str| {
            let (now, peak) = (mb(pid, "VmRSS"), mb(pid, "VmHWM"));
            println!("  helper RSS {what}: {now} MB (peak {peak} MB)");
        };
        rss("after the taps");
        let t = Instant::now();
        super::super::worker::recognize_via(call, &frame, 1920, 1080, super::super::Read::One(0))
            .unwrap();
        println!("  1080p OCR, warm: {:.0?}", t.elapsed());
        rss("after a 1080p OCR");
        for run in ["MT cold", "all warm", "all warm"] {
            println!(
                "  live, {run}: {}",
                live_steps(&Via(call), &frame, &changed)
            );
            rss("after it");
        }
        std::thread::sleep(Duration::from_secs(3)); // past the helper's idle trim
        rss("idle 3 s");
        assert!(mb(pid, "VmRSS") < 400, "idle helper gave its memory back");
        println!(
            "  live, after idle: {}",
            live_steps(&Via(call), &frame, &changed)
        );
        rss("after it");
        assert_eq!(
            warm.lock().unwrap().pid(),
            Some(pid),
            "one child throughout"
        );
        drop(warm);
        assert!(gone(pid), "the helper is gone with its session");
        println!("  session dropped: helper {pid} gone");
    }
}
