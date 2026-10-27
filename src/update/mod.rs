//! Auto-update (owner decision "Auto-update", AGENTS.md). With `[update] auto = true`
//! a thread asks GitHub for the latest release a little after start and then every
//! 12 h (only the request: no id, no data; a User-Agent and the ETag). A newer final
//! release is downloaded with `SHA256SUMS.txt` and its minisign signature, checked
//! against `verify::RELEASE_PUBLIC_KEY`, and handed to the UI (`Event::Ready`),
//! which installs it when nothing is open (`install`): swap the file, run the new one
//! once (`--version`), relaunch it, exit. The new process confirms the update at
//! start (`confirm`: notes card) or rolls it back if it cannot start (`roll_back`).
//! deb / rpm / source builds can't replace themselves: `Event::Manual` (notes card
//! with the release page). Failures are logged, never shown, unless the user asked
//! (Settings > Check now: `Event::Status`).

mod notes;
mod release;
pub use release::version;
mod swap;
pub(crate) mod verify;

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

pub use notes::{Kind as NoteKind, Line as NoteLine, parse as parse_notes};
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

/// What the update thread tells the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The result of a check the user asked for (Settings shows it).
    Status(String),
    /// Downloaded and verified: install when idle.
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
                Some(setup) => check(&release::Net, &setup, &mut memo, &mut handled, tx),
                None => "this is a development build: it does not update itself".into(),
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
            url: url.unwrap_or_else(|| LATEST_URL.into()),
            current,
            install: Install::detect(),
            key: verify::RELEASE_PUBLIC_KEY,
        })
    }
}

/// One check; sends `Ready` / `Manual` for a newer release (once per version:
/// `handled`). Returns the status line.
fn check(
    http: &dyn Http,
    setup: &Setup,
    memo: &mut Memo,
    handled: &mut Option<String>,
    tx: &Triggers,
) -> String {
    let current = setup.current;
    let release = match release::latest(http, &setup.url, memo) {
        Ok(r) => r,
        Err(CheckError::Wait(s)) => {
            return format!("GitHub asks to wait: try again in {} min", s.div_ceil(60));
        }
        Err(CheckError::Failed(e)) => {
            crate::log!("[opencircle] update check failed: {}", release::scrub(&e));
            return "could not reach GitHub: try again later".into();
        }
    };
    if !release::is_newer(&release, current) {
        return format!("OpenCircle {current} is up to date");
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    if handled.as_deref() == Some(version.as_str()) {
        return format!("OpenCircle {version} is waiting to install");
    }
    let install = &setup.install;
    let manual = |handled: &mut Option<String>| {
        *handled = Some(version.clone());
        let _ = tx.unbounded_send(Trigger::Update(Event::Manual {
            version: version.clone(),
            notes: release.body.clone().unwrap_or_default(),
            url: release.html_url.clone(),
        }));
        format!("OpenCircle {version} is available: get it from the release page")
    };
    if *install == Install::Manual {
        return manual(handled);
    }
    let Some(dir) = stage_dir() else {
        return "no data folder: not installed".into();
    };
    match release::stage(
        http,
        &release,
        install,
        std::env::consts::ARCH,
        &dir,
        setup.key,
    ) {
        Ok(staged) => {
            *handled = Some(version.clone());
            let _ = tx.unbounded_send(Trigger::Update(Event::Ready(staged)));
            format!(
                "OpenCircle {version} is downloaded and verified: it installs when OpenCircle is not in use"
            )
        }
        Err(e) if e == "no download for this system" => manual(handled),
        Err(e) => {
            crate::log!(
                "[opencircle] update {version} refused: {}",
                release::scrub(&e)
            );
            format!("OpenCircle {version} could not be verified: not installed")
        }
    }
}

/// Where downloads are staged: `<local data dir>/opencircle/update`, this user's
/// own (never the shared temp dir), made fresh and private for each download.
fn stage_dir() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("opencircle").join("update"))
}

/// A download left by an earlier run (only one instance runs, so none is in use).
fn remove_stale_downloads() {
    if let Some(dir) = stage_dir()
        && std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir())
    {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Installs a verified update now (call only while OpenCircle is idle): swaps the
/// file, runs the new one once, writes the marker and starts it. `Ok` = the caller
/// exits now. On `Err` everything is as before.
pub fn install(staged: &Staged) -> Result<(), String> {
    let result = install_at(staged, &swap::Marker::path().ok_or("no data folder")?);
    let _ = std::fs::remove_dir_all(&staged.dir);
    result
}

fn install_at(staged: &Staged, marker_path: &Path) -> Result<(), String> {
    let target = match &staged.install {
        Install::Exe(p) | Install::AppImage(p) => p,
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
    };
    marker.write(marker_path).map_err(|e| undo(e.to_string()))?;
    if let Err(e) = spawn(target, Some(RELAUNCH_ARG)) {
        let _ = std::fs::remove_file(marker_path);
        return Err(undo(format!("cannot start the new version: {e}")));
    }
    crate::log!(
        "[opencircle] update {}: installed, restarting",
        staged.version
    );
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

/// At start: an update that is waiting to be confirmed is confirmed (the old
/// program goes). Returns its notes card: version, notes, release page.
pub fn confirm() -> Option<(String, String, String)> {
    let marker = swap::confirm(&swap::Marker::path()?)?;
    crate::log!("[opencircle] update {}: running", marker.version);
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
    Some((marker.version, marker.notes, marker.url))
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
        let (tx, mut rx) = fmpsc::unbounded();
        let fake = server(&release("v9.9.9"), SIG);
        let exe = setup(
            Install::Exe("C:/x/opencircle.exe".into()),
            verify::tests::TEST_KEY,
        );
        let (mut memo, mut handled) = (Memo::default(), None);
        let status = check(&fake, &exe, &mut memo, &mut handled, &tx);
        assert!(status.contains("verified"), "{status}");
        let [Event::Ready(staged)] = &events(&mut rx)[..] else {
            panic!("one Ready");
        };
        assert_eq!(std::fs::read(&staged.file).unwrap(), verify::tests::EXE);
        let _ = std::fs::remove_dir_all(&staged.dir);
        let again = check(&fake, &exe, &mut memo, &mut handled, &tx);
        assert!(again.contains("waiting"), "{again}");
        assert!(events(&mut rx).is_empty(), "no second download");

        // deb / rpm: a notice with the page, nothing downloaded
        let fake = server(&release("v9.9.9"), SIG);
        let status = check(
            &fake,
            &setup(Install::Manual, ""),
            &mut Memo::default(),
            &mut None,
            &tx,
        );
        assert!(status.contains("release page"), "{status}");
        assert!(
            matches!(&events(&mut rx)[..], [Event::Manual { version, .. }] if version == "9.9.9")
        );
        assert_eq!(fake.asked.lock().unwrap().len(), 1, "only the release JSON");

        // a bad signature: refused, nothing handed over
        let fake = server(&release("v9.9.9"), SIG);
        let wrong = setup(Install::Exe("C:/x/o.exe".into()), verify::tests::WRONG_KEY);
        let status = check(&fake, &wrong, &mut Memo::default(), &mut None, &tx);
        assert!(status.contains("not installed"), "{status}");
        assert!(events(&mut rx).is_empty());

        // up to date
        let fake = server(&release("v1.0.0"), SIG);
        let status = check(&fake, &exe, &mut Memo::default(), &mut None, &tx);
        assert_eq!(status, "OpenCircle 1.0.0 is up to date");
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
            let err = install_at(&staged("#!/bin/sh\necho 1.0.0\n"), &marker).unwrap_err();
            assert!(err.contains("1.0.0"), "{err}");
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            assert!(!marker.exists() && !swap::backup_of(&target).exists());
            // changed after it was verified: refused, nothing moved
            let mut swapped = staged("#!/bin/sh\necho 9.9.9\n");
            std::fs::write(&swapped.file, "#!/bin/sh\necho 9.9.9 # evil\n").unwrap();
            swapped.dir = dir.join("dl-unused");
            let err = install_at(&swapped, &marker).unwrap_err();
            assert!(err.contains("changed after"), "{err}");
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            // crashes: same
            assert!(install_at(&staged("#!/bin/sh\nexit 3\n"), &marker).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            // runs (the relaunch, with RELAUNCH_ARG, prints the version and exits too)
            let good = "#!/bin/sh\necho 9.9.9\n";
            install_at(&staged(good), &marker).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), good);
            let m = swap::Marker::read(&marker).unwrap();
            assert_eq!((m.version.as_str(), m.backup.exists()), ("9.9.9", true));
            assert_eq!(
                std::fs::read(&m.backup).unwrap(),
                b"old",
                "kept until confirmed"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn manual_installs_get_a_notice_not_a_download() {
        assert_eq!(
            install_at(
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
}
