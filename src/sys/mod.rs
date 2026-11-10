//! OS plumbing with no UI: opening links and files, detached process start,
//! the desktop session's kind, the user's language.

pub mod locale;
pub mod mem;
pub mod open;
pub mod serve_once;
pub mod session;
pub mod spawn;

use std::ffi::OsString;

/// `s` as the NUL-terminated UTF-16 the Windows APIs take.
#[cfg(any(windows, test))]
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// A test hook's variable (`OPENCIRCLE_TEST_*`: screenshots of our windows to
/// a folder, a PNG instead of the screen, a test crash). Release builds read
/// hooks only with the one opt-in `OPENCIRCLE_TEST_HOOKS=1` (the VM scripts
/// set it), so a stray variable can't make a user's copy write captures to
/// disk. `OPENCIRCLE_TEST_UPDATE_URL` is not one: the update gate needs it in
/// published builds (and an update still needs the release key).
pub fn test_hook(name: &str) -> Option<OsString> {
    hooks_on(
        cfg!(debug_assertions),
        std::env::var_os("OPENCIRCLE_TEST_HOOKS"),
    )
    .then(|| std::env::var_os(name))
    .flatten()
}

fn hooks_on(debug_build: bool, opt_in: Option<OsString>) -> bool {
    debug_build || opt_in.is_some_and(|v| v == "1")
}

#[test]
fn wide_strings_end_in_a_nul() {
    assert_eq!(wide("a\u{e9}"), [0x61, 0xe9, 0]);
    assert_eq!(wide(""), [0]);
}

#[test]
fn test_hooks_need_the_opt_in_in_release_builds() {
    assert!(!hooks_on(false, None), "release: off by default");
    assert!(!hooks_on(false, Some("0".into())));
    assert!(!hooks_on(false, Some("".into())));
    assert!(hooks_on(false, Some("1".into())), "the VM scripts' opt-in");
    assert!(hooks_on(true, None), "debug and test builds: always on");
}
