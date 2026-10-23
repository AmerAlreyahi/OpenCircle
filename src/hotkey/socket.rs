//! Single instance + CLI trigger: later launches send one line to the running app.
//! Protocol: one ASCII line, `capture` or `run <action> [<target>]`; anything else
//! is dropped (see `parse_command`, the one place that decides what is accepted).
//!
//! The endpoint is per user, so another account can neither squat it (making our
//! instance think one is already running) nor send it commands:
//! - Unix: a socket file in `$XDG_RUNTIME_DIR` (0700 by spec), else in a 0700
//!   `opencircle-<uid>` dir we own under the temp dir; peers must have our uid.
//! - Windows: a pipe named after the user whose DACL admits only its owner.
use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::Instant;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name};

use crate::Triggers;
use crate::config::Target;

/// Longest accepted command line; anything longer is dropped.
const MAX_LINE: u64 = 64;
const MAX_ACTION: usize = 32;

/// A command from a later launch. Built only by `parse_command` / `from_args`, so an
/// action here is always well-formed and known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Capture,
    /// `target: None` = the target the user configured for that action.
    Run {
        action: String,
        target: Option<Target>,
    },
}

impl Command {
    /// The wire form, without the newline.
    pub fn line(&self) -> String {
        match self {
            Command::Capture => "capture".into(),
            Command::Run {
                action,
                target: None,
            } => format!("run {action}"),
            Command::Run {
                action,
                target: Some(t),
            } => format!("run {action} {}", super::target_name(*t)),
        }
    }

    /// Parses the command line after the program name:
    /// `capture` (extra words ignored, as before) or `run <action> [--target <t>]`.
    pub fn from_args(args: &[String]) -> Result<Command, String> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["capture", ..] => Ok(Command::Capture),
            ["run", action] => run(action, None),
            ["run", action, "--target", t] => run(action, Some(t)),
            ["run", action, t] if t.starts_with("--target=") => {
                run(action, Some(&t["--target=".len()..]))
            }
            ["run"] => Err("run needs an action, like `opencircle run translate`".into()),
            _ => Err("unrecognised arguments".into()),
        }
    }
}

fn run(action: &str, target: Option<&str>) -> Result<Command, String> {
    let well_formed = (1..=MAX_ACTION).contains(&action.len())
        && action
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !well_formed || !super::known_action(action) {
        return Err(format!("unknown action \"{}\"", printable(action)));
    }
    let target = match target {
        None => None,
        Some(t) => Some(super::parse_target(t).ok_or_else(|| {
            format!(
                "unknown target \"{}\" (full_screen, circle, last_region or window)",
                printable(t)
            )
        })?),
    };
    Ok(Command::Run {
        action: action.into(),
        target,
    })
}

/// Echo-safe copy of untrusted text for an error message.
fn printable(s: &str) -> String {
    s.chars()
        .take(MAX_ACTION)
        .map(|c| if c.is_ascii_graphic() { c } else { '?' })
        .collect()
}

/// Strict parse of one wire line (newline included: an unfinished line is rejected).
pub fn parse_command(line: &str) -> Result<Command, String> {
    let line = line.strip_suffix('\n').ok_or("incomplete line")?;
    if !line.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
        return Err("not plain ASCII text".into());
    }
    let words: Vec<&str> = line.split(' ').collect();
    match words.as_slice() {
        ["capture"] => Ok(Command::Capture),
        ["run", action] => run(action, None),
        ["run", action, target] => run(action, Some(target)),
        _ => Err("unknown command".into()),
    }
}

/// Reads one command from a peer: at most `MAX_LINE` bytes, valid UTF-8, complete.
fn read_command(peer: impl Read) -> Result<Command, String> {
    let mut line = String::new();
    BufReader::new(peer.take(MAX_LINE))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    parse_command(&line)
}

/// Sends `command` to the running instance. `Err` = no instance is running.
pub fn forward(command: &str) -> io::Result<()> {
    let mut stream = LocalSocketStream::connect(endpoint()?)?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")
}

/// Becomes the running instance: serves commands from later launches on a thread.
pub fn listen(tx: Triggers) -> io::Result<()> {
    let listener = listener_options(endpoint()?)?.create_sync()?;
    std::thread::spawn(move || {
        for conn in listener.incoming().filter_map(Result::ok) {
            if !peer_is_us(&conn) {
                continue;
            }
            // One thread per connection: a client that sends nothing can't block the rest.
            // ponytail: no read timeout; only our own user can connect, so a stalled
            // client costs one parked thread, not the service.
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Dropped silently: the peer is a script, not a person to explain to.
                if let Ok(command) = read_command(conn) {
                    let _ = tx.unbounded_send(super::trigger(&command, Instant::now()));
                }
            });
        }
    });
    Ok(())
}

#[cfg(unix)]
fn endpoint() -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let uid = current_uid();
    let dir = socket_dir(
        std::env::var_os("XDG_RUNTIME_DIR"),
        uid,
        &std::env::temp_dir(),
    );
    if std::env::var_os("XDG_RUNTIME_DIR").is_none_or(|d| d.is_empty()) {
        // Our fallback dir: create it 0700, or accept it only if it is ours and private.
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let meta = std::fs::symlink_metadata(&dir)?;
        if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is not a private directory of this user", dir.display()),
            ));
        }
    }
    dir.join("opencircle.sock").to_fs_name::<GenericFilePath>()
}

/// Where the socket lives: the runtime dir, else a per-uid dir under `temp`.
#[cfg(unix)]
fn socket_dir(
    runtime: Option<std::ffi::OsString>,
    uid: u32,
    temp: &std::path::Path,
) -> std::path::PathBuf {
    match runtime {
        Some(dir) if !dir.is_empty() => dir.into(),
        _ => temp.join(format!("opencircle-{uid}")),
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn listener_options(name: Name<'static>) -> io::Result<ListenerOptions<'static>> {
    // A socket file left by a crashed instance is replaced (we only get here
    // after connecting to it failed).
    Ok(ListenerOptions::new().name(name).try_overwrite(true))
}

#[cfg(unix)]
fn peer_is_us(conn: &LocalSocketStream) -> bool {
    conn.peer_creds()
        .ok()
        .and_then(|creds| creds.euid())
        .is_some_and(|uid| uid == current_uid())
}

#[cfg(windows)]
fn endpoint() -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    pipe_name(&std::env::var("USERNAME").unwrap_or_default()).to_ns_name::<GenericNamespaced>()
}

/// Per-user pipe name; characters outside `[A-Za-z0-9_-]` become `_`.
#[cfg_attr(not(windows), allow(dead_code))]
fn pipe_name(user: &str) -> String {
    let user: String = user
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("opencircle-{user}")
}

#[cfg(windows)]
fn listener_options(name: Name<'static>) -> io::Result<ListenerOptions<'static>> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    // Protected DACL with one ACE: full access for the owner (the creating user).
    // ponytail: a pipe pre-created by another user under our name still makes us
    // exit as "already running" (DoS only); verify the server's owner if it matters.
    let sd = SecurityDescriptor::deserialize(widestring::u16cstr!("D:P(A;;GA;;;OW)"))?;
    Ok(ListenerOptions::new().name(name).security_descriptor(sd))
}

#[cfg(windows)]
fn peer_is_us(_conn: &LocalSocketStream) -> bool {
    true // the pipe's DACL already admits only our user
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn run_of(action: &str, target: Option<Target>) -> Command {
        Command::Run {
            action: action.into(),
            target,
        }
    }

    #[test]
    fn parses_the_wire_lines() {
        assert_eq!(parse_command("capture\n"), Ok(Command::Capture));
        assert_eq!(
            parse_command("run translate\n"),
            Ok(run_of("translate", None))
        );
        assert_eq!(
            parse_command("run ai_explain circle\n"),
            Ok(run_of("ai_explain", Some(Target::Circle)))
        );
        assert_eq!(
            parse_command("run copy_text last_region\n"),
            Ok(run_of("copy_text", Some(Target::LastRegion)))
        );
    }

    #[test]
    fn rejects_bad_wire_lines() {
        for bad in [
            "capture", // unfinished
            "",
            "\n",
            "Capture\n",
            "capture now\n",
            "capture \n",
            "run\n",
            "run \n",
            "run  translate\n", // empty word
            "run nope\n",       // unknown action
            "run Translate\n",
            "run translate sideways\n", // bad target
            "run translate circle x\n",
            "run translate\r\n",
            "run trans\tlate\n",
            "run tr\u{e4}nslate\n",
            "run ../../etc/passwd\n",
            "run translate\0\n",
            "\x1b[2Jrun translate\n",
            "capture\nrun translate\n",
            &format!("run {}\n", "a".repeat(33)),
        ] {
            assert!(parse_command(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn reads_from_a_peer_with_limits() {
        let read = |bytes: &[u8]| read_command(io::Cursor::new(bytes.to_vec()));
        assert_eq!(read(b"capture\n"), Ok(Command::Capture));
        assert!(read(b"capture\n\xff").is_ok(), "only the first line counts");
        assert!(read(b"run tran\xffslate\n").is_err(), "invalid UTF-8");
        assert!(read(&[0xff; 10]).is_err());
        assert!(read(b"").is_err());
        // Longer than MAX_LINE: cut short before the newline, so incomplete.
        let long = format!("run translate {}\n", "x".repeat(200));
        assert!(read(long.as_bytes()).is_err());
        let just_over = format!("run {}\n", "a".repeat(MAX_LINE as usize));
        assert!(read(just_over.as_bytes()).is_err());
    }

    #[test]
    fn parses_cli_arguments() {
        assert_eq!(
            Command::from_args(&args(&["capture"])),
            Ok(Command::Capture)
        );
        assert_eq!(
            Command::from_args(&args(&["capture", "extra"])),
            Ok(Command::Capture)
        );
        assert_eq!(
            Command::from_args(&args(&["run", "translate"])),
            Ok(run_of("translate", None))
        );
        assert_eq!(
            Command::from_args(&args(&["run", "lens", "--target", "full_screen"])),
            Ok(run_of("lens", Some(Target::FullScreen)))
        );
        assert_eq!(
            Command::from_args(&args(&["run", "lens", "--target=circle"])),
            Ok(run_of("lens", Some(Target::Circle)))
        );
        for bad in [
            &["run"][..],
            &["run", "nope"],
            &["run", "lens", "--target"],
            &["run", "lens", "--target", "x"],
            &["run", "lens", "--target=x"],
            &["run", "lens", "circle"],
            &["run", "lens", "--target", "circle", "more"],
            &["run", "lens\n"],
            &["go"],
        ] {
            assert!(Command::from_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn what_we_send_is_what_we_accept() {
        for command in [
            Command::Capture,
            run_of("translate", None),
            run_of("ai_explain", Some(Target::Circle)),
            run_of("copy_text", Some(Target::LastRegion)),
            run_of("lens", Some(Target::FullScreen)),
        ] {
            assert_eq!(parse_command(&format!("{}\n", command.line())), Ok(command));
        }
    }

    #[cfg(unix)]
    #[test]
    fn socket_dir_prefers_runtime_dir_and_falls_back_per_uid() {
        let tmp = std::path::Path::new("/tmp");
        assert_eq!(
            socket_dir(Some("/run/user/1000".into()), 1000, tmp),
            std::path::PathBuf::from("/run/user/1000")
        );
        assert_eq!(socket_dir(None, 1000, tmp), tmp.join("opencircle-1000"));
        assert_eq!(
            socket_dir(Some("".into()), 42, tmp),
            tmp.join("opencircle-42")
        );
    }

    #[test]
    fn pipe_name_is_per_user_and_sanitised() {
        assert_eq!(pipe_name("amer"), "opencircle-amer");
        assert_eq!(pipe_name(r"DOMAIN\Amer Z"), "opencircle-DOMAIN_Amer_Z");
        assert_ne!(pipe_name("alice"), pipe_name("bob"));
    }
}
