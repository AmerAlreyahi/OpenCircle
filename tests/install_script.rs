//! `scripts/install.sh` (the Linux one-command install), run for real in its test
//! mode against local files: a good release, tampered ones, the real v0.1.0 list.
//! The signed fixtures are the updater's own test pair (src/update/verify.rs).
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/install.sh");
const TEST_KEY: &str = "RWSgzdo6hR+6Sc4BS7E5EbUQ0oNvTzpWOxUcHtjz7NCgdU/drq8dH4wL";
const WRONG_KEY: &str = "RWTHvR4v6FeTQDjOyNbkFag6emGYS/XU+pGF0h0Vc3mCNWeDwcebDBLc";
const ASSET: &str = "OpenCircle_9.9.9_x86_64.AppImage";
const APPIMAGE: &[u8] = b"OpenCircle 9.9.9 test appimage\n";
const SUMS: &str = "\
4072929c317ec620b528e2c89db29a59d3d48950a68abf2050ef988c3aa4a7e3  OpenCircle_9.9.9_x64-portable.exe
7648ecaf62b6aa287953bbc20590685263e9e3328f2ac4cd04018c294fedd78a  OpenCircle_9.9.9_x86_64.AppImage
";
const SIG: &str = "untrusted comment: signature from rsign secret key
RUSgzdo6hR+6SXDqXJshXB6Uye/0C7TrTkRpJp4Su7PYFs/wGEjIkkoJwnn9wNL9de2XcwrOSug54JDoFWuXHg6iKCE6LuK7dQQ=
trusted comment: OpenCircle v9.9.9
WCpy6e+eWE3H34iMPtayBwO3eavwLD+Y7kqziwOB9ppmNXBwF16cko/XSKgB8qGBMThWkl84UTCkXhHE4P5yDQ==
";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oc-install-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake "download place": the signed list, its signature and the AppImage.
fn release(name: &str, sums: &str, sig: &str, image: &[u8]) -> PathBuf {
    let dir = tmp(name);
    fs::write(dir.join("SHA256SUMS.txt"), sums).unwrap();
    fs::write(dir.join("SHA256SUMS.txt.minisig"), sig).unwrap();
    fs::write(dir.join(ASSET), image).unwrap();
    dir
}

/// Runs the script in test mode with an empty HOME. `tag` is the release it asks for.
fn run(base: &Path, home: &Path, key: &str, tag: &str, args: &[&str]) -> Output {
    Command::new("sh")
        .arg(SCRIPT)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", home)
        .env("OPENCIRCLE_INSTALL_TEST", "1")
        .env("OPENCIRCLE_KEY", key)
        .env("OPENCIRCLE_TAG", tag)
        .env("OPENCIRCLE_BASE", format!("file://{}", base.display()))
        .env("OPENCIRCLE_NO_START", "1")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn installed(home: &Path) -> Vec<PathBuf> {
    [
        "bin/opencircle",
        "share/applications/com.opencircle.app.desktop",
    ]
    .iter()
    .map(|p| home.join(".local").join(p))
    .filter(|p| p.exists())
    .collect()
}

/// The fixtures are x86_64 AppImages; the script picks its asset by the machine.
fn x86_64() -> bool {
    std::env::consts::ARCH == "x86_64"
}

#[test]
fn a_good_release_installs_and_runs_again_and_uninstalls() {
    if !x86_64() {
        return;
    }
    let (base, home) = (release("good", SUMS, SIG, APPIMAGE), tmp("good-home"));
    let out = run(&base, &home, TEST_KEY, "v9.9.9", &[]);
    assert!(out.status.success(), "{}", text(&out));
    let bin = home.join(".local/bin/opencircle");
    assert_eq!(fs::read(&bin).unwrap(), APPIMAGE);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o111,
        0o111
    );
    let entry =
        fs::read_to_string(home.join(".local/share/applications/com.opencircle.app.desktop"))
            .unwrap();
    assert!(
        entry.contains(&format!("Exec={}", bin.display())),
        "{entry}"
    );
    assert!(
        entry.contains("Icon=com.opencircle.app")
            && entry.contains("StartupWMClass=com.opencircle.app")
    );

    // Idempotent: the same again is the same result.
    let again = run(&base, &home, TEST_KEY, "v9.9.9", &[]);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(fs::read(&bin).unwrap(), APPIMAGE);

    // Uninstall removes what was installed, and is safe to repeat.
    let gone = run(&base, &home, TEST_KEY, "v9.9.9", &["--uninstall"]);
    assert!(gone.status.success(), "{}", text(&gone));
    assert!(installed(&home).is_empty(), "{:?}", installed(&home));
    let twice = run(&base, &home, TEST_KEY, "v9.9.9", &["--uninstall"]);
    assert!(twice.status.success() && text(&twice).contains("Nothing to remove"));
}

#[test]
fn a_menu_entry_that_is_not_ours_survives_uninstall() {
    let (base, home) = (tmp("foreign"), tmp("foreign-home"));
    let apps = home.join(".local/share/applications");
    fs::create_dir_all(&apps).unwrap();
    let theirs = apps.join("com.opencircle.app.desktop");
    fs::write(&theirs, "[Desktop Entry]\nName=Mine\n").unwrap();
    let out = run(&base, &home, TEST_KEY, "v9.9.9", &["--uninstall"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(theirs.exists());
}

fn refuses(base: &Path, home: &Path, key: &str, tag: &str, why: &str) {
    if !x86_64() {
        return;
    }
    let out = run(base, home, key, tag, &[]);
    assert!(!out.status.success(), "must refuse: {}", text(&out));
    assert!(text(&out).contains(why), "{}", text(&out));
    assert!(installed(home).is_empty(), "nothing may be installed");
}

#[test]
fn a_tampered_list_is_refused() {
    let list = SUMS.replace("7648ecaf", "7648ecab");
    let (base, home) = (release("t-sums", &list, SIG, APPIMAGE), tmp("t-sums-home"));
    refuses(
        &base,
        &home,
        TEST_KEY,
        "v9.9.9",
        "signature on this download is not valid",
    );
}

#[test]
fn a_tampered_signature_is_refused() {
    let sig = SIG.replace("RUSgzdo6", "RUSgzdo7");
    let (base, home) = (release("t-sig", SUMS, &sig, APPIMAGE), tmp("t-sig-home"));
    refuses(
        &base,
        &home,
        TEST_KEY,
        "v9.9.9",
        "signature on this download is not valid",
    );
}

#[test]
fn a_tampered_trusted_comment_is_refused() {
    // The comment is signed too: changing the release name breaks the global signature.
    let sig = SIG.replace("OpenCircle v9.9.9", "OpenCircle v9.9.8");
    let (base, home) = (release("t-cmt", SUMS, &sig, APPIMAGE), tmp("t-cmt-home"));
    refuses(
        &base,
        &home,
        TEST_KEY,
        "v9.9.8",
        "signature on this download is not valid",
    );
}

#[test]
fn a_signature_made_with_another_key_is_refused() {
    let (base, home) = (release("t-key", SUMS, SIG, APPIMAGE), tmp("t-key-home"));
    refuses(
        &base,
        &home,
        WRONG_KEY,
        "v9.9.9",
        "signature on this download is not valid",
    );
}

#[test]
fn an_old_signed_release_served_as_a_newer_one_is_refused() {
    // Genuine signature, but for v9.9.9 while v9.9.10 was asked for.
    let (base, home) = (release("t-old", SUMS, SIG, APPIMAGE), tmp("t-old-home"));
    refuses(
        &base,
        &home,
        TEST_KEY,
        "v9.9.10",
        "an old release is being served as the new one",
    );
}

#[test]
fn a_swapped_appimage_is_refused() {
    let (base, home) = (
        release("t-img", SUMS, SIG, b"not the signed file\n"),
        tmp("t-img-home"),
    );
    refuses(&base, &home, TEST_KEY, "v9.9.9", "checksum does not match");
}

#[test]
fn the_real_release_key_accepts_the_real_v0_1_0_list() {
    if !x86_64() {
        return;
    }
    // The shipped key (no override) and the published v0.1.0 signature pass the
    // signature step; the stand-in AppImage then fails the checksum step: proof that
    // the signature was accepted, since that is the next thing checked.
    let dir = tmp("real");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/release-v0.1.0");
    for f in ["SHA256SUMS.txt", "SHA256SUMS.txt.minisig"] {
        fs::copy(fixtures.join(f), dir.join(f)).unwrap();
    }
    fs::write(dir.join("opencircle_0.1.0_x86_64.AppImage"), b"stand-in").unwrap();
    let home = tmp("real-home");
    let shipped = release_key();
    let out = run(&dir, &home, &shipped, "v0.1.0", &[]);
    let said = text(&out);
    assert!(said.contains("Signature OK"), "{said}");
    assert!(said.contains("checksum does not match"), "{said}");
    assert!(installed(&home).is_empty());
    // And a v0.1.0 list is not accepted as v0.1.1.
    let out = run(&dir, &home, &shipped, "v0.1.1", &[]);
    assert!(
        text(&out).contains("old release is being served"),
        "{}",
        text(&out)
    );
}

/// The key the app's updater trusts (src/update/verify.rs), which the script must share.
fn release_key() -> String {
    let source =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/update/verify.rs")).unwrap();
    let key = source
        .lines()
        .find_map(|l| l.strip_prefix("pub const RELEASE_PUBLIC_KEY: &str = \""))
        .and_then(|l| l.strip_suffix("\";"))
        .expect("RELEASE_PUBLIC_KEY in verify.rs");
    let script = fs::read_to_string(SCRIPT).unwrap();
    assert!(
        script.contains(&format!("\nKEY=\"{key}\"\n")),
        "scripts/install.sh must carry the updater's key: {key}"
    );
    key.to_string()
}

#[test]
fn the_script_is_posix_sh_with_the_safety_switch_on() {
    let script = fs::read_to_string(SCRIPT).unwrap();
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("\nset -eu\n"));
    // Overrides only exist behind the explicit test switch.
    assert!(script.contains("OPENCIRCLE_BASE=\"\" OPENCIRCLE_TAG=\"\" OPENCIRCLE_NO_START=\"\""));
}
