use std::sync::{Arc, Mutex};

use super::*;
use crate::features::text_reading::{Answer, LOCAL_FAKE, fake_server, payload};

const MB: u64 = 1 << 20;

fn card(discrete: bool, free_mb: u64) -> Gpu {
    Gpu {
        discrete,
        name: format!("card {free_mb}"),
        free: free_mb * MB,
        total: 16_000 * MB,
    }
}

/// The owner's rule: a discrete card with free video memory >= model + ~1 GB;
/// an integrated one never; the most free wins.
#[test]
fn the_gpu_rule_needs_a_discrete_card_with_the_model_plus_a_gigabyte() {
    let need = MODEL.size + HEADROOM; // ~2105 MB
    let just = Gpu {
        free: need,
        ..card(true, 0)
    };
    assert!(fits(std::slice::from_ref(&just), MODEL.size).is_some());
    let short = Gpu {
        free: need - 1,
        ..card(true, 0)
    };
    assert!(fits(&[short], MODEL.size).is_none());
    assert!(fits(&[card(false, 16_000)], MODEL.size).is_none());
    assert!(fits(&[], MODEL.size).is_none());
    let cards = [card(true, 3000), card(false, 9000), card(true, 8000)];
    let best = fits(&cards, MODEL.size);
    assert_eq!(best.map(|g| g.free), Some(8000 * MB));
}

#[test]
fn the_model_is_pinned_like_the_other_models() {
    assert_eq!(MODEL.sha256.len(), 64);
    assert!(
        MODEL
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(MODEL.size, 1_133_080_448);
    assert!(
        MODEL
            .url()
            .starts_with("https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/")
    );
    assert!(MODEL.url().ends_with(MODEL.name));
}

fn signed(size: u64) -> SignedAsset {
    SignedAsset {
        url: "https://example.test/helper".into(),
        size,
        sha256: [7; 32],
    }
}

/// Off, no card, a card without room: the CPU path, and nothing asked of the
/// network. A fitting card: the download is the model plus the helper of
/// this version's release (read from its signed checksums only then).
#[test]
fn the_plan_downloads_nothing_unless_a_card_fits() {
    let dir = crate::shared::paths::test_dir("gpu-plan");
    let never = |_: &str| -> Result<SignedAsset, String> { panic!("asked the release") };
    let no_probe = || -> Result<Vec<Gpu>, String> { panic!("probed") };
    let off = plan_with(LiveGpu::Off, no_probe, &dir, never);
    assert_eq!(off.unwrap_err(), "GPU for live translation is off");
    let igpu = plan_with(LiveGpu::Auto, || Ok(vec![card(false, 9000)]), &dir, never);
    assert!(
        igpu.unwrap_err()
            .contains("integrated, 9000 of 16000 MB free")
    );
    let none = plan_with(LiveGpu::Auto, || Ok(vec![]), &dir, never);
    assert!(none.unwrap_err().contains("no graphics card"));
    let failed = plan_with(LiveGpu::Auto, || Err("helper stopped".into()), &dir, never);
    assert!(failed.unwrap_err().contains("helper stopped"));
    if HELPER.is_none() {
        return; // (no GPU path on this system)
    }
    let p = plan_with(
        LiveGpu::Auto,
        || Ok(vec![card(true, 8000)]),
        &dir,
        |name| {
            assert_eq!(Some(name), HELPER);
            Ok(signed(45 * MB))
        },
    )
    .unwrap();
    assert_eq!(
        (p.card.as_str(), p.download),
        ("card 8000", MODEL.size + 45 * MB)
    );
    // this version's helper already here: the release isn't asked again
    std::fs::write(helper_path(&dir), b"x").unwrap();
    let p = plan_with(LiveGpu::Auto, || Ok(vec![card(true, 8000)]), &dir, never).unwrap();
    assert_eq!((p.download, p.helper), (MODEL.size, None));
    // no release for this version (a development build): the CPU path
    std::fs::remove_file(helper_path(&dir)).unwrap();
    let dev = plan_with(
        LiveGpu::Auto,
        || Ok(vec![card(true, 8000)]),
        &dir,
        |_| Err("not found".into()),
    );
    assert!(dev.unwrap_err().contains("no GPU helper"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_helpers_probe_reply_is_read() {
    let reply = json!({"ok": true, "gpu": true, "devices": [
        {"index": 0, "kind": "discrete", "name": "RTX", "free_mb": 22000, "total_mb": 24576},
        {"index": 1, "kind": "integrated", "name": "UHD", "free_mb": 4000, "total_mb": 8000},
    ], "fits": true, "device": 0});
    let gpus = probed(&reply).unwrap();
    assert_eq!(
        gpus[0],
        Gpu {
            discrete: true,
            name: "RTX".into(),
            free: 22000 * MB,
            total: 24576 * MB
        }
    );
    assert!(!gpus[1].discrete);
    assert!(
        probed(&json!({"ok": false, "error": "backend init: no"}))
            .unwrap_err()
            .contains("no")
    );
    assert!(probed(&json!({"ok": true})).is_err());
}

/// The back-off: the median of the first lines is the baseline; three lines
/// in a row over twice it = slowed (for good); a normal line resets the count.
#[test]
fn three_lines_twice_as_slow_in_a_row_fall_back() {
    let mut p = Pace::default();
    for ms in [12.0, 10.0, 30.0, 9.0, 11.0] {
        p.note(ms); // baseline 11
    }
    for ms in [23.0, 23.0, 11.0, 23.0, 23.0, 22.0] {
        p.note(ms); // 22 = exactly 2x: not slow
    }
    assert!(!p.slowed);
    p.note(23.0);
    p.note(23.0);
    assert!(!p.slowed, "two in a row");
    p.note(40.0);
    assert!(p.slowed);
    p.note(5.0);
    assert!(p.slowed, "slowed stays");
    // warm-up lines never count
    let mut p = Pace::default();
    (0..4).for_each(|_| p.note(500.0));
    assert!(!p.slowed);
}

/// A helper stand-in: answers by `op`, records every request; `die` makes
/// the next request fail like a crashed helper.
#[derive(Clone, Default)]
struct Fake {
    seen: Arc<Mutex<Vec<Value>>>,
    die: Arc<Mutex<bool>>,
    free_mb: u64,
}

impl Fake {
    fn call(&self) -> Call {
        let f = self.clone();
        Box::new(move |req: &Value| {
            f.seen.lock().unwrap().push(req.clone());
            if std::mem::take(&mut *f.die.lock().unwrap()) {
                return Err("helper stopped (exit status: 1)".into());
            }
            Ok(match req["op"].as_str() {
                Some("probe") => json!({"ok": true, "devices": [
                    {"kind": "discrete", "name": "Fake GPU", "free_mb": f.free_mb, "total_mb": 24576}]}),
                Some("load") => json!({"ok": true, "backend": "gpu", "load_ms": 900}),
                Some("translate") if req["text"] == "too long" => {
                    json!({"ok": false, "error": "prompt does not fit"})
                }
                Some("translate") => {
                    json!({"ok": true, "text": format!("<{}>", req["text"].as_str().unwrap()),
                    "gen_ms": 50.0, "gen_tokens": 10})
                }
                _ => json!({"ok": false, "error": "bad request"}),
            })
        })
    }

    fn ops(&self) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .map(|r| r["op"].as_str().unwrap().to_string())
            .collect()
    }
}

fn translator(fake: &Fake) -> GpuTranslator {
    let mut t = GpuTranslator {
        call: fake.call(),
        model: "/m.gguf".into(),
        target: "ar".into(),
        card: "Fake GPU".into(),
        loaded: false,
        pace: Pace::default(),
    };
    t.load().unwrap();
    t
}

/// Lines go with their glossary terms; a helper that died is started again
/// by the session and gets the model again before the next line; a refused
/// line (the helper is fine) doesn't reload.
#[test]
fn a_helper_that_died_gets_the_model_again_before_the_next_line() {
    let fake = Fake::default();
    let mut t = translator(&fake);
    let terms = vec![("Ronova".to_string(), "رونوفا".to_string())];
    assert_eq!(t.line("Hi Ronova", terms).unwrap(), "<Hi Ronova>");
    let sent = fake.seen.lock().unwrap()[1].clone();
    assert_eq!(sent["glossary"], json!([["Ronova", "رونوفا"]]));
    assert_eq!(sent["target"], "ar");
    assert!(
        t.line("too long", vec![])
            .unwrap_err()
            .contains("does not fit")
    );
    *fake.die.lock().unwrap() = true;
    assert!(t.line("a", vec![]).is_err());
    assert_eq!(t.line("b", vec![]).unwrap(), "<b>");
    let ops = fake.ops();
    assert_eq!(
        ops,
        [
            "load",
            "translate",
            "translate",
            "translate",
            "load",
            "translate"
        ]
    );
    assert!(!t.slowed());
}

/// The glossary's terms of a line, each once, kept terms as themselves.
#[test]
fn a_lines_glossary_terms_go_as_pairs() {
    let g = crate::features::translate::glossary::store::parse(
        "name = \"G\"\n[[term]]\ntext = \"Ronova\"\nar = \"رونوفا\"\n[[term]]\ntext = \"Stellaron\"\n",
    )
    .unwrap();
    let pairs = g.term_pairs("Ronova saw ronova and a Stellaron", "ar");
    assert_eq!(
        pairs,
        [
            ("Ronova".into(), "رونوفا".into()),
            ("Stellaron".into(), "Stellaron".into())
        ]
    );
    assert!(g.term_pairs("nothing here", "ar").is_empty());
}

/// The whole download path against a local server (the update gate's way):
/// the helper of this version (pin from its signed release entry), the
/// helper's own probe, then the model (a cut connection resumes), the load.
/// A card the helper says has no room: the model is never fetched.
#[test]
fn start_downloads_the_helper_then_the_model_only_when_the_helper_agrees() {
    LOCAL_FAKE.set(true);
    let dir = crate::shared::paths::test_dir("gpu-start");
    let old = dir.join("opencircle-mt-gpu-0.0.1");
    std::fs::write(&old, b"old helper").unwrap();
    let helper_bytes: &'static [u8] = b"#!/bin/sh\nexit 0\n";
    let (h, _) = fake_server("helper", helper_bytes, vec![Answer::File]);
    let asset = SignedAsset {
        url: h.url().into(),
        size: h.size,
        sha256: crate::shared::update::verify::sha256(helper_bytes).unwrap(),
    };
    let data = payload();
    let (m, seen) = fake_server("model.gguf", data, vec![Answer::Cut, Answer::File]);
    let plan = Plan {
        card: "x".into(),
        download: m.size + h.size,
        helper: Some(asset.clone()),
    };
    // the helper says the card has no room now: no model download at all
    let tight = Fake {
        free_mb: 900,
        ..Fake::default()
    };
    let model = (dir.join(m.name), m);
    let got = start_with(
        plan.clone(),
        &dir,
        model.clone(),
        "ar",
        |_, _| {},
        |_| tight.call(),
    );
    assert!(got.err().unwrap().contains("900 of 24576 MB free"));
    assert!(seen.lock().unwrap().is_empty() && !model.0.exists());
    assert_eq!(std::fs::read(helper_path(&dir)).unwrap(), helper_bytes);
    assert!(!old.exists(), "an older version's helper is removed");

    let fake = Fake {
        free_mb: 22000,
        ..Fake::default()
    };
    let last = Arc::new(Mutex::new((0, 0)));
    let l = last.clone();
    let spawned = Arc::new(Mutex::new(None));
    let s = spawned.clone();
    let plan = Plan {
        helper: None,
        download: m.size,
        ..plan
    };
    let t = start_with(
        plan,
        &dir,
        model.clone(),
        "ar",
        move |d, n| *l.lock().unwrap() = (d, n),
        |p| {
            *s.lock().unwrap() = Some(p.to_path_buf());
            fake.call()
        },
    )
    .unwrap();
    assert_eq!(t.card, "Fake GPU");
    assert_eq!(std::fs::read(&model.0).unwrap(), data);
    assert_eq!(
        *seen.lock().unwrap(),
        [None, Some(data.len() as u64 / 2)],
        "resumed"
    );
    assert_eq!(*last.lock().unwrap(), (m.size, m.size));
    assert_eq!(
        spawned.lock().unwrap().as_deref(),
        Some(helper_path(&dir).as_path())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(helper_path(&dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the helper may run");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
