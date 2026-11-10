# Contributing

Thanks for helping. Bug reports, fixes and ideas are all welcome. Please be
kind: see the [Code of Conduct](CODE_OF_CONDUCT.md). For security problems, use
the [Security policy](SECURITY.md), not a public issue.

## Build and run

You need a recent stable Rust. On Linux, see [docs/linux-notes.md](docs/linux-notes.md)
for the system packages.

    cargo run                      # build and start
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) cargo test

Run the tests with empty data and config folders (as above). CI has no
downloaded models or config, and a machine that has them can hide a failure.

A change that touches Windows code should also pass the Windows cross-check
(needs `cargo-xwin`):

    cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings

Packaging: `cargo packager --release`.

## Report a bug well

Use the bug form. Please give us:

- the OpenCircle version (Settings > About);
- your OS and version; on Linux also the desktop and whether it is Wayland or X11;
- the steps, what you expected and what happened;
- a screenshot, if it helps (hide anything private);
- the end of the log file, `opencircle.log`, in the `logs` folder inside your
  local data folder (`%LOCALAPPDATA%\OpenCircle\logs` on Windows). It never holds
  your text or keys, but look before you paste.

## Code rules, short

- Fix the root cause, in the place every caller goes through.
- A bug fix comes with a test that fails before the fix and passes after.
- Windows and Linux together: a change must build and work on both.
- No duplicate code: if two places need it, build it once and share it.
- Match the style around you. Keep the diff small. No new dependency unless it
  is really needed.
- Nothing is sent anywhere the user did not choose, and no secret is ever
  logged or committed.

## Commits

Commits must not carry AI attribution lines (`Co-Authored-By`, "Generated with",
or any AI name). The repo's hooks enforce it. After you clone, run once:

    git config core.hooksPath .githooks

Open a pull request against `main` and fill in the short checklist.
