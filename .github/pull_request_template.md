## What changed

<!-- A few lines: what and why. Link the issue if there is one. -->

## Checklist

- [ ] A test that fails before the change and passes after
- [ ] Gate run: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (empty data/config dirs)
- [ ] Works on Windows and Linux (or I say here what I could not check)
- [ ] No AI attribution lines in the commits (`git config core.hooksPath .githooks`)
