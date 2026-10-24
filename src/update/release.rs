//! The newest GitHub release: ask for it (ETag, rate limit), compare its version
//! with ours, pick the asset for this install, download and verify it.
//! The network sits behind `Http`, so tests run on canned answers.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use super::verify;

/// The signed checksums file and its minisign signature, as release.yml names them.
pub const SUMS: &str = "SHA256SUMS.txt";
pub const SIG: &str = "SHA256SUMS.txt.minisig";

/// Largest JSON / checksums / signature, and largest asset, we accept.
const SMALL: u64 = 1 << 20;
const BIG: u64 = 400 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

impl Release {
    pub fn version(&self) -> Option<[u64; 3]> {
        version(&self.tag_name)
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// `X.Y.Z` (an optional leading `v`); anything else (pre-release suffixes too) = `None`.
pub fn version(tag: &str) -> Option<[u64; 3]> {
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let mut out = [0; 3];
    for n in &mut out {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *n = p.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// `release` is a published, final release newer than `current` (our `X.Y.Z`).
pub fn is_newer(release: &Release, current: &str) -> bool {
    !release.draft
        && !release.prerelease
        && matches!((release.version(), version(current)), (Some(new), Some(old)) if new > old)
}

/// How this copy of OpenCircle was installed, so how it can be updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Windows: this exe is replaced by the release's portable exe.
    Exe(PathBuf),
    /// Linux AppImage (`$APPIMAGE`): the file is replaced.
    AppImage(PathBuf),
    /// deb, rpm, a source build: no self-replace (no root); notify + release page.
    Manual,
}

impl Install {
    /// This process's install kind.
    pub fn detect() -> Install {
        if cfg!(windows) {
            return std::env::current_exe().map_or(Install::Manual, Install::Exe);
        }
        match std::env::var_os("APPIMAGE") {
            Some(p) if !p.is_empty() && Path::new(&p).is_absolute() => Install::AppImage(p.into()),
            _ => Install::Manual,
        }
    }
}

/// The asset that updates `install` on `arch` (`std::env::consts::ARCH`), named as
/// release.yml / cargo-packager name them.
pub fn pick<'a>(assets: &'a [Asset], install: &Install, arch: &str) -> Option<&'a Asset> {
    let (suffix, arch) = match (install, arch) {
        (Install::Exe(_), "x86_64") => ("-portable.exe", "_x64"),
        (Install::Exe(_), "aarch64") => ("-portable.exe", "_arm64"),
        (Install::AppImage(_), "x86_64") => (".AppImage", "_x86_64"),
        (Install::AppImage(_), "aarch64") => (".AppImage", "_aarch64"),
        _ => return None,
    };
    assets
        .iter()
        .find(|a| a.name.ends_with(suffix) && a.name.contains(arch) && !a.name.contains('/'))
}

/// One HTTP answer.
pub struct Got {
    pub status: u16,
    pub etag: Option<String>,
    /// Seconds to wait (`retry-after`, or until `x-ratelimit-reset` when none remain).
    pub wait: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

/// GET `url` (with `If-None-Match: etag`), nothing else sent but a User-Agent.
pub trait Http {
    fn get(&self, url: &str, etag: Option<&str>) -> Result<Got, String>;
}

/// The real network: ureq, rustls, no cookies, only GETs.
pub struct Net;

impl Http for Net {
    fn get(&self, url: &str, etag: Option<&str>) -> Result<Got, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            // https only, except the test hook's local fake server
            .https_only(super::test_url().is_none())
            .http_status_as_error(false)
            .user_agent("OpenCircle")
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(900)))
            .build()
            .into();
        let mut req = agent
            .get(url)
            .header("Accept", "application/vnd.github+json");
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        let resp = req.call().map_err(|e| scrub(&e.to_string()))?;
        let header = |n: &str| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let num = |n: &str| header(n).and_then(|v| v.trim().parse::<u64>().ok());
        let wait = num("retry-after").or_else(|| {
            (num("x-ratelimit-remaining") == Some(0))
                .then(|| num("x-ratelimit-reset"))
                .flatten()
                .map(|reset| reset.saturating_sub(unix_now()))
        });
        Ok(Got {
            status: resp.status().as_u16(),
            etag: header("etag"),
            wait,
            body: Box::new(resp.into_body().into_reader()),
        })
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// An error line without URL queries (download links carry signed tokens).
pub fn scrub(text: &str) -> String {
    text.split(' ')
        .map(|w| w.split_once('?').map_or(w, |(head, _)| head))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the last check remembers: the ETag and body of the last answer, and when
/// GitHub said to come back.
#[derive(Debug, Default)]
pub struct Memo {
    etag: Option<(String, Vec<u8>)>,
    pub not_before: Option<SystemTime>,
}

/// Why a check gave no release.
#[derive(Debug, PartialEq, Eq)]
pub enum CheckError {
    /// Rate limited: try after this many seconds.
    Wait(u64),
    Failed(String),
}

/// The latest release at `url`. A 304 reuses the remembered answer.
pub fn latest(http: &dyn Http, url: &str, memo: &mut Memo) -> Result<Release, CheckError> {
    if let Some(t) = memo.not_before
        && SystemTime::now() < t
    {
        let left = t
            .duration_since(SystemTime::now())
            .map_or(0, |d| d.as_secs());
        return Err(CheckError::Wait(left));
    }
    let tag = memo.etag.as_ref().map(|(t, _)| t.as_str());
    let got = http.get(url, tag).map_err(CheckError::Failed)?;
    let body = match got.status {
        200 => {
            let body = read_small(got.body).map_err(CheckError::Failed)?;
            if let Some(tag) = got.etag {
                memo.etag = Some((tag, body.clone()));
            }
            body
        }
        304 if memo.etag.is_some() => memo
            .etag
            .as_ref()
            .map(|(_, b)| b.clone())
            .unwrap_or_default(),
        403 | 429 => {
            let wait = got.wait.unwrap_or(3600).max(60);
            memo.not_before = Some(SystemTime::now() + Duration::from_secs(wait));
            return Err(CheckError::Wait(wait));
        }
        s => return Err(CheckError::Failed(format!("GitHub answered {s}"))),
    };
    serde_json::from_slice(&body).map_err(|e| CheckError::Failed(format!("bad release JSON: {e}")))
}

fn read_small(body: impl Read) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    body.take(SMALL + 1)
        .read_to_end(&mut out)
        .map_err(|e| scrub(&e.to_string()))?;
    if out.len() as u64 > SMALL {
        return Err("answer too large".into());
    }
    Ok(out)
}

fn fetch_small(http: &dyn Http, asset: &Asset) -> Result<Vec<u8>, String> {
    let got = http.get(&asset.browser_download_url, None)?;
    if got.status != 200 {
        return Err(format!("{}: download answered {}", asset.name, got.status));
    }
    read_small(got.body)
}

/// A downloaded, verified update waiting to be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub version: String,
    /// The verified file (inside `dir`) and its SHA-256: the install copies it
    /// and checks the copy again, so only these bytes are ever installed.
    pub file: PathBuf,
    pub sha256: [u8; 32],
    /// Our private staging dir (`private_dir`): deleted after the install or on any failure.
    pub dir: PathBuf,
    pub install: Install,
    pub notes: String,
    pub url: String,
}

/// Downloads `release`'s asset for `install` plus the checksums and signature into
/// `dir` (made fresh and private: `private_dir`), checks the signature with `key`,
/// then the asset's SHA-256. Any later failure deletes `dir`.
pub fn stage(
    http: &dyn Http,
    release: &Release,
    install: &Install,
    arch: &str,
    dir: &Path,
    key: &str,
) -> Result<Staged, String> {
    private_dir(dir).map_err(|e| format!("update folder: {e}"))?;
    let result = stage_in(http, release, install, arch, dir, key);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    result
}

fn stage_in(
    http: &dyn Http,
    release: &Release,
    install: &Install,
    arch: &str,
    dir: &Path,
    key: &str,
) -> Result<Staged, String> {
    let asset = pick(&release.assets, install, arch).ok_or("no download for this system")?;
    let sums = release.asset(SUMS).ok_or("the release has no checksums")?;
    let sig = release.asset(SIG).ok_or("the release is not signed")?;
    let sums = fetch_small(http, sums)?;
    let sig = String::from_utf8(fetch_small(http, sig)?).map_err(|_| "bad signature file")?;
    verify::signed(&sums, &sig, key, release.tag_name.trim_start_matches('v'))?;
    let sums = String::from_utf8(sums).map_err(|_| "bad checksums file")?;
    let want =
        verify::sum_for(&sums, &asset.name).ok_or("the checksums do not list the download")?;

    let file = dir.join(&asset.name);
    let got = http.get(&asset.browser_download_url, None)?;
    if got.status != 200 {
        return Err(format!("download answered {}", got.status));
    }
    let hash = save_hashed(got.body.take(BIG), &file).map_err(|e| scrub(&e.to_string()))?;
    if hash != want {
        return Err("the download does not match its checksum".into());
    }
    Ok(Staged {
        version: release.tag_name.trim_start_matches('v').to_string(),
        file,
        sha256: hash,
        dir: dir.to_path_buf(),
        install: install.clone(),
        notes: release.body.clone().unwrap_or_default(),
        url: release.html_url.clone(),
    })
}

/// Makes `dir` new and private (0700 on Unix): a leftover of ours is removed
/// first; anything else there (a symlink, a file, someone else's or an open
/// folder) is refused. The folder is created, never reused, so nobody else can
/// have put or swapped a file in it.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        if !ours(&meta) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is not OpenCircle's own private folder", dir.display()),
            ));
        }
        std::fs::remove_dir_all(dir)?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    #[cfg(not(unix))]
    let builder = &mut builder;
    builder.create(dir) // fails if something appeared meanwhile
}

/// A real folder (not a link), and on Unix this user's with no access for others.
fn ours(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let uid = unsafe { libc::geteuid() };
        meta.uid() == uid && meta.mode() & 0o077 == 0
    }
    #[cfg(not(unix))]
    true // the per-user local app data folder: only this user may write in it
}

/// Writes `body` to `path` (a new file: never an existing one or a link) and
/// returns its SHA-256.
fn save_hashed(body: impl Read, path: &Path) -> io::Result<[u8; 32]> {
    struct Tee<R> {
        inner: R,
        out: File,
    }
    impl<R: Read> Read for Tee<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.out.write_all(&buf[..n])?;
            Ok(n)
        }
    }
    let mut tee = Tee {
        inner: body,
        out: std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    };
    let hash = verify::sha256(&mut tee)?;
    tee.out.sync_all()?;
    Ok(hash)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::update::verify::tests::{self as v, APPIMAGE, EXE, TEST_KEY, WRONG_KEY};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Canned answers by URL; records what was asked.
    #[derive(Default)]
    pub struct Fake {
        pub files: HashMap<String, (u16, Vec<u8>)>,
        pub etag: Option<String>,
        pub asked: Mutex<Vec<(String, Option<String>)>>,
    }

    impl Http for Fake {
        fn get(&self, url: &str, etag: Option<&str>) -> Result<Got, String> {
            self.asked
                .lock()
                .unwrap()
                .push((url.into(), etag.map(Into::into)));
            let (status, body) = match (&self.etag, etag) {
                (Some(have), Some(sent)) if have == sent => (304, Vec::new()),
                _ => self.files.get(url).cloned().ok_or("connection refused")?,
            };
            Ok(Got {
                status,
                etag: self.etag.clone(),
                wait: (status == 403).then_some(120),
                body: Box::new(io::Cursor::new(body)),
            })
        }
    }

    const BASE: &str = "https://example.test/dl/";
    pub const LATEST: &str = "https://example.test/latest";

    pub fn release(tag: &str) -> Release {
        let asset = |name: &str| Asset {
            name: name.into(),
            browser_download_url: format!("{BASE}{name}"),
            size: 1,
        };
        Release {
            tag_name: tag.into(),
            body: Some("## New\n- faster".into()),
            html_url: format!("https://github.com/x/y/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            assets: [
                "OpenCircle_9.9.9_x64-setup.exe",
                "OpenCircle_9.9.9_x64-portable.exe",
                "OpenCircle_9.9.9_x64_en-US.msi",
                "OpenCircle_9.9.9_x86_64.AppImage",
                "opencircle_9.9.9_amd64.deb",
                "opencircle-9.9.9-1.x86_64.rpm",
                SUMS,
                SIG,
            ]
            .map(asset)
            .into(),
        }
    }

    /// A fake GitHub serving `release` with good files; `edit` changes them.
    pub fn server(release: &Release, sig: &str) -> Fake {
        let mut files = HashMap::new();
        files.insert(
            LATEST.to_string(),
            (200, serde_json::to_vec(&json(release)).unwrap()),
        );
        for (name, body) in [
            ("OpenCircle_9.9.9_x64-portable.exe", EXE),
            ("OpenCircle_9.9.9_x86_64.AppImage", APPIMAGE),
            (SUMS, v::SUMS.as_bytes()),
            (SIG, sig.as_bytes()),
        ] {
            files.insert(format!("{BASE}{name}"), (200, body.to_vec()));
        }
        Fake {
            files,
            ..Fake::default()
        }
    }

    fn json(r: &Release) -> serde_json::Value {
        serde_json::json!({
            "tag_name": r.tag_name, "body": r.body, "html_url": r.html_url,
            "draft": r.draft, "prerelease": r.prerelease,
            "assets": r.assets.iter().map(|a| serde_json::json!({
                "name": a.name, "browser_download_url": a.browser_download_url, "size": a.size,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oc-update-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn versions_compare_as_numbers_and_skip_drafts_prereleases_and_odd_tags() {
        assert_eq!(version("v1.2.3"), Some([1, 2, 3]));
        assert_eq!(version("0.10.0"), Some([0, 10, 0]));
        for bad in [
            "1.2",
            "1.2.3.4",
            "v1.2.3-beta",
            "1.x.3",
            "",
            "v",
            "1..3",
            "+1.2.3",
        ] {
            assert_eq!(version(bad), None, "{bad}");
        }
        let mut r = release("v0.10.0");
        assert!(is_newer(&r, "0.9.9"), "10 > 9 as numbers, not text");
        assert!(!is_newer(&r, "0.10.0"));
        assert!(!is_newer(&r, "0.11.0"));
        assert!(!is_newer(&r, "garbage"));
        r.prerelease = true;
        assert!(!is_newer(&r, "0.9.9"));
        r.prerelease = false;
        r.draft = true;
        assert!(!is_newer(&r, "0.9.9"));
    }

    #[test]
    fn each_install_kind_gets_its_own_asset_or_none() {
        let r = release("v9.9.9");
        let name = |i: &Install, arch| pick(&r.assets, i, arch).map(|a| a.name.as_str());
        let exe = Install::Exe("C:/x/opencircle.exe".into());
        let app = Install::AppImage("/home/u/OpenCircle.AppImage".into());
        assert_eq!(
            name(&exe, "x86_64"),
            Some("OpenCircle_9.9.9_x64-portable.exe")
        );
        assert_eq!(
            name(&app, "x86_64"),
            Some("OpenCircle_9.9.9_x86_64.AppImage")
        );
        assert_eq!(
            name(&app, "aarch64"),
            None,
            "no arm build: no wrong-arch install"
        );
        assert_eq!(name(&exe, "aarch64"), None);
        assert_eq!(
            name(&Install::Manual, "x86_64"),
            None,
            "deb / rpm: notify only"
        );
    }

    #[test]
    fn check_sends_the_etag_and_reuses_the_answer_on_304() {
        let mut fake = server(&release("v9.9.9"), v::SIG);
        fake.etag = Some("\"abc\"".into());
        let mut memo = Memo::default();
        let first = latest(&fake, LATEST, &mut memo).unwrap();
        let second = latest(&fake, LATEST, &mut memo).unwrap();
        assert_eq!(first, second);
        let asked = fake.asked.lock().unwrap();
        assert_eq!(asked[0].1, None);
        assert_eq!(asked[1].1.as_deref(), Some("\"abc\""));
    }

    #[test]
    fn rate_limit_waits_without_asking_again() {
        let mut fake = server(&release("v9.9.9"), v::SIG);
        fake.files.get_mut(LATEST).unwrap().0 = 403;
        let mut memo = Memo::default();
        assert_eq!(latest(&fake, LATEST, &mut memo), Err(CheckError::Wait(120)));
        assert!(matches!(
            latest(&fake, LATEST, &mut memo),
            Err(CheckError::Wait(_))
        ));
        assert_eq!(
            fake.asked.lock().unwrap().len(),
            1,
            "the second check stayed home"
        );
        fake.files.get_mut(LATEST).unwrap().0 = 500;
        assert!(matches!(
            latest(&fake, LATEST, &mut Memo::default()),
            Err(CheckError::Failed(_))
        ));
    }

    #[test]
    fn a_signed_matching_download_is_staged() {
        let r = release("v9.9.9");
        let fake = server(&r, v::SIG);
        let dir = temp("good");
        let exe = Install::Exe("C:/x/opencircle.exe".into());
        let s = stage(&fake, &r, &exe, "x86_64", &dir, TEST_KEY).unwrap();
        assert_eq!(std::fs::read(&s.file).unwrap(), EXE);
        assert_eq!(s.version, "9.9.9");
        assert_eq!(s.notes, "## New\n- faster");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_signature_or_checksum_installs_nothing_and_leaves_no_files() {
        let r = release("v9.9.9");
        let app = Install::AppImage("/x/OpenCircle.AppImage".into());
        type Edit = Box<dyn Fn(&mut Fake)>;
        let cases: [(&str, Edit, &str); 5] = [
            ("wrongkey", Box::new(|_| {}), WRONG_KEY),
            ("nokey", Box::new(|_| {}), ""),
            (
                "tampered",
                Box::new(|f| {
                    let sums = v::SUMS.replace("7648", "7649");
                    f.files.get_mut(&format!("{BASE}{SUMS}")).unwrap().1 = sums.into_bytes();
                }),
                TEST_KEY,
            ),
            (
                "nosig",
                Box::new(|f| {
                    f.files.remove(&format!("{BASE}{SIG}"));
                }),
                TEST_KEY,
            ),
            (
                "mismatch",
                Box::new(|f| {
                    let url = format!("{BASE}OpenCircle_9.9.9_x86_64.AppImage");
                    f.files.get_mut(&url).unwrap().1 = b"evil".to_vec();
                }),
                TEST_KEY,
            ),
        ];
        for (name, edit, key) in cases {
            let mut fake = server(&r, v::SIG);
            edit(&mut fake);
            let dir = temp(name);
            assert!(
                stage(&fake, &r, &app, "x86_64", &dir, key).is_err(),
                "{name}"
            );
            assert!(!dir.exists(), "{name}: temp dir deleted");
        }
        let mut unsigned = r.clone();
        unsigned.assets.retain(|a| a.name != SIG);
        let dir = temp("unlisted");
        let err = stage(
            &server(&r, v::SIG),
            &unsigned,
            &app,
            "x86_64",
            &dir,
            TEST_KEY,
        );
        assert_eq!(err, Err("the release is not signed".into()));
    }

    #[cfg(unix)]
    #[test]
    fn a_staging_dir_someone_else_made_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let r = release("v9.9.9");
        let app = Install::AppImage("/x/OpenCircle.AppImage".into());
        let fake = server(&r, v::SIG);
        // a symlink to somewhere else
        let base = temp("planted");
        std::fs::create_dir_all(base.join("elsewhere")).unwrap();
        let dir = base.join("update");
        std::os::unix::fs::symlink(base.join("elsewhere"), &dir).unwrap();
        let err = stage(&fake, &r, &app, "x86_64", &dir, TEST_KEY).unwrap_err();
        assert!(err.contains("not OpenCircle's own"), "{err}");
        assert!(
            std::fs::symlink_metadata(&dir).is_ok(),
            "left alone, not followed"
        );
        assert_eq!(
            std::fs::read_dir(base.join("elsewhere")).unwrap().count(),
            0
        );
        // a folder others may write in
        std::fs::remove_file(&dir).unwrap();
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(stage(&fake, &r, &app, "x86_64", &dir, TEST_KEY).is_err());
        // our own leftover: replaced by a fresh private folder
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(dir.join("stale"), b"x").unwrap();
        let s = stage(&fake, &r, &app, "x86_64", &dir, TEST_KEY).unwrap();
        assert!(!dir.join("stale").exists());
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(s.sha256, verify::sha256(APPIMAGE).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scrub_drops_url_queries() {
        assert_eq!(
            scrub("io: https://objects.example/x.exe?X-Amz-Signature=secret failed"),
            "io: https://objects.example/x.exe failed"
        );
    }
}
