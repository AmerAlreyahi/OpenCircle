//! Auto-update (owner decisions "Auto-update" and "Update experience", AGENTS.md).
//! With `[update] auto = true` a thread asks GitHub for the latest release a little
//! after start and then every 12 h (only the request: no id, no data; a User-Agent
//! and the ETag). A newer final release is downloaded with `SHA256SUMS.txt` and its
//! minisign signature and checked against `verify::RELEASE_PUBLIC_KEY`. A portable
//! exe or AppImage is then put in place at once (`prepare`: swap the file, run the
//! new one once with `--version`; the running program keeps going): the new
//! version is what starts next, whatever ends this run (quit, log-off, a reboot).
//! The UI (`Event::Ready`) tells the user and restarts into it on Restart now, or
//! after the session has been locked a minute (`restart`, `flow`). The new process
//! confirms the update at start (`confirm`: What's new) or rolls it back if it
//! cannot start (`roll_back`). A copy our setup installed (Program Files: the user
//! can't write it) gets the release's setup instead, run elevated and quiet only on
//! Restart now (`install`), and it starts the new version (#151). deb / rpm /
//! source builds can't replace themselves: `Event::Manual` ("Get it"). Failures are
//! logged, never shown, unless the user asked (Settings > Check now: `Event::Status`).

mod flow;
mod notes;
mod release;
pub use release::version;
mod swap;
pub(crate) mod verify;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

pub use flow::{Head, How, Lock, Moment, Pill, Say, Step, When, away, installs_at, status};
pub use notes::{
    Line as NoteLine, Notes, Style as NoteStyle, parse as parse_notes, plain as plain_note,
};
use release::{CheckError, Http, Memo};
pub use release::{Install, Staged};

use crate::{Trigger, Triggers};

/// GitHub's latest final release of the project (drafts and pre-releases never show here).
pub const LATEST_URL: &str = "https://api.github.com/repos/AmerAlreyahi/OpenCircle/releases/latest";

/// Hidden arguments: print the version and exit (the install's smoke test), and
/// "started by an update" (wait for the old process to exit first).
pub const VERSION_ARG: &str = "--version";
pub const RELAUNCH_ARG: &str = "updated";

/// The first check waits this long (never slows the first open), then every `EVERY`.
const FIRST_DELAY: Duration = Duration::from_secs(45);
const EVERY: Duration = Duration::from_secs(12 * 3600);

/// Test hook (not for users): `OPENCIRCLE_TEST_UPDATE_URL=<latest release JSON URL>`
/// (http allowed, e.g. a local fake release server); the first check comes after 3 s
/// and a development build (0.0.0) checks too. Downloads must still pass the signature.
pub fn test_url() -> Option<String> {
    std::env::var("OPENCIRCLE_TEST_UPDATE_URL")
        .ok()
        .filter(|u| !u.is_empty())
}

/// What a check found: the UI words it in its language, `Display` is the
/// English of the log (and the tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// GitHub asks to wait this many seconds.
    Wait(u64),
    Unreachable,
    /// A newer version the user installs from the release page.
    Out(String),
    /// A newer version, ready to install on restart.
    Ready(String),
    UpToDate(String),
    DevBuild,
    NoFolder,
    Unverified(String),
}

impl std::fmt::Display for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Answer::Wait(s) => write!(
                f,
                "GitHub asks to wait: try again in {} min",
                s.div_ceil(60)
            ),
            Answer::Unreachable => f.write_str("could not reach GitHub: try again later"),
            Answer::Out(v) => write!(f, "OpenCircle {v} is out: get it from the release page"),
            Answer::Ready(v) => write!(
                f,
                "OpenCircle {v} is ready: restart OpenCircle to install it"
            ),
            Answer::UpToDate(v) => write!(f, "OpenCircle {v} is up to date"),
            Answer::DevBuild => {
                f.write_str("this is a development build: it does not update itself")
            }
            Answer::NoFolder => f.write_str("no data folder: not installed"),
            Answer::Unverified(v) => {
                write!(f, "OpenCircle {v} could not be verified: not installed")
            }
        }
    }
}

/// What the update thread tells the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The result of a check the user asked for (Settings shows it).
    Status(Answer),
    /// Downloaded and verified (a swap install: already in place for the next
    /// start): the user is told, it installs at a `flow::Moment`.
    Ready(Staged),
    /// A newer release this install can't apply itself: show it, link the page.
    Manual {
        version: String,
        notes: String,
        url: String,
    },
}

/// Wakes the update thread for a check now (Settings > Check now).
static NOW: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

/// An update waiting for the user (Settings, the tray), from the update thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub how: How,
}

static OFFER: Mutex<Option<Offer>> = Mutex::new(None);
/// When GitHub last answered a check.
static CHECKED: Mutex<Option<SystemTime>> = Mutex::new(None);

/// The update waiting for the user, if any (the same until this process exits:
/// one update per run).
pub fn offer() -> Option<Offer> {
    OFFER.lock().unwrap().clone()
}

/// When GitHub last answered a check in this run.
pub fn checked() -> Option<SystemTime> {
    *CHECKED.lock().unwrap()
}

/// Starts the update thread (once, from `main`).
pub fn start(tx: Triggers) {
    let (now_tx, now_rx) = mpsc::channel();
    *NOW.lock().unwrap() = Some(now_tx);
    let spawned = std::thread::Builder::new()
        .name("update".into())
        .spawn(move || run(&now_rx, &tx));
    if let Err(e) = spawned {
        crate::log!("[opencircle] update: cannot start a thread: {e}");
    }
}

/// Settings > Check now: one check at once; its result comes back as `Event::Status`.
pub fn check_now() {
    if let Some(tx) = NOW.lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

fn run(now: &mpsc::Receiver<()>, tx: &Triggers) {
    remove_stale_downloads();
    let mut memo = Memo::default();
    let mut handled: Option<String> = None;
    let first = if test_url().is_some() {
        Duration::from_secs(3)
    } else {
        FIRST_DELAY
    };
    let mut asked = now.recv_timeout(first).is_ok();
    loop {
        if asked || crate::config::current().update.auto == Some(true) {
            let status = match Setup::this() {
                Some(setup) => check(&release::Net, &setup, &mut memo, &mut handled, tx, &prepare),
                None => Answer::DevBuild,
            };
            crate::log!("[opencircle] update: {status}");
            if asked {
                let _ = tx.unbounded_send(Trigger::Update(Event::Status(status)));
            }
        }
        asked = match now.recv_timeout(EVERY) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
    }
}

/// Where and what a check runs against.
struct Setup {
    url: String,
    current: &'static str,
    install: Install,
    key: &'static str,
}

impl Setup {
    /// This process; `None` for a development build (0.0.0) outside the test hook.
    fn this() -> Option<Setup> {
        let current = env!("CARGO_PKG_VERSION");
        let url = test_url();
        if current == "0.0.0" && url.is_none() {
            return None;
        }
        Some(Setup {
            key: key(url.is_some()),
            url: url.unwrap_or_else(|| LATEST_URL.into()),
            current,
            install: Install::detect(),
        })
    }
}

/// The key releases must be signed with. Only for the test hook (`hook`) may a build
/// trust a throwaway key instead, and only one given at BUILD time
/// (`OPENCIRCLE_TEST_RELEASE_KEY`, a VM test build's fake releases): release.yml never
/// sets it, so a published copy trusts `RELEASE_PUBLIC_KEY` alone, hook or not.
fn key(hook: bool) -> &'static str {
    match option_env!("OPENCIRCLE_TEST_RELEASE_KEY") {
        Some(test) if hook && !test.is_empty() => test,
        _ => verify::RELEASE_PUBLIC_KEY,
    }
}

/// One check; sends `Ready` / `Manual` for a newer release, once per run
/// (`handled`: the first update offered waits until this process ends; a newer
/// one is the next run's). A swap install is put in place first (`prepare`).
/// Returns the status line.
fn check(
    http: &dyn Http,
    setup: &Setup,
    memo: &mut Memo,
    handled: &mut Option<String>,
    tx: &Triggers,
    prepare: &dyn Fn(&Staged) -> Result<(), String>,
) -> Answer {
    let current = setup.current;
    let release = match release::latest(http, &setup.url, memo) {
        Ok(r) => r,
        Err(CheckError::Wait(s)) => {
            return Answer::Wait(s);
        }
        Err(CheckError::Failed(e)) => {
            crate::log!("[opencircle] update check failed: {}", release::scrub(&e));
            return Answer::Unreachable;
        }
    };
    *CHECKED.lock().unwrap() = Some(SystemTime::now());
    let install = &setup.install;
    if let Some(version) = handled.as_deref() {
        // kept (never downloaded again): the UI holds it
        return match How::of(install) {
            How::GetIt => Answer::Out(version.into()),
            _ => Answer::Ready(version.into()),
        };
    }
    if !release::is_newer(&release, current) {
        return Answer::UpToDate(current.into());
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    let offer = |handled: &mut Option<String>, how: How| {
        *handled = Some(version.clone());
        *OFFER.lock().unwrap() = Some(Offer {
            version: version.clone(),
            notes: release.body.clone().unwrap_or_default(),
            url: release.html_url.clone(),
            how,
        });
    };
    let manual = |handled: &mut Option<String>| {
        offer(handled, How::GetIt);
        let _ = tx.unbounded_send(Trigger::Update(Event::Manual {
            version: version.clone(),
            notes: release.body.clone().unwrap_or_default(),
            url: release.html_url.clone(),
        }));
        Answer::Out(version.clone())
    };
    if *install == Install::Manual {
        return manual(handled);
    }
    let Some(dir) = stage_dir() else {
        return Answer::NoFolder;
    };
    let staged = match release::stage(
        http,
        &release,
        install,
        std::env::consts::ARCH,
        &dir,
        setup.key,
    ) {
        Ok(staged) => staged,
        Err(e) if e == "no download for this system" => return manual(handled),
        Err(e) => {
            crate::log!(
                "[opencircle] update {version} refused: {}",
                release::scrub(&e)
            );
            return Answer::Unverified(version);
        }
    };
    if let Err(e) = prepare(&staged) {
        // can't replace itself (e.g. a folder the user can't write): the page instead
        crate::log!("[opencircle] update {version}: {e}");
        return manual(handled);
    }
    offer(handled, How::of(install));
    let _ = tx.unbounded_send(Trigger::Update(Event::Ready(staged)));
    Answer::Ready(version)
}

/// Where downloads are staged: `<local data dir>/opencircle/update`, this user's
/// own (never the shared temp dir), made fresh and private for each download.
fn stage_dir() -> Option<std::path::PathBuf> {
    crate::paths::update_stage()
}

/// A download left by an earlier run (only one instance runs, so none is in use).
fn remove_stale_downloads() {
    if let Some(dir) = stage_dir()
        && std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir())
    {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The update thread, right after a download is verified: a swap install's new
/// program is put in place now (`prepare_at`), so it is what starts next,
/// whatever ends this run. A setup's waits for Restart now (`restart`); deb /
/// rpm never get here. `Err` = everything is as before.
fn prepare(staged: &Staged) -> Result<(), String> {
    if !matches!(staged.install, Install::Exe(_) | Install::AppImage(_)) {
        return Ok(());
    }
    let marker = swap::Marker::path().ok_or("no data folder")?;
    let result = prepare_at(staged, &marker);
    let _ = std::fs::remove_dir_all(&staged.dir);
    result
}

/// Restarts into the waiting update at `moment` (`flow::installs_at` says
/// whether it may): a swap install starts its new program (already in place)
/// and the caller exits; a setup's copy runs the release's setup, elevated (one
/// Windows prompt), only on the button. `Ok` = the caller exits now; on `Err`
/// nothing changed (a declined prompt can be retried).
pub fn restart(staged: &Staged, moment: Moment) -> Result<(), String> {
    if !installs_at(&staged.install, moment) {
        return Err("this update installs only when asked".into());
    }
    let marker = swap::Marker::path().ok_or("no data folder")?;
    match &staged.install {
        // the setup runs from the staged folder: kept (a declined prompt can be
        // retried); the next start's `remove_stale_downloads` takes it
        Install::Setup(_) => run_setup(staged, &marker, elevate),
        // after the button, What's new; after a lock, its small pill
        _ => relaunch_at(&marker, moment != Moment::Button),
    }
}
/// What the setup is told: install quietly, then start the app (`setup::Silent`).
pub const SETUP_ARGS: &str = "/quiet /launch";

/// Runs the verified setup with `run` (elevated: one UAC prompt). The marker is
/// written first so the new version shows its notes; `Err` (the prompt declined,
/// the file changed) = nothing changed, the staged file stays for another try.
/// `Ok` = the caller exits now: the setup quits a still-running OpenCircle anyway
/// before Windows Installer replaces its files, and starts the new one after.
fn run_setup(
    staged: &Staged,
    marker_path: &Path,
    run: impl FnOnce(&Path, &str) -> std::io::Result<()>,
) -> Result<(), String> {
    let Install::Setup(exe) = &staged.install else {
        return Err("not installed by the setup".into());
    };
    // held open, writing and deleting shared with no one, until the setup has started:
    // only the verified bytes can run
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut open, 1); // FILE_SHARE_READ
    let file = open.open(&staged.file).map_err(|e| e.to_string())?;
    if verify::sha256(&file).ok() != Some(staged.sha256) {
        return Err("the file changed after it was verified".into());
    }
    let marker = swap::Marker {
        version: staged.version.clone(),
        target: exe.clone(),
        backup: PathBuf::new(), // nothing to roll back: Windows Installer does that
        notes: staged.notes.clone(),
        url: staged.url.clone(),
        quiet: false, // only ever on Restart now: What's new
    };
    marker.write(marker_path).map_err(|e| e.to_string())?;
    let hex: String = staged.sha256.iter().map(|b| format!("{b:02x}")).collect();
    let args = format!("{SETUP_ARGS} {}{hex}", crate::setup::SHA256_ARG);
    if let Err(e) = run(&staged.file, &args) {
        let _ = std::fs::remove_file(marker_path);
        return Err(format!("the setup did not start: {e}"));
    }
    drop(file);
    crate::log!(
        "[opencircle] update {}: the setup runs, restarting",
        staged.version
    );
    Ok(())
}

/// Starts `exe` with `args` as administrator (`ShellExecuteEx` "runas": the UAC
/// prompt). Returns once it started, or the prompt was declined (`ERROR_CANCELLED`).
#[cfg(windows)]
fn elevate(exe: &Path, args: &str) -> std::io::Result<()> {
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCWSTR, w};
    let (file, params) = (HSTRING::from(exe.as_os_str()), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to live through the call.
    unsafe { ShellExecuteExW(&mut info) }.map_err(std::io::Error::from)
}

#[cfg(not(windows))]
fn elevate(_: &Path, _: &str) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Swaps the file, runs the new one once and writes the marker (a quiet one:
/// a start that isn't Restart now shows the small "Updated" pill). The running
/// program keeps going. On `Err` everything is as before.
fn prepare_at(staged: &Staged, marker_path: &Path) -> Result<(), String> {
    let target = match &staged.install {
        Install::Exe(p) | Install::AppImage(p) => p,
        Install::Setup(_) => return Err("this install updates through its setup".into()),
        Install::Manual => return Err("this install updates by hand".into()),
    };
    let backup = swap::replace(target, &staged.file, &staged.sha256)
        .map_err(|e| format!("cannot replace {}: {e}", target.display()))?;
    let undo = |e: String| {
        let _ = swap::restore(target, &backup);
        e
    };
    smoke_test(target, &staged.version).map_err(undo)?;
    let marker = swap::Marker {
        version: staged.version.clone(),
        target: target.clone(),
        backup: backup.clone(),
        notes: staged.notes.clone(),
        url: staged.url.clone(),
        quiet: true,
    };
    marker.write(marker_path).map_err(|e| undo(e.to_string()))?;
    crate::log!(
        "[opencircle] update {}: in place, starts with the next run",
        staged.version
    );
    Ok(())
}

/// Starts the program the marker at `marker_path` waits for (`quiet`: its
/// small pill instead of What's new). `Ok` = the caller exits now.
fn relaunch_at(marker_path: &Path, quiet: bool) -> Result<(), String> {
    let mut marker = swap::Marker::read(marker_path).ok_or("no update is waiting")?;
    marker.quiet = quiet;
    marker.write(marker_path).map_err(|e| e.to_string())?;
    spawn(&marker.target, Some(RELAUNCH_ARG))
        .map_err(|e| format!("cannot start the new version: {e}"))?;
    crate::log!("[opencircle] update {}: restarting", marker.version);
    Ok(())
}

/// The new program runs and says it is `version`.
fn smoke_test(exe: &Path, version: &str) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.arg(VERSION_ARG)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    clean_env(&mut cmd);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("the new version does not run: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the new version hangs".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
    }
    match out.trim() {
        v if v == version => Ok(()),
        v => Err(format!("the new version says it is \"{v}\", not {version}")),
    }
}

/// An AppImage's own variables point into the old one's mount, gone once it exits:
/// the new program sets its own.
fn clean_env(cmd: &mut Command) {
    for var in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
        cmd.env_remove(var);
    }
    if let Some(appdir) = std::env::var_os("APPDIR").filter(|d| !d.is_empty()) {
        let appdir = appdir.to_string_lossy().into_owned();
        for (k, v) in std::env::vars_os() {
            if v.to_string_lossy().contains(&appdir) {
                cmd.env_remove(k);
            }
        }
    }
}

/// Starts `exe` (with `arg`) on its own: no stdio, own process group, not waited for.
fn spawn(exe: &Path, arg: Option<&str>) -> std::io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.args(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    clean_env(&mut cmd);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    cmd.spawn().map(drop)
}

/// Started by an update: waits until the old process has let go of the single-instance
/// socket (`running` = it still answers). `false` after 30 s: the update is rolled back.
pub fn wait_for_old(running: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while running() {
        if Instant::now() > deadline {
            crate::log!("[opencircle] update: the old version did not exit");
            roll_back();
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    true
}

/// An update this process started with (`confirm`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// Not after Restart now (a quit, a lock, a reboot): the small "Updated" pill,
    /// not What's new.
    pub quiet: bool,
}

/// At start: an update that is waiting to be confirmed is confirmed (the old
/// program goes). Another copy of OpenCircle (elsewhere on the disk) leaves it
/// for the program it is for.
pub fn confirm() -> Option<Installed> {
    let path = swap::Marker::path()?;
    let waiting = swap::Marker::read(&path)?;
    if !is_this_program(&waiting.target) {
        return None;
    }
    let marker = swap::confirm(&path, env!("CARGO_PKG_VERSION"))?;
    crate::log!("[opencircle] update {}: running", marker.version);
    // a second start soon after (the setup's own start, Windows restarting what
    // it closed) opens nothing (`just_updated`)
    let _ = std::fs::write(stamp(&path), marker.version.as_bytes());
    // The old process may still be exiting (Windows locks a running exe).
    let backup = marker.backup.clone();
    std::thread::spawn(move || {
        for _ in 0..30 {
            if !backup.exists() || std::fs::remove_file(&backup).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    Some(Installed {
        version: marker.version,
        notes: marker.notes,
        url: marker.url,
        quiet: marker.quiet,
    })
}

/// `target` is this process's program (an AppImage's is the image, not its mount).
fn is_this_program(target: &Path) -> bool {
    let this = match Install::detect() {
        Install::Exe(p) | Install::Setup(p) | Install::AppImage(p) => p,
        Install::Manual => return false,
    };
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    real(&this) == real(target)
}

/// `update.done` beside the marker: when the last update was confirmed.
fn stamp(marker: &Path) -> PathBuf {
    marker.with_file_name("update.done")
}

/// A second start this soon after an update's start opens nothing.
const JUST: Duration = Duration::from_secs(60);

/// An update just started (or is about to): a start that finds OpenCircle
/// running then opens nothing (no Settings), whoever started it.
pub fn just_updated() -> bool {
    let Some(path) = swap::Marker::path() else {
        return false;
    };
    let waiting = swap::Marker::read(&path).map(|m| m.version);
    let age = (std::fs::metadata(stamp(&path)).and_then(|m| m.modified()))
        .ok()
        .and_then(|t| t.elapsed().ok());
    is_just_updated(waiting.as_deref(), env!("CARGO_PKG_VERSION"), age)
}

/// This version's update is still unconfirmed (`waiting`), or one was confirmed
/// `stamp_age` ago, within `JUST`.
fn is_just_updated(waiting: Option<&str>, ours: &str, stamp_age: Option<Duration>) -> bool {
    waiting == Some(ours) || stamp_age.is_some_and(|age| age < JUST)
}

/// An update to this version waits to be confirmed: a start that fails rolls it back.
pub fn pending() -> bool {
    (swap::Marker::path().and_then(|p| swap::Marker::read(&p)))
        .is_some_and(|m| m.version == env!("CARGO_PKG_VERSION"))
}

/// The new program failed to start: the old one goes back in place and is started.
pub fn roll_back() {
    if let Some(marker) = swap::Marker::path().and_then(|p| swap::roll_back(&p))
        && let Err(e) = spawn(&marker.target, None)
    {
        crate::log!("[opencircle] update: cannot start the old version again: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::channel::mpsc as fmpsc;
    use release::tests::{release, server};
    use verify::tests::SIG;

    /// `check` stages into the one real update folder: one test at a time.
    static STAGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn events(rx: &mut fmpsc::UnboundedReceiver<Trigger>) -> Vec<Event> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|t| match t {
                Trigger::Update(e) => Some(e),
                _ => None,
            })
            .collect()
    }

    fn setup(install: Install, key: &'static str) -> Setup {
        Setup {
            url: release::tests::LATEST.into(),
            current: "1.0.0",
            install,
            key,
        }
    }

    #[test]
    fn a_check_hands_over_once_per_version() {
        let _stage = STAGE.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, mut rx) = fmpsc::unbounded();
        let fake = server(&release("v9.9.9"), SIG);
        let exe = setup(
            Install::Exe("C:/x/opencircle.exe".into()),
            verify::tests::TEST_KEY,
        );
        let (mut memo, mut handled) = (Memo::default(), None);
        let prepared = std::cell::Cell::new(0);
        let prepare = |_: &Staged| {
            prepared.set(prepared.get() + 1);
            Ok(())
        };
        let status = check(&fake, &exe, &mut memo, &mut handled, &tx, &prepare);
        assert!(status.to_string().contains("ready: restart"), "{status}");
        assert_eq!(prepared.get(), 1, "put in place before it is offered");
        let [Event::Ready(staged)] = &events(&mut rx)[..] else {
            panic!("one Ready");
        };
        assert_eq!(std::fs::read(&staged.file).unwrap(), verify::tests::EXE);
        assert_eq!(
            offer().map(|o| (o.version, o.how)),
            Some(("9.9.9".into(), How::Restart))
        );
        assert!(checked().is_some());
        let _ = std::fs::remove_dir_all(&staged.dir);
        let again = check(&fake, &exe, &mut memo, &mut handled, &tx, &prepare);
        assert!(again.to_string().contains("9.9.9 is ready"), "{again}");
        // a newer one in the same run: the first stays the one offered
        let newer = server(&release("v9.9.10"), SIG);
        let later = check(
            &newer,
            &exe,
            &mut Memo::default(),
            &mut handled,
            &tx,
            &prepare,
        );
        assert!(later.to_string().contains("9.9.9 is ready"), "{later}");
        assert!(events(&mut rx).is_empty(), "no second download");
        assert_eq!(prepared.get(), 1);

        // can't be put in place: the release page instead, nothing to restart into
        let fake = server(&release("v9.9.9"), SIG);
        let status = check(&fake, &exe, &mut Memo::default(), &mut None, &tx, &|s| {
            let _ = std::fs::remove_dir_all(&s.dir);
            Err("cannot replace".into())
        });
        assert!(status.to_string().contains("release page"), "{status}");
        assert!(matches!(&events(&mut rx)[..], [Event::Manual { .. }]));

        // deb / rpm: a notice with the page, nothing downloaded
        let fake = server(&release("v9.9.9"), SIG);
        let status = check(
            &fake,
            &setup(Install::Manual, ""),
            &mut Memo::default(),
            &mut None,
            &tx,
            &|_| panic!("nothing to put in place"),
        );
        assert!(status.to_string().contains("release page"), "{status}");
        assert!(
            matches!(&events(&mut rx)[..], [Event::Manual { version, .. }] if version == "9.9.9")
        );
        assert_eq!(fake.asked.lock().unwrap().len(), 1, "only the release JSON");

        // a bad signature: refused, nothing handed over
        let fake = server(&release("v9.9.9"), SIG);
        let wrong = setup(Install::Exe("C:/x/o.exe".into()), verify::tests::WRONG_KEY);
        let status = check(&fake, &wrong, &mut Memo::default(), &mut None, &tx, &|_| {
            Ok(())
        });
        assert!(status.to_string().contains("not installed"), "{status}");
        assert!(events(&mut rx).is_empty());

        // up to date
        let fake = server(&release("v1.0.0"), SIG);
        let status = check(&fake, &exe, &mut Memo::default(), &mut None, &tx, &|_| {
            Ok(())
        });
        assert_eq!(status, Answer::UpToDate("1.0.0".into()));
    }

    #[test]
    fn install_swaps_smoke_tests_and_rolls_back_a_broken_build() {
        // The "new version" is a script that prints a version (or not).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = release::tests::temp("install");
            std::fs::create_dir_all(dir.join("dl")).unwrap();
            let target = dir.join("OpenCircle.AppImage");
            std::fs::write(&target, b"old").unwrap();
            let marker = dir.join("update.json");
            let staged = |body: &str| {
                let file = dir.join("dl").join("new");
                std::fs::write(&file, body).unwrap();
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
                Staged {
                    version: "9.9.9".into(),
                    sha256: verify::sha256(body.as_bytes()).unwrap(),
                    file,
                    dir: dir.join("dl"),
                    install: Install::AppImage(target.clone()),
                    notes: "## New".into(),
                    url: "https://example.test/r".into(),
                }
            };
            // says the wrong version: refused, the old file is back, no marker
            let err = prepare_at(&staged("#!/bin/sh\necho 1.0.0\n"), &marker).unwrap_err();
            assert!(err.contains("1.0.0"), "{err}");
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            assert!(!marker.exists() && !swap::backup_of(&target).exists());
            // changed after it was verified: refused, nothing moved
            let mut swapped = staged("#!/bin/sh\necho 9.9.9\n");
            std::fs::write(&swapped.file, "#!/bin/sh\necho 9.9.9 # evil\n").unwrap();
            swapped.dir = dir.join("dl-unused");
            let err = prepare_at(&swapped, &marker).unwrap_err();
            assert!(err.contains("changed after"), "{err}");
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            // crashes: same
            assert!(prepare_at(&staged("#!/bin/sh\nexit 3\n"), &marker).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            // runs: in place for the next start, quietly (no relaunch yet)
            let good = "#!/bin/sh\necho 9.9.9\n";
            prepare_at(&staged(good), &marker).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), good);
            let m = swap::Marker::read(&marker).unwrap();
            assert_eq!((m.version.as_str(), m.backup.exists()), ("9.9.9", true));
            assert!(m.quiet, "a start that isn't Restart now: the small pill");
            assert_eq!(
                std::fs::read(&m.backup).unwrap(),
                b"old",
                "kept until confirmed"
            );
            // Restart now: What's new next (the relaunch, with RELAUNCH_ARG, prints
            // the version and exits)
            relaunch_at(&marker, false).unwrap();
            assert!(!swap::Marker::read(&marker).unwrap().quiet);
            assert!(relaunch_at(&dir.join("none.json"), false).is_err());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// An installed copy (#151: Program Files can't be swapped): the setup runs
    /// elevated with the verified bytes only; declined = nothing changed, kept.
    #[test]
    fn an_installed_copy_runs_its_verified_setup_and_a_declined_prompt_changes_nothing() {
        use std::cell::RefCell;
        let dir = release::tests::temp("setup");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("update.json");
        let setup_exe = dir.join("OpenCircle-Setup-9.9.9-x64.exe");
        std::fs::write(&setup_exe, b"setup").unwrap();
        let staged = Staged {
            version: "9.9.9".into(),
            file: setup_exe.clone(),
            sha256: verify::sha256(&b"setup"[..]).unwrap(),
            dir: dir.clone(),
            install: Install::Setup(r"C:\Program Files\OpenCircle\opencircle.exe".into()),
            notes: "## New".into(),
            url: "https://example.test/r".into(),
        };
        // the user says No to Windows' prompt
        let ran = RefCell::new(Vec::new());
        let declined = run_setup(&staged, &marker, |exe, args| {
            ran.borrow_mut().push((exe.to_path_buf(), args.to_string()));
            Err(std::io::Error::other(
                "The operation was canceled by the user.",
            ))
        });
        assert!(declined.unwrap_err().contains("canceled"));
        assert!(!marker.exists(), "no notes promised");
        assert!(setup_exe.exists(), "kept for another try");
        // Yes: the verified file runs quietly and starts the app after; the marker
        // makes the new version show its notes
        run_setup(&staged, &marker, |exe, args| {
            ran.borrow_mut().push((exe.to_path_buf(), args.to_string()));
            Ok(())
        })
        .unwrap();
        let hex: String = staged.sha256.iter().map(|b| format!("{b:02x}")).collect();
        let expected = (setup_exe.clone(), format!("/quiet /launch /sha256={hex}"));
        assert_eq!(*ran.borrow(), [expected.clone(), expected]);
        let m = swap::Marker::read(&marker).unwrap();
        assert_eq!((m.version.as_str(), m.notes.as_str()), ("9.9.9", "## New"));
        assert_eq!(m.backup, PathBuf::new(), "nothing for roll_back to move");
        // changed after it was verified: never runs
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(&setup_exe, b"evil").unwrap();
        let err = run_setup(&staged, &marker, |_, _| panic!("must not run")).unwrap_err();
        assert!(err.contains("changed after"), "{err}");
        assert!(!marker.exists());
        // and the swap never touches an installed copy
        assert!(prepare_at(&staged, &marker).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The setup asset needs the same signed checksum as every download.
    #[test]
    fn an_installed_copy_takes_the_setup_only_when_it_is_signed() {
        let _stage = STAGE.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, mut rx) = fmpsc::unbounded();
        let mut r = release("v9.9.9");
        r.assets.push(release::Asset {
            name: "OpenCircle-Setup-9.9.9-x64.exe".into(),
            browser_download_url: "https://example.test/dl/OpenCircle-Setup-9.9.9-x64.exe".into(),
            size: 1,
        });
        let mut fake = server(&r, SIG);
        fake.files.insert(
            "https://example.test/dl/OpenCircle-Setup-9.9.9-x64.exe".into(),
            (200, b"setup".to_vec()),
        );
        let installed = setup(
            Install::Setup("C:/Program Files/OpenCircle/opencircle.exe".into()),
            verify::tests::TEST_KEY,
        );
        // the signed checksums don't list this setup: refused, nothing handed over
        let status = check(
            &fake,
            &installed,
            &mut Memo::default(),
            &mut None,
            &tx,
            &|_| Ok(()),
        );
        assert!(
            status.to_string().contains("could not be verified"),
            "{status}"
        );
        assert!(events(&mut rx).is_empty());
        // a wrong key: the same
        let wrong = setup(
            Install::Setup("C:/Program Files/OpenCircle/opencircle.exe".into()),
            verify::tests::WRONG_KEY,
        );
        let status = check(&fake, &wrong, &mut Memo::default(), &mut None, &tx, &|_| {
            Ok(())
        });
        assert!(
            status.to_string().contains("could not be verified"),
            "{status}"
        );
        assert!(events(&mut rx).is_empty());
    }

    #[test]
    fn a_test_key_needs_the_test_hook() {
        assert_eq!(key(false), verify::RELEASE_PUBLIC_KEY);
        if option_env!("OPENCIRCLE_TEST_RELEASE_KEY").is_none() {
            assert_eq!(key(true), verify::RELEASE_PUBLIC_KEY);
        }
    }

    #[test]
    fn manual_installs_get_a_notice_not_a_download() {
        assert_eq!(
            prepare_at(
                &Staged {
                    version: "9.9.9".into(),
                    file: "x".into(),
                    sha256: [0; 32],
                    dir: "y".into(),
                    install: Install::Manual,
                    notes: String::new(),
                    url: String::new(),
                },
                Path::new("unused"),
            ),
            Err("this install updates by hand".into())
        );
    }

    /// A setup's update never runs at quit or lock (its prompt would come from
    /// nowhere): refused before anything is touched.
    #[test]
    fn a_setup_update_restarts_only_on_the_button() {
        let staged = Staged {
            version: "9.9.9".into(),
            file: "x".into(),
            sha256: [0; 32],
            dir: "y".into(),
            install: Install::Setup("C:/Program Files/OpenCircle/opencircle.exe".into()),
            notes: String::new(),
            url: String::new(),
        };
        for moment in [Moment::Quit, Moment::Locked] {
            let err = restart(&staged, moment).unwrap_err();
            assert!(err.contains("only when asked"), "{err}");
        }
    }

    /// A start that finds OpenCircle running opens nothing while this version's
    /// update is unconfirmed or was confirmed under a minute ago (the setup's own
    /// start, Windows restarting what it closed). Before: it opened Settings.
    #[test]
    fn a_start_right_after_an_update_opens_nothing() {
        let s = Duration::from_secs;
        assert!(is_just_updated(Some("9.9.9"), "9.9.9", None));
        assert!(is_just_updated(None, "9.9.9", Some(s(10))));
        assert!(!is_just_updated(None, "9.9.9", Some(s(61))));
        assert!(
            !is_just_updated(Some("9.9.8"), "9.9.9", None),
            "another version's"
        );
        assert!(!is_just_updated(None, "9.9.9", None));
    }
}
