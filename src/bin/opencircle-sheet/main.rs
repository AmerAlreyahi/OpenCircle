//! `opencircle-sheet`: the results card's helper process on Linux (the code is
//! `src/sheet/helper`, shared with Windows, where it is a mode of `opencircle.exe`
//! instead). The only binary that links WebKitGTK: the resident app stays light.
#[cfg(target_os = "linux")]
#[path = "../../sheet/helper/mod.rs"]
mod helper;
#[allow(dead_code)] // the app's half
#[path = "../../sheet/proto.rs"]
mod proto;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    helper::main(std::env::args().skip(1).collect())
}

// Windows ships one exe (`opencircle.exe sheet-helper`); this stub is built by
// `--features sheet` but never packaged (Cargo.toml `binaries`).
#[cfg(not(target_os = "linux"))]
fn main() {}
