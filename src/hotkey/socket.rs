//! Single instance + CLI trigger: later launches send one line to the running app.
//! Protocol: one UTF-8 line; `capture` opens the overlay, anything else is ignored.
use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::Instant;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name};

use crate::{Trigger, Triggers};

fn name() -> io::Result<Name<'static>> {
    // Abstract socket on Linux, named pipe on Windows: nothing stale left on disk.
    "opencircle.sock".to_ns_name::<GenericNamespaced>()
}

/// Sends `command` to the running instance. `Err` = no instance is running.
pub fn forward(command: &str) -> io::Result<()> {
    let mut stream = LocalSocketStream::connect(name()?)?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")
}

/// Becomes the running instance: serves commands from later launches on a thread.
pub fn listen(tx: Triggers) -> io::Result<()> {
    let listener = ListenerOptions::new().name(name()?).create_sync()?;
    std::thread::spawn(move || {
        for conn in listener.incoming().filter_map(Result::ok) {
            let mut line = String::new();
            // Cap the read: the socket is reachable by other local processes.
            let _ = BufReader::new(conn.take(64)).read_line(&mut line);
            if line.trim_end() == "capture" {
                let _ = tx.unbounded_send(Trigger::Capture(Instant::now()));
            }
        }
    });
    Ok(())
}
