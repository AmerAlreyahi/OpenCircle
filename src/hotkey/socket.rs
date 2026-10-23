//! Single instance + CLI trigger: later launches send one line to the running app.
//! Protocol: one UTF-8 line; `capture` opens the overlay, anything else is ignored.
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

use crate::{Trigger, Triggers};

/// Longest accepted command line; anything longer is dropped.
const MAX_LINE: u64 = 64;

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
                let mut line = String::new();
                let read = BufReader::new(conn.take(MAX_LINE)).read_line(&mut line);
                // Only complete lines count.
                if read.is_ok() && line.strip_suffix('\n') == Some("capture") {
                    let _ = tx.unbounded_send(Trigger::Capture(Instant::now()));
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
