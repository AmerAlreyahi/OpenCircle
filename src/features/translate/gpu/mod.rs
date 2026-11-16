//! Live translation on the graphics card (planning #138 / #139; owner's GPU
//! rule, 2026-11-09 / 11-14; live-approach.md 5.7): Hy-MT2-1.8B (4-bit GGUF,
//! Apache-2.0) in `opencircle-mt-gpu`, a separate helper exe (llama.cpp,
//! Vulkan; `gpu-helper/`), so `opencircle.exe` never links llama.cpp.
//!
//! [`live`] is the one call live translate makes at its start: `Some` = a
//! [`GpuTranslator`] (the live core's `Translator`), `None` = the CPU path
//! (today's models; the reason is logged). In order, each step only after
//! the one before passed, so a PC without a fitting card downloads nothing:
//! 1. Settings > Translate "GPU for live translation" is Auto (`translate.live_gpu`);
//! 2. the cards Vulkan sees (asked in the `ocr-worker` helper, before
//!    anything is downloaded) pass the GPU rule ([`fits`]);
//! 3. the helper of THIS version's release: its SHA-256 from the release's
//!    signed checksums (`update::signed_asset`, the updater's check), fetched
//!    by the model downloader (retries, resume, plain messages, progress);
//! 4. the helper's own probe (llama.cpp's numbers) passes the rule again;
//! 5. the model, pinned like `mt::PAIRS` ([`MODEL`]), same downloader;
//! 6. the helper loads it on the card.
//!
//! [`plan`] is steps 1-2 (+ what 3 and 5 would download, for a question
//! card) and [`start`] the rest, for a caller that asks before downloading.
//! While playing, [`GpuTranslator::slowed`] says when lines got ~2x slower
//! than at the start several times in a row: the caller goes back to the CPU.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::features::text_reading::{self, ModelFile, Session};
use crate::live_core::Translator;
use crate::shared::config::{self, LiveGpu};
use crate::shared::platform::vulkan::Gpu;
use crate::shared::update::{self, SignedAsset};

#[cfg(test)]
mod tests;

/// Hy-MT2-1.8B Q4_K_M (tencent/Hy-MT2-1.8B-GGUF, Apache-2.0), unchanged,
/// re-hosted on OpenCircle-models (docs/internal/reports-0.3.0/w9-model-release).
pub(crate) const MODEL: ModelFile = ModelFile::plain(
    "Hy-MT2-1.8B-Q4_K_M.gguf",
    1_133_080_448,
    "dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699",
    "https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/models-v4/Hy-MT2-1.8B-Q4_K_M.gguf",
);

/// Free video memory the rule keeps beside the model (owner: model + ~1 GB).
const HEADROOM: u64 = 1 << 30;

/// This system's helper, as release.yml names the asset; none = no GPU path.
const HELPER: Option<&str> = if cfg!(all(windows, target_arch = "x86_64")) {
    Some("opencircle-mt-gpu-windows-x86_64.exe")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("opencircle-mt-gpu-linux-x86_64")
} else {
    None
};

/// Per request; the first `load` reads 1.1 GB from disk onto the card.
const TIMEOUT: Duration = Duration::from_secs(30);
const N_CTX: u32 = 1024;

/// The GPU rule: the discrete card with the most free video memory, when
/// that is at least `model` + `HEADROOM`. An integrated one never counts.
pub fn fits(gpus: &[Gpu], model: u64) -> Option<&Gpu> {
    (gpus.iter())
        .filter(|g| g.discrete && g.free >= model.saturating_add(HEADROOM))
        .max_by_key(|g| g.free)
}

/// What the last [`plan`] / [`start`] decided (Settings' status line):
/// `Some(card)` = live translate runs on it, `None` = on the processor.
static LAST: Mutex<Option<Option<String>>> = Mutex::new(None);

/// Settings > Translate's line: unknown until live translate has started once.
pub fn last_choice() -> Option<Option<String>> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn decided(card: Option<&str>) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(card.map(Into::into));
}

/// Steps 1-2 passed: the card, and the bytes [`start`] would download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub card: String,
    pub download: u64,
    /// The helper's signed release entry, when it isn't on disk yet.
    helper: Option<SignedAsset>,
}

/// Steps 1-2 (no download; the release's checksums are read when the helper
/// is missing, which also proves this version has one). `Err` = the CPU path, why.
pub fn plan() -> Result<Plan, String> {
    let plan = plan_with(
        config::current().translate.live_gpu,
        || text_reading::gpus_in_child().map_err(|e| e.to_string()),
        &dir(),
        update::signed_asset,
    );
    if plan.is_err() {
        decided(None);
    }
    plan
}

fn plan_with(
    setting: LiveGpu,
    gpus: impl FnOnce() -> Result<Vec<Gpu>, String>,
    dir: &Path,
    signed: impl FnOnce(&str) -> Result<SignedAsset, String>,
) -> Result<Plan, String> {
    if setting == LiveGpu::Off {
        return Err("GPU for live translation is off".into());
    }
    let asset = HELPER.ok_or("no GPU helper for this system")?;
    let gpus = gpus().map_err(|e| format!("the graphics card check failed: {e}"))?;
    let card = fits(&gpus, MODEL.size).ok_or_else(|| no_fit(&gpus))?;
    let helper = match helper_path(dir).is_file() {
        true => None,
        false => Some(signed(asset).map_err(|e| format!("no GPU helper for {VERSION}: {e}"))?),
    };
    let model = text_reading::missing_bytes(&[(dir.join(MODEL.name), &MODEL)]);
    Ok(Plan {
        card: card.name.clone(),
        download: model + helper.as_ref().map_or(0, |h| h.size),
        helper,
    })
}

/// Why no card passed the rule (the log).
fn no_fit(gpus: &[Gpu]) -> String {
    if gpus.is_empty() {
        return "no graphics card with a Vulkan driver".into();
    }
    let list: Vec<String> = (gpus.iter())
        .map(|g| {
            let kind = if g.discrete { "discrete" } else { "integrated" };
            format!(
                "{} ({kind}, {} of {} MB free)",
                g.name,
                g.free >> 20,
                g.total >> 20
            )
        })
        .collect();
    format!(
        "no discrete card with {} MB free: {}",
        (MODEL.size + HEADROOM) >> 20,
        list.join(", ")
    )
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `<data>/OpenCircle/gpu`.
fn dir() -> PathBuf {
    crate::shared::paths::gpu()
}

/// The helper of this version (a new app version brings its own: the
/// protocol may change with it).
fn helper_path(dir: &Path) -> PathBuf {
    dir.join(format!(
        "opencircle-mt-gpu-{VERSION}{}",
        std::env::consts::EXE_SUFFIX
    ))
}

/// Steps 3-6 for `plan` (downloads only what is missing; `progress(done,
/// total)` over both files).
pub fn start(
    plan: Plan,
    target: &str,
    progress: impl FnMut(u64, u64),
) -> Result<GpuTranslator, String> {
    let dir = dir();
    let model = (dir.join(MODEL.name), &MODEL);
    let got = start_with(plan, &dir, model, target, progress, |path| {
        let mut session = Session::new(path, &[], TIMEOUT);
        Box::new(move |req: &Value| {
            let reply = session.call(&[req.to_string().as_bytes()]);
            HELPER_PID.store(session.pid().unwrap_or(0), Ordering::Relaxed);
            let reply = reply.map_err(|e| e.to_string())?;
            serde_json::from_slice(&reply).map_err(|e| format!("helper's reply: {e}"))
        })
    });
    decided(got.as_ref().ok().map(|t| t.card.as_str()));
    got
}

/// The running GPU helper's process (0 = none): live translate's CPU back-off leaves it out of
/// "other processes", like the OCR helper.
static HELPER_PID: AtomicU32 = AtomicU32::new(0);

pub fn helper_pid() -> Option<u32> {
    Some(HELPER_PID.load(Ordering::Relaxed)).filter(|&p| p != 0)
}

/// The test hook that turns the GPU path on (it is off otherwise; owner, 2026-11-16).
pub const TEST_HOOK: &str = "OPENCIRCLE_TEST_GPU";

/// The one call (module doc): `None` = the CPU path, the reason logged.
pub fn live(target: &str, progress: impl FnMut(u64, u64)) -> Option<GpuTranslator> {
    let got = plan().and_then(|p| start(p, target, progress));
    match &got {
        Ok(t) => crate::log!(
            "[opencircle] live translate on the graphics card: {}",
            t.card
        ),
        Err(why) => crate::log!("[opencircle] live translate on the processor: {why}"),
    }
    got.ok()
}

/// One helper request (JSON) and its reply; `Err` = the helper died, hung
/// or answered nonsense (the session starts a new one on the next call).
pub(crate) type Call = Box<dyn FnMut(&Value) -> Result<Value, String> + Send>;

pub(crate) fn start_with(
    plan: Plan,
    dir: &Path,
    model: (PathBuf, &'static ModelFile),
    target: &str,
    mut progress: impl FnMut(u64, u64),
    spawn: impl FnOnce(&Path) -> Call,
) -> Result<GpuTranslator, String> {
    let total = plan.download;
    let helper = helper_path(dir);
    let mut done = 0;
    if let Some(asset) = &plan.helper {
        let file = helper_file(asset, helper.file_name().and_then(|n| n.to_str()));
        text_reading::ensure(&[(helper.clone(), file)], |n, _| progress(n, total))
            .map_err(|e| e.to_string())?;
        done = asset.size;
        forget_old_helpers(dir, &helper);
    }
    executable(&helper).map_err(|e| format!("the GPU helper: {e}"))?;
    let mut call = spawn(&helper);
    // the helper's own view of the cards (what llama.cpp will use) passes the rule too
    let probe = call(&json!({"op": "probe", "model_bytes": model.1.size}))?;
    let gpus = probed(&probe)?;
    let card = fits(&gpus, model.1.size)
        .ok_or_else(|| no_fit(&gpus))?
        .name
        .clone();
    text_reading::ensure(std::slice::from_ref(&model), |n, _| {
        progress(done + n, total)
    })
    .map_err(|e| e.to_string())?;
    let mut t = GpuTranslator {
        call,
        model: model.0,
        target: target.into(),
        card,
        loaded: false,
        pace: Pace::default(),
    };
    t.load()?;
    Ok(t)
}

/// The downloader's pin for the helper, from its signed release entry.
// ponytail: leaks one small pin per download (once per app version).
fn helper_file(asset: &SignedAsset, name: Option<&str>) -> &'static ModelFile {
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let hex: String = asset.sha256.iter().map(|b| format!("{b:02x}")).collect();
    let name = leak(name.unwrap_or("opencircle-mt-gpu").into());
    let file = ModelFile::plain(name, asset.size, leak(hex), leak(asset.url.clone()));
    Box::leak(Box::new(file))
}

/// An older version's helper goes once this one's is here.
fn forget_old_helpers(dir: &Path, keep: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("opencircle-mt-gpu-") && e.path() != keep {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Unix: the downloaded helper may run (the downloader writes 0644).
fn executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(path)?.permissions();
        if perm.mode() & 0o100 == 0 {
            perm.set_mode(0o755);
            std::fs::set_permissions(path, perm)?;
        }
    }
    #[cfg(not(unix))]
    std::fs::metadata(path)?;
    Ok(())
}

/// The cards in the helper's `probe` reply.
fn probed(reply: &Value) -> Result<Vec<Gpu>, String> {
    ok(reply)?;
    let list = reply["devices"].as_array().ok_or("probe: no device list")?;
    Ok(list
        .iter()
        .map(|d| Gpu {
            discrete: d["kind"] == "discrete",
            name: d["name"].as_str().unwrap_or_default().into(),
            free: d["free_mb"].as_u64().unwrap_or(0) << 20,
            total: d["total_mb"].as_u64().unwrap_or(0) << 20,
        })
        .collect())
}

/// A reply's `"ok": false` as its error.
fn ok(reply: &Value) -> Result<(), String> {
    match reply["ok"].as_bool() {
        Some(true) => Ok(()),
        _ => Err((reply["error"].as_str())
            .unwrap_or("the GPU helper refused")
            .into()),
    }
}

/// The live core's translator on the graphics card: one line at a time into
/// `target`, with the glossary's terms of that line (game screens only:
/// `glossary::store::current`).
pub struct GpuTranslator {
    call: Call,
    model: PathBuf,
    target: String,
    /// The card it runs on (Settings, the log).
    pub card: String,
    /// The helper has the model loaded (a new helper after a failure hasn't).
    loaded: bool,
    pace: Pace,
}

impl GpuTranslator {
    fn load(&mut self) -> Result<(), String> {
        let path = self.model.to_string_lossy();
        let req = json!({"op": "load", "path": path, "n_ctx": N_CTX, "gpu": true});
        let reply = (self.call)(&req)?;
        ok(&reply)?;
        if reply["backend"] != "gpu" {
            return Err("the helper loaded the model on the processor".into());
        }
        self.loaded = true;
        Ok(())
    }

    /// Into `target` from the next line on (the model knows every language Live offers).
    pub fn set_target(&mut self, target: &str) {
        self.target = target.into();
    }

    /// Lines have been ~2x slower than at the start several times in a row
    /// (the GPU rule's back-off): the caller goes back to the CPU path.
    pub fn slowed(&self) -> bool {
        self.pace.slowed
    }

    /// `text` into the target, `terms` = the glossary's (term, what it becomes).
    fn line(&mut self, text: &str, terms: Vec<(String, String)>) -> Result<String, String> {
        if !self.loaded {
            self.load()?;
        }
        let req =
            json!({"op": "translate", "text": text, "target": self.target, "glossary": terms});
        let reply = (self.call)(&req).inspect_err(|_| self.loaded = false)?;
        ok(&reply)?;
        let tokens = reply["gen_tokens"].as_f64().unwrap_or(0.0);
        if tokens > 0.0 {
            self.pace
                .note(reply["gen_ms"].as_f64().unwrap_or(0.0) / tokens);
        }
        Ok(reply["text"].as_str().unwrap_or_default().to_string())
    }
}

impl Drop for GpuTranslator {
    fn drop(&mut self) {
        HELPER_PID.store(0, Ordering::Relaxed); // (its session, and so the helper, ends with it)
    }
}

impl Translator for GpuTranslator {
    fn translate(&mut self, text: &str) -> Result<String, String> {
        let glossary = super::glossary::store::current().unwrap_or_default();
        // a line that IS a term needs no model
        if let Some(whole) = glossary.whole(text, &self.target) {
            return Ok(whole);
        }
        let terms = glossary.term_pairs(text, &self.target);
        self.line(text, terms)
    }
}

/// The back-off (owner's GPU rule): each line's time per generated token
/// against the median of the first `WARM_UP`; `IN_A_ROW` lines over `SLOW`x
/// that in a row = slowed (for good: the caller moves to the CPU path).
#[derive(Debug, Default)]
pub(crate) struct Pace {
    first: Vec<f64>,
    baseline: Option<f64>,
    slow: u8,
    slowed: bool,
}

const WARM_UP: usize = 5;
const SLOW: f64 = 2.0;
const IN_A_ROW: u8 = 3;

impl Pace {
    pub(crate) fn note(&mut self, ms_per_token: f64) {
        let Some(base) = self.baseline else {
            self.first.push(ms_per_token);
            if self.first.len() == WARM_UP {
                self.first.sort_by(f64::total_cmp);
                self.baseline = Some(self.first[WARM_UP / 2]);
            }
            return;
        };
        self.slow = match ms_per_token > SLOW * base {
            true => self.slow.saturating_add(1),
            false => 0,
        };
        self.slowed |= self.slow >= IN_A_ROW;
    }
}

/// A test's helper process (0 = none).
#[cfg(test)]
pub(crate) fn pretend_helper(pid: u32) {
    HELPER_PID.store(pid, Ordering::Relaxed);
}
