use super::*;

/// A fake decoder: plays `script` (per step, one token per row still
/// running) and checks it is fed back the previous tokens, and which
/// rows of the last step are kept.
fn scripted(
    script: Vec<(Vec<usize>, Vec<u32>)>,
    vocab: usize,
) -> impl FnMut(&[i64], &[usize]) -> Result<Vec<f32>, OcrError> {
    let mut step = 0;
    let mut prev: Option<Vec<i64>> = None;
    move |tokens: &[i64], keep: &[usize]| {
        let (kept, row) = &script[step.min(script.len() - 1)];
        match &prev {
            None => assert!(
                tokens.iter().all(|&t| t == 9),
                "starts with pad: {tokens:?}"
            ),
            Some(p) => {
                let fed: Vec<i64> = kept.iter().map(|&k| p[k]).collect();
                assert_eq!(tokens, fed.as_slice(), "fed back");
            }
        }
        if step > 0 {
            assert_eq!(keep, kept.as_slice(), "step {step}");
        }
        step += 1;
        let mut logits = vec![0.0; tokens.len() * vocab];
        for (r, &t) in row.iter().enumerate() {
            logits[r * vocab + t as usize] = 1.0;
            logits[r * vocab + 9] = 5.0; // pad scores highest: never chosen
        }
        prev = Some(row.iter().map(|&t| t as i64).collect());
        Ok(logits)
    }
}

#[test]
fn greedy_stops_rows_at_eos_and_all_at_max_len() {
    // rows 0, 1, 2: "3 4", "5", "6 7 8"; an ended row leaves the batch
    let script = vec![
        (vec![0, 1, 2], vec![3, 5, 6]),
        (vec![0, 1, 2], vec![4, 0, 7]),
        (vec![0, 2], vec![0, 8]),
        (vec![1], vec![0]),
    ];
    let out = greedy(3, 9, 0, 50, scripted(script, 10)).unwrap();
    assert_eq!(out, vec![vec![3, 4], vec![5], vec![6, 7, 8]]);
    // never ends: capped
    let out = greedy(1, 9, 0, 4, scripted(vec![(vec![0], vec![2])], 10)).unwrap();
    assert_eq!(out, vec![vec![2, 2, 2, 2]]);
    // a decoder that answers nothing is an error, not a panic
    assert!(greedy(2, 9, 0, 4, |_, _| Ok(vec![1.0])).is_err());
}

#[test]
fn pairs_by_language_code() {
    let codes = |p: Option<&Pair>| p.map(|p| (p.source, p.target));
    assert_eq!(codes(pair("en", "ar")), Some(("en", "ar")));
    assert_eq!(codes(pair("EN-us", "AR-sa")), Some(("en", "ar")));
    assert_eq!(codes(pair("ja", "en")), Some(("ja", "en")));
    // not available offline: no pair (the caller says so and offers AI)
    assert!(pair("fr", "ko").is_none() && pair("en", "zh-Hant").is_none());
    assert!(pair("", "ar").is_none() && pair(lang::AUTO, "ar").is_none());
    assert!(available(lang::AUTO, "fr") && available(lang::AUTO, "en"));
    assert!(!available(lang::AUTO, "ko") && !available("fr", "ko"));
    // through English: Italian -> en -> Arabic
    let via = route("it", "ar").unwrap();
    assert_eq!(
        via.iter().map(|p| (p.source, p.target)).collect::<Vec<_>>(),
        [("it", "en"), ("en", "ar")]
    );
    assert!(available("it", "ar") && route("pt", "ko").is_none());
    // our own conversions go direct, downloaded from the models release
    for (s, t) in [
        ("fr", "ar"),
        ("es", "ar"),
        ("ar", "fr"),
        ("ar", "es"),
        ("ar", "de"),
    ] {
        let r = route(s, t).unwrap();
        assert_eq!((r.len(), r[0].source, r[0].target), (1, s, t));
        let url = format!(
            "\"https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/\
                 models-v1/opus-mt-{s}-{t}-encoder_model_quantized.onnx\""
        );
        assert!(format!("{:?}", r[0].encoder).contains(&url), "{s}-{t}");
    }
    // German into Arabic stays through English (direct was worse)
    assert_eq!(route("de", "ar").unwrap().len(), 2);
    let why = unavailable("fr", "ja");
    assert!(why.contains("from French into Japanese") && why.contains("Choose AI"));
    let why = unavailable(lang::AUTO, "ko");
    assert!(
        why.starts_with("there is no local model into Korean yet"),
        "{why}"
    );
    let p = pair("en", "ar").unwrap();
    assert_eq!(p.label(), "English → Arabic local translation");
    let files = p.files();
    // OCR's files first (ORT), then the pair's, in its own folder
    assert_eq!(files[0].1.name, models::ORT.name);
    let own = files.iter().filter(|(path, _)| path.starts_with(p.dir()));
    assert_eq!(own.count(), 4);
    let total: u64 = files.iter().map(|(_, f)| f.size).sum();
    assert!(p.missing_bytes() <= total);
    // the pair reads its source with that language's recognizer
    let ar = pair("ar", "en").unwrap().files();
    assert!(
        ar.iter()
            .any(|(_, f)| f.name == "arabic_pp-ocrv5_mobile_rec.onnx")
    );
}

/// One entry per direction, each in its own folder, every language in
/// the language table; every file pinned (64 hex digits, a size).
#[test]
fn the_pair_table_is_consistent() {
    for (i, p) in PAIRS.iter().enumerate() {
        for code in [p.source, p.target] {
            assert_eq!(lang::find(code).map(|l| l.code), Some(code), "{code}");
        }
        assert_ne!(p.source, p.target);
        assert!(
            PAIRS[..i]
                .iter()
                .all(|o| (o.source, o.target) != (p.source, p.target))
        );
        assert!(PAIRS[..i].iter().all(|o| o.dir != p.dir));
        assert_eq!(p.dir, format!("{DIR_PREFIX}{}-{}", p.source, p.target));
        for f in [&p.encoder, &p.decoder, &p.spm, &p.vocab] {
            assert!(f.size > 0 && f.sha256.len() == 64, "{}/{}", p.dir, f.name);
            assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert_eq!(pair(p.source, p.target).map(|q| q.dir), Some(p.dir));
    }
    // what the consents say: en->ar alone is about 120 MB
    let p = pair("en", "ar").unwrap();
    let bytes: u64 = [&p.encoder, &p.decoder, &p.spm, &p.vocab]
        .map(|f| f.size)
        .iter()
        .sum();
    assert_eq!(bytes, 113_896_297);
}

/// Every pair's decoder is the lighter one, of another size than its v1
/// file (the two are told apart by size); encoders and tokenizers keep their
/// v1 pins. The pairs added after models-v2 (Portuguese) have no v1 file:
/// all four of their files are models-v3 assets.
#[test]
fn every_decoder_is_the_lighter_v2_one() {
    const V3: [(&str, &str); 2] = [("en", "pt"), ("pt", "en")];
    for p in &PAIRS {
        let v3 = V3.contains(&(p.source, p.target));
        let url = format!(
            "\"https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/\
                 models-{}/opus-mt-{}-{}-decoder_model_merged_quantized.onnx\"",
            if v3 { "v3" } else { "v2" },
            p.source,
            p.target
        );
        assert!(format!("{:?}", p.decoder).contains(&url), "{}", p.dir);
        let v1 = V1_DECODERS
            .iter()
            .find(|f| p.dir == format!("{DIR_PREFIX}{}", f.0));
        if v3 {
            assert!(v1.is_none(), "{}", p.dir);
            assert!(!p.outdated() && p.update_bytes() == 0, "{}", p.dir);
            for f in [&p.encoder, &p.spm, &p.vocab] {
                assert!(format!("{f:?}").contains("models-v3"), "{}", p.dir);
            }
            continue;
        }
        assert_ne!(p.decoder.size, v1.unwrap().1, "{}", p.dir);
        assert!(
            !format!("{:?}", p.encoder).contains("models-v2"),
            "{}",
            p.dir
        );
    }
}

/// Portuguese both ways with English; every other language reaches it
/// through English (two models); en->pt asks for Brazilian Portuguese.
#[test]
fn portuguese_routes_through_english() {
    let hops =
        |s, t| route(s, t).map(|r| r.iter().map(|p| (p.source, p.target)).collect::<Vec<_>>());
    assert_eq!(hops("pt", "en"), Some(vec![("pt", "en")]));
    assert_eq!(hops("en", "pt"), Some(vec![("en", "pt")]));
    assert_eq!(hops("pt", "ar"), Some(vec![("pt", "en"), ("en", "ar")]));
    assert_eq!(hops("ar", "pt"), Some(vec![("ar", "en"), ("en", "pt")]));
    assert_eq!(hops("pt", "fr"), Some(vec![("pt", "en"), ("en", "fr")]));
    assert_eq!(hops("pt", "ko"), None);
    assert!(available(lang::AUTO, "pt") && available("pt", "es"));
    assert_eq!(pair("en", "pt").unwrap().lang_token, Some(">>pt_br<<"));
    assert_eq!(pair("pt", "en").unwrap().lang_token, None);
}

/// A pinned file with `bytes` as its content.
fn pinned(name: &'static str, bytes: &[u8]) -> &'static ModelFile {
    let hash = crate::shared::update::verify::sha256(bytes).unwrap();
    let hash: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Box::leak(Box::new(ModelFile::plain(
        name,
        bytes.len() as u64,
        Box::leak(hash.into_boxed_str()),
        "",
    )))
}

/// An upgrading user never loses translation: a v1 decoder on disk is
/// used (nothing to download) while it verifies; a download of the pair
/// (another file missing, or a damaged v1) fetches v2, which replaces the
/// v1 file (one decoder on disk) and is then used; a failed download
/// keeps the v1 file and fails as before.
#[test]
fn a_v1_decoder_keeps_working_until_the_next_download() {
    models::LOCAL_FAKE.set(true); // no network: the URLs are empty
    let dir = std::env::temp_dir().join(format!("oc-mt-v1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (old, new) = (
        &b"the v1 decoder, two output layers"[..],
        &b"the v2 decoder"[..],
    );
    let (v1, v2) = (pinned("decoder.onnx", old), pinned("decoder.onnx", new));
    let vocab = pinned("vocab.json", b"{}");
    let (dec_path, vocab_path) = (dir.join("decoder.onnx"), dir.join("vocab.json"));
    let list = |ok: fn(&Path, &ModelFile) -> bool| {
        let files = vec![(dec_path.clone(), v2), (vocab_path.clone(), vocab)];
        without_v1(files, v2, Some(v1), ok)
    };
    let names = |l: Vec<(PathBuf, &ModelFile)>| l.iter().map(|(_, f)| f.name).collect::<Vec<_>>();
    let sizes: fn(&Path, &ModelFile) -> bool = |p, f| has_size(p, f.size);
    std::fs::write(&vocab_path, b"{}").unwrap();
    // v1 only on disk: used, nothing to download (sizes, then verified)
    std::fs::write(&dec_path, old).unwrap();
    assert_eq!(names(list(sizes)), ["vocab.json"]);
    assert_eq!(names(list(verified)), ["vocab.json"]);
    assert_eq!(models::missing_bytes(&list(sizes)), 0);
    models::ensure(&list(verified), |_, _| {}).unwrap();
    assert_eq!(std::fs::read(&dec_path).unwrap(), old, "kept");
    // a damaged v1 (right size) is fetched again, as v2
    std::fs::write(&dec_path, b"the v1 decoder, two output layerZ").unwrap();
    assert_eq!(names(list(verified)), ["decoder.onnx", "vocab.json"]);
    std::fs::write(&dec_path, old).unwrap();
    // another file missing: the download takes the v2 decoder too ...
    std::fs::remove_file(&vocab_path).unwrap();
    assert_eq!(models::missing_bytes(&list(sizes)), v2.size + vocab.size);
    // ... a failed one keeps the v1 file (no part file) ...
    assert!(matches!(
        models::ensure(&list(verified), |_, _| {}),
        Err(OcrError::Download(_))
    ));
    assert_eq!(std::fs::read(&dec_path).unwrap(), old);
    // ... a verified one writes v2 over it: one decoder on disk, v2 used
    models::write_verified(new, &dec_path, v2, |_| {}).unwrap();
    models::write_verified(&b"{}"[..], &vocab_path, vocab, |_| {}).unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert_eq!(std::fs::read(&dec_path).unwrap(), new);
    assert_eq!(names(list(verified)), ["decoder.onnx", "vocab.json"]);
    assert_eq!(models::missing_bytes(&list(sizes)), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A pair's decoder in a fresh folder: the v1 file on disk, v2 served
/// by a local server answering `answers` (`models::tests::fake_server`).
fn v1_install(
    name: &str,
    answers: Vec<models::tests::Answer>,
) -> (Decoder, PathBuf, models::tests::Seen) {
    models::LOCAL_FAKE.set(true);
    let dir = std::env::temp_dir().join(format!("oc-mt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let v1 = b"the v1 decoder, two output layers";
    let (v2, seen) = models::tests::fake_server("decoder.onnx", models::tests::payload(), answers);
    let path = dir.join("decoder.onnx");
    std::fs::write(&path, v1).unwrap();
    let decoder = Decoder {
        path,
        v2,
        v1: Some(v1.len() as u64),
    };
    (decoder, dir, seen)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = (std::fs::read_dir(dir).unwrap().flatten())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Nothing has it loaded: swapped at once.
fn idle(decoders: &[Decoder]) {
    decoders.iter().for_each(|d| _ = d.swap());
}

/// An install with a v1 decoder: the Translate use starts the update (a
/// server hiccup is retried), it is stopped part way (Cancel, or the app
/// killed: the same `.part` on disk) while v1 still translates, the next
/// one resumes from where it stopped (a Range), is verified and swapped
/// in by one rename: the v2 file in the v1 one's place, nothing else left;
/// the next Translate use downloads nothing.
#[test]
fn a_v1_install_is_updated_resumed_verified_and_swapped() {
    use models::tests::Answer;
    let data = models::tests::payload();
    let answers = vec![Answer::Status(504), Answer::File, Answer::File];
    let (d, dir, seen) = v1_install("update", answers);
    assert!(d.outdated() && d.to_fetch() == data.len() as u64);
    // stopped after the first bytes
    let got = update_decoders(
        std::slice::from_ref(&d),
        |done, _| {
            if done > 0 {
                stop_update();
            }
        },
        idle,
    );
    assert!(matches!(got, Err(OcrError::Stopped)), "{got:?}");
    assert!(d.outdated(), "v1 still in place, still translating");
    let part = std::fs::metadata(dir.join("decoder.v2.part"))
        .unwrap()
        .len();
    assert!(part > 0 && part < data.len() as u64, "{part}");
    assert_eq!(files_in(&dir), ["decoder.onnx", "decoder.v2.part"]);
    // the next use continues it
    update_decoders(std::slice::from_ref(&d), |_, _| {}, idle).unwrap();
    assert_eq!(*seen.lock().unwrap(), [None, None, Some(part)]);
    assert_eq!(std::fs::read(&d.path).unwrap(), data, "v2 in v1's place");
    assert_eq!(files_in(&dir), ["decoder.onnx"], "one copy, no temp");
    assert!(!d.outdated() && d.to_fetch() == 0);
    // the next Translate use: no download, no request
    update_decoders(std::slice::from_ref(&d), |_, _| {}, idle).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 3);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The decoder is loaded (a `Translator` lives): the finished download
/// waits beside it, the loaded file untouched (Windows' rule, enforced
/// here on every OS); once unloaded, the next swap puts it in place. A
/// failed update changes nothing. A stale `.v2.*` beside a current
/// decoder is cleaned.
#[test]
fn a_loaded_decoder_waits_for_its_swap() {
    use models::tests::Answer;
    let data = models::tests::payload();
    let (d, dir, _) = v1_install("loaded", vec![Answer::Status(404), Answer::File]);
    let v1 = std::fs::read(&d.path).unwrap();
    // a 404: nothing changes
    assert!(update_decoders(std::slice::from_ref(&d), |_, _| {}, idle).is_err());
    assert_eq!(files_in(&dir), ["decoder.onnx"]);
    let loaded = InUse::new(d.path.clone());
    update_decoders(std::slice::from_ref(&d), |_, _| {}, idle).unwrap();
    assert_eq!(std::fs::read(&d.path).unwrap(), v1, "the loaded file stays");
    assert_eq!(files_in(&dir), ["decoder.onnx", "decoder.v2.onnx"]);
    assert!(d.outdated() && d.to_fetch() == 0, "only the swap is left");
    assert!(!d.swap());
    drop(loaded);
    assert!(d.swap());
    assert_eq!(std::fs::read(&d.path).unwrap(), data);
    assert_eq!(files_in(&dir), ["decoder.onnx"]);
    std::fs::write(dir.join("decoder.v2.part"), b"x").unwrap();
    assert!(d.swap());
    assert_eq!(files_in(&dir), ["decoder.onnx"], "a crash's leftover goes");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A pair's folder in `root` named `name`, its files sparse at the pinned
/// sizes (`whole`) or only its encoder.
fn model_folder(root: &Path, name: &str, p: &Pair, whole: bool) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = vec![&p.encoder];
    if whole {
        files.extend([&p.decoder, &p.spm, &p.vocab]);
    }
    for f in files {
        std::fs::File::create(dir.join(f.name))
            .and_then(|file| file.set_len(f.size))
            .unwrap();
    }
    dir
}

fn mt_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oc-mt-fold-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn rename(old: &Path, new: &Path) -> std::io::Result<()> {
    std::fs::rename(old, new)
}

/// An old-named folder becomes the OptimaMT-named one: the same files
/// (a lighter-model update's `.part` too), still whole by the pins (so
/// the next Translate use downloads nothing), and a route through English
/// moves both of its pairs; the spelling is the Hugging Face repo's.
#[test]
fn old_model_folders_are_renamed_with_their_files() {
    let root = mt_root("rename");
    let (en_fr, fr_en) = (pair("en", "fr").unwrap(), pair("fr", "en").unwrap());
    assert_eq!(en_fr.dir, "OptimaMT-en-fr");
    for p in [en_fr, fr_en] {
        let old = model_folder(&root, &p.old_dir(), p, true);
        std::fs::write(old.join("decoder.onnx.v2.part"), b"x").unwrap();
    }
    assert_eq!(en_fr.old_dir(), "opus-mt-en-fr");
    let other = root.join("somebody-else");
    std::fs::create_dir_all(&other).unwrap();
    adopt_old_folders_in(&root, rename);
    for p in [en_fr, fr_en] {
        let new = root.join(p.dir);
        assert!(!root.join(p.old_dir()).exists());
        assert_eq!(p.intact(&new), 4, "whole: nothing to download");
        assert!(new.join("decoder.onnx.v2.part").exists(), "moves with it");
    }
    assert!(other.exists(), "nothing else is touched");
    // nothing to do: the same again changes nothing
    adopt_old_folders_in(&root, rename);
    assert!(root.join(en_fr.dir).is_dir() && !root.join(en_fr.old_dir()).exists());
    // an empty or missing root is fine
    adopt_old_folders_in(&root.join("nowhere"), rename);
    std::fs::remove_dir_all(root).unwrap();
}

/// Both there (a crash, or a downgrade then an upgrade): the whole one
/// stays whichever name it has, the other is removed, never both.
#[test]
fn two_copies_leave_the_whole_one() {
    let p = pair("en", "fr").unwrap();
    for (old_whole, new_whole, tie) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let root = mt_root("both");
        let old = model_folder(&root, &p.old_dir(), p, old_whole);
        let new = model_folder(&root, p.dir, p, new_whole);
        std::fs::write(old.join("only-old"), b"o").unwrap();
        std::fs::write(new.join("only-new"), b"n").unwrap();
        adopt_old_folders_in(&root, rename);
        assert!(!old.exists(), "one folder is left: {old_whole} {new_whole}");
        assert_eq!(p.intact(&new), if old_whole || new_whole { 4 } else { 1 });
        // the old one's files came over, or the new one stayed (a tie)
        assert_eq!(new.join("only-old").exists(), old_whole && !tie);
        assert_eq!(new.join("only-new").exists(), !old_whole || tie);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// A folder that can't be renamed now (a file open on Windows) stays where
/// it is, still used (`Pair::dir` falls back to it) and not removed; the
/// next start moves it.
#[test]
fn a_locked_folder_is_left_and_moved_at_the_next_start() {
    let root = mt_root("locked");
    let p = pair("en", "fr").unwrap();
    let old = model_folder(&root, &p.old_dir(), p, true);
    let locked = |_: &Path, _: &Path| Err(std::io::Error::other("in use"));
    adopt_old_folders_in(&root, locked);
    assert!(old.is_dir() && !root.join(p.dir).exists());
    assert_eq!(p.intact(&old), 4, "nothing lost");
    adopt_old_folders_in(&root, rename);
    assert!(!old.exists() && root.join(p.dir).is_dir());
    std::fs::remove_dir_all(root).unwrap();
}

/// The folders are under the data folder uninstall removes whole, and the
/// OptimaMT spelling is the one of the repos on Hugging Face.
#[test]
fn the_model_folders_sit_under_the_data_folder_uninstall_removes() {
    let (data, config) = (PathBuf::from("/d"), PathBuf::from("/c"));
    let removed = crate::shared::paths::user_data_in(Some(&data), Some(&config));
    let folder = data.join("OpenCircle").join("mt").join(PAIRS[0].dir);
    assert!(removed.iter().any(|r| folder.starts_with(r)), "{removed:?}");
    for p in &PAIRS {
        let hf = format!("OptimaMT/{}-{}", p.source, p.target);
        assert_eq!(p.dir.replacen('-', "/", 1), hf);
    }
}

/// The real en-ar pair with its v1 decoder (the file 0.1.3 downloaded)
/// translates without a download. XDG_DATA_HOME must hold v0.1.3's
/// OpenCircle/ocr and OpenCircle/mt/OptimaMT-en-ar:
/// `cargo test --release mt::tests::real_v1 -- --ignored --nocapture`
#[test]
#[ignore]
fn real_v1_decoder_still_translates() {
    let p = pair("en", "ar").unwrap();
    let on_disk = std::fs::metadata(p.dir().join(p.decoder.name)).unwrap();
    assert_eq!(on_disk.len(), V1_DECODERS[0].1, "the v1 decoder");
    assert_eq!(p.missing_bytes(), 0);
    // a test that tried to download would panic
    p.ensure(|_, _| {}).unwrap();
    let mut tr = Translator::load(p, &text_reading::models_dir()).unwrap();
    let out = tr.translate(&["New Game".to_string()]).unwrap();
    println!("v1 decoder: New Game -> {}", out[0]);
    assert_eq!(out[0], "لعبة جديدة");
}

#[test]
fn cleaned_output_has_no_markup() {
    assert_eq!(clean("  أحضر [[السيف]] ]] "), "أحضر السيف");
    assert_eq!(
        clean("la création d &apos; un compte &quot;A&amp;B&quot;"),
        "la création d'un compte \"A&B\""
    );
}

fn rss_mb(field: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    (s.lines().find(|l| l.starts_with(field)))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024)
}

/// Peak memory of one translation along `OC_ROUTE` (e.g. `it:ar`), each
/// pair loaded in turn as the helper does, with whatever decoder is on
/// disk (put a v1 file in place to compare). One process per run (the
/// peak is the process's): `OC_ROUTE=en:ar cargo test --release
/// mt::tests::real_route_peak -- --ignored --nocapture`
#[test]
#[ignore]
fn real_route_peak_memory() {
    let route_of = std::env::var("OC_ROUTE").unwrap_or("en:ar".into());
    let (src, tgt) = route_of.split_once(':').unwrap();
    let mut text: Vec<String> = [
        "Your journey begins in the old harbour town.",
        "Collect three crystals before the gate closes at nightfall.",
        "The merchant will not trade with you until you finish his quest.",
        "Press the button to open the inventory and equip the sword.",
        "Settings were saved. Restart the game to apply the new language.",
    ]
    .iter()
    .cycle()
    .take(32)
    .map(|s| s.to_string())
    .collect();
    for p in route(src, tgt).unwrap() {
        p.ensure(|_, _| {}).unwrap(); // (a verified v1 decoder is kept)
        let v = if p.outdated() { "v1" } else { "v2" };
        let mut tr = Translator::load(p, &text_reading::models_dir()).unwrap();
        text = tr.translate(&text).unwrap();
        println!("{} ({v} decoder): peak {} MB", p.dir, rss_mb("VmHWM:"));
    }
    println!("{route_of}: {}", text[0]);
}

/// Pairs added 2026-10-30 download (pinned hashes) and translate a line,
/// one route through English too (Italian -> en -> Spanish is direct, so
/// Dutch -> en -> Arabic): `cargo test mt::tests::real_more_pairs -- --ignored --nocapture`
#[test]
#[ignore]
fn real_more_pairs() {
    let lines = [
        ("es", "en", "Un faro es una torre de señalización luminosa."),
        ("de", "en", "Der Leuchtturm steht an der Küste."),
        ("ru", "en", "Маяк стоит на берегу моря."),
        ("it", "en", "Il faro si trova sulla costa."),
        ("ko", "en", "등대는 해안에 있습니다."),
        ("en", "it", "The lighthouse is on the coast."),
        ("es", "fr", "El faro está en la costa."),
        ("nl", "ar", "De vuurtoren staat aan de kust."),
    ];
    for (src, tgt, line) in lines {
        let mut text = vec![line.to_string()];
        for p in route(src, tgt).unwrap() {
            p.ensure(|_, _| {}).unwrap();
            let mut tr = Translator::load(p, &text_reading::models_dir()).unwrap();
            text = tr.translate(&text).unwrap();
        }
        println!("{src}->{tgt}: {line} -> {}", text[0]);
        assert!(
            !text[0].trim().is_empty() && text[0] != line,
            "{src}->{tgt}"
        );
    }
}

/// The 20 game lines of spike S7 with the real model, in process (models
/// downloaded on first run, about 155 MB with OCR's), then the same
/// through the helper process. Build the binary first:
/// `cargo build --release && cargo test --release mt::tests::real -- --ignored --nocapture`
#[test]
#[ignore]
fn real_model_translates_game_lines() {
    let p = pair("en", "ar").unwrap();
    p.ensure(|_, _| {}).unwrap();
    let lines: Vec<String> = GAME_LINES.iter().map(|(en, _)| en.to_string()).collect();
    let t = std::time::Instant::now();
    let mut tr = Translator::load(p, &text_reading::models_dir()).unwrap();
    println!("load {:?}, RSS {} MB", t.elapsed(), rss_mb("VmRSS"));
    // the same pieces as Hugging Face's MarianTokenizer, >>ara<< first
    // (spike S7 compared 16 lines with quotes, CJK, URLs, tabs: all equal)
    let ids = tr.encode("Bring 5 wolf pelts");
    assert_eq!(ids, [4, 16852, 285, 37089, 1045, 3982, 55, 18, 0]);
    let ids = tr.encode("the door,\" said");
    assert_eq!(ids[3..5], [8687, 452], "',\"' is one piece");
    let mut out = Vec::new();
    for run in 1..=3 {
        let t = std::time::Instant::now();
        out = tr.translate(&lines).unwrap();
        println!("20 lines, run {run}: {:?}", t.elapsed());
    }
    let t = std::time::Instant::now();
    for l in &lines {
        tr.translate(std::slice::from_ref(l)).unwrap();
    }
    println!("20 lines one at a time: {:?}", t.elapsed());
    println!("peak RSS {} MB", rss_mb("VmHWM"));
    for ((en, reference), got) in GAME_LINES.iter().zip(&out) {
        println!("{en} | {got} | {reference}");
    }
    assert_eq!(out.len(), 20);
    assert!(
        out.iter()
            .all(|t| !t.is_empty() && t.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)))
    );
    assert_eq!(out[0], "لعبة جديدة");

    // the helper process: target/<profile>/deps/<test> -> target/<profile>/opencircle
    let exe = std::env::current_exe().unwrap();
    let exe = exe.parent().unwrap().parent().unwrap().join("opencircle");
    let t = std::time::Instant::now();
    let child = text_reading::worker::translate_with(&exe, &lines, p).unwrap();
    println!("helper process, cold: {:?}", t.elapsed());
    assert_eq!(child, out);
}

/// Spike S7's lines: menus, prompts, quest text, subtitles; with a
/// reference translation.
const GAME_LINES: [(&str, &str); 20] = [
    ("New Game", "لعبة جديدة"),
    ("Continue", "متابعة"),
    ("Load Game", "تحميل اللعبة"),
    ("Settings", "الإعدادات"),
    ("Quit to Desktop", "الخروج إلى سطح المكتب"),
    (
        "Are you sure you want to quit? Unsaved progress will be lost.",
        "هل أنت متأكد أنك تريد الخروج؟ سيضيع التقدم غير المحفوظ.",
    ),
    ("Press E to interact", "اضغط E للتفاعل"),
    ("Inventory is full", "المخزون ممتلئ"),
    (
        "Quest Updated: The Lost Sword",
        "تم تحديث المهمة: السيف المفقود",
    ),
    (
        "Find the blacksmith in the northern village.",
        "ابحث عن الحداد في القرية الشمالية.",
    ),
    (
        "Bring 5 wolf pelts to the hunter.",
        "أحضر 5 جلود ذئاب إلى الصياد.",
    ),
    (
        "Defeat the dragon before nightfall.",
        "اهزم التنين قبل حلول الليل.",
    ),
    ("You don't have enough gold.", "ليس لديك ما يكفي من الذهب."),
    (
        "Level Up! You gained a skill point.",
        "ارتفع مستواك! حصلت على نقطة مهارة.",
    ),
    (
        "I have been waiting for you, traveler.",
        "كنت في انتظارك أيها المسافر.",
    ),
    (
        "We must leave the city before the guards find us.",
        "يجب أن نغادر المدينة قبل أن يجدنا الحراس.",
    ),
    (
        "Don't trust him. He's lying to you.",
        "لا تثق به. إنه يكذب عليك.",
    ),
    (
        "The bridge is out. We'll have to find another way across.",
        "الجسر منهار. علينا أن نجد طريقًا آخر للعبور.",
    ),
    (
        "Your health is low. Find a safe place to rest.",
        "صحتك منخفضة. ابحث عن مكان آمن للراحة.",
    ),
    ("Checkpoint reached", "تم الوصول إلى نقطة الحفظ"),
];

/// The staged models-v3 files (`../staging/models/release-v3`) laid out as the
/// app keeps them: `ensure` verifies every pinned size and sha256 (it would
/// download otherwise, which a test must not), then Portuguese translates both
/// ways and through English. Needs OCR's files in the data dir too (see the
/// worker report); `XDG_DATA_HOME=<dir> cargo test --release
/// mt::tests::real_portuguese -- --ignored --nocapture`
#[test]
#[ignore]
fn real_portuguese_translates() {
    for (src, tgt, line, want) in [
        (
            "en",
            "pt",
            "Press E to interact",
            Some("Pressione E para interagir"),
        ),
        (
            "pt",
            "en",
            "Você encontrou uma espada rara!",
            Some("You found a rare sword!"),
        ),
        ("pt", "ar", "Salvar e sair", None),
        ("ar", "pt", "احفظ واخرج", None),
    ] {
        let mut text = vec![line.to_string()];
        for p in route(src, tgt).unwrap() {
            p.ensure(|_, _| {}).unwrap();
            let mut tr = Translator::load(p, &text_reading::models_dir()).unwrap();
            text = tr.translate(&text).unwrap();
        }
        println!("{src}->{tgt}: {line} -> {}", text[0]);
        assert!(
            !text[0].trim().is_empty() && text[0] != line,
            "{src}->{tgt}"
        );
        if let Some(want) = want {
            assert_eq!(text[0], want, "{src}->{tgt}");
        }
    }
}
