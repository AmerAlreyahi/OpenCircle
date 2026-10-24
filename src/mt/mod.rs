//! Offline translation: no key, no account, nothing leaves the PC. OPUS-MT
//! (Helsinki-NLP's Marian transformer models; attribution in `NOTICE`) as int8
//! ONNX, greedy decoding on the ONNX Runtime that OCR already downloads.
//! Like OCR it runs only in the helper process (`ocr::worker`), so ORT and
//! the ~600 MB it peaks at never stay in the app: the app calls
//! [`translate`]. Source text is English (what OCR reads).
//!
//! Spike S7 (20 game lines, en->ar, this machine's CPU): about as good as
//! Firefox's Bergamot model and opus-mt-tc-big, more literal than an AI model.
mod unigram;

use std::path::{Path, PathBuf};

use ort::session::builder::SessionBuilder;
use ort::session::{Session, SessionInputValue};
use ort::value::{DynValue, Tensor};

use crate::ocr::models::{self, ModelFile};
use crate::ocr::{self, OcrError};
use unigram::Unigram;

/// One direction (English -> `target`): its pinned files and the model's shape.
pub struct Pair {
    /// Target language code, as in `translate.target`.
    pub target: &'static str,
    /// Folder under `models_root()`.
    dir: &'static str,
    encoder: ModelFile,
    /// optimum's "merged" decoder: one graph for the first step and the
    /// steps with past keys/values (`use_cache_branch`).
    decoder: ModelFile,
    /// The source language's SentencePiece model, and the shared vocabulary.
    spm: ModelFile,
    vocab: ModelFile,
    /// Multi-target models want the target first in the source: `>>ara<<`.
    lang_token: Option<&'static str>,
    /// Marian: `pad_token_id`, also the decoder's start token.
    pad: u32,
    eos: u32,
    layers: usize,
    heads: usize,
    head_dim: usize,
}

/// A file of a Hugging Face repo at a pinned commit.
macro_rules! hf {
    ($repo:literal, $rev:literal, $name:literal, $path:literal, $size:literal, $sha:literal) => {
        ModelFile::plain(
            $name,
            $size,
            $sha,
            concat!(
                "https://huggingface.co/",
                $repo,
                "/resolve/",
                $rev,
                "/",
                $path
            ),
        )
    };
}

/// Helsinki-NLP/opus-mt-en-ar, exported to ONNX and int8-quantized by
/// Xenova (transformers.js). Another pair is one more const of this shape
/// (Xenova/opus-mt-en-{fr,es,de,ru,zh,jap}...; sizes and SHA-256 from the
/// Hugging Face API, shape from its config.json).
const EN_AR: Pair = Pair {
    target: "ar",
    dir: "opus-mt-en-ar",
    encoder: hf!(
        "Xenova/opus-mt-en-ar",
        "034a684356c19021a187cda4d7f823298b913921",
        "encoder_model_quantized.onnx",
        "onnx/encoder_model_quantized.onnx",
        51_773_854,
        "53c354d87ced44b39085fe8a46a25f09ea4578d442dbce9663f3977228f8a04e"
    ),
    decoder: hf!(
        "Xenova/opus-mt-en-ar",
        "034a684356c19021a187cda4d7f823298b913921",
        "decoder_model_merged_quantized.onnx",
        "onnx/decoder_model_merged_quantized.onnx",
        59_078_120,
        "c0e553b0c26611780c70417c9b4c1ca7169e9cdb18ba9e649197efb017af9f02"
    ),
    spm: hf!(
        "Xenova/opus-mt-en-ar",
        "034a684356c19021a187cda4d7f823298b913921",
        "source.spm",
        "source.spm",
        801_074,
        "81ac8f4d4ee271f4109eac67560a31f9c2d555463ba5eb95c32c6deee8ae819d"
    ),
    vocab: hf!(
        "Xenova/opus-mt-en-ar",
        "034a684356c19021a187cda4d7f823298b913921",
        "vocab.json",
        "vocab.json",
        2_243_581,
        "8b74af9e4c1ec122b8d06362db6c38795e10ca2b0156b2110cf08d4716744b76"
    ),
    lang_token: Some(">>ara<<"),
    pad: 62801,
    eos: 0,
    layers: 6,
    heads: 8,
    head_dim: 64,
};

const PAIRS: [&Pair; 1] = [&EN_AR];

/// Lines per decoder run (padded to the longest; sorted by length first).
/// 20 lines: one run of 20 took 0.37 s, one run per line 1.7 s.
// ponytail: dynamic int8 quantization scales by the whole batch, so a line's
// wording can change with its neighbours (the cache keeps the first one).
// Translate one line per run if that ever matters more than speed.
const BATCH: usize = 32;
/// CPU threads per session: a game is running too. 4 took 0.37 s for the 20
/// lines, 8 took 0.31 s, all 20 (ORT's default) 0.6 s.
const THREADS: usize = 4;
/// Source tokens kept per line, and the most tokens a translation gets.
const MAX_TOKENS: usize = 200;

/// The offline model into `target` ("ar", "ar-SA"), if there is one.
pub fn pair(target: &str) -> Option<&'static Pair> {
    let code = target.split(['-', '_']).next().unwrap_or_default();
    PAIRS
        .into_iter()
        .find(|p| p.target.eq_ignore_ascii_case(code))
}

/// Per-user folder (Linux `~/.local/share/OpenCircle/mt`, Windows
/// `%LOCALAPPDATA%\OpenCircle\mt`), beside OCR's.
fn models_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCircle")
        .join("mt")
}

impl Pair {
    fn dir(&self) -> PathBuf {
        models_root().join(self.dir)
    }

    /// Everything offline translate needs: OCR's files, then this pair's.
    fn files(&'static self) -> Vec<(PathBuf, &'static ModelFile)> {
        let dir = self.dir();
        let mut files = models::ocr_files();
        files.extend(
            [&self.encoder, &self.decoder, &self.spm, &self.vocab].map(|f| (dir.join(f.name), f)),
        );
        files
    }

    /// Bytes still to download (0 = ready). Cheap: sizes only.
    pub fn missing_bytes(&'static self) -> u64 {
        models::missing_bytes(&self.files())
    }

    /// Downloads and verifies whatever is missing (OCR's files too), with
    /// one `progress(done, total)`. Only after the user agreed.
    pub fn ensure(&'static self, progress: impl FnMut(u64, u64)) -> Result<(), OcrError> {
        models::ensure(&self.files(), progress)
    }
}

/// App side: translates `texts` into `target` in the helper process. The
/// models must be present (`Pair::ensure`).
pub fn translate(texts: &[String], target: &str) -> Result<Vec<String>, OcrError> {
    ocr::translate_in_child(texts, target)
}

/// glibc: no ORT memory arena (~350 MB less in a warm helper at the same
/// speed). Each run allocates ~200 MB; the helper's malloc settings
/// (`ocr::worker`) keep that memory between runs instead of the arena.
/// Elsewhere (not measured; without those settings a run was 4x slower) the
/// arena stays.
fn without_arena(b: SessionBuilder) -> ort::Result<SessionBuilder> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    return (b.with_execution_providers([ort::ep::CPU::default().build()])).map_err(Into::into);
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    Ok(b)
}

fn engine_err(e: impl std::fmt::Display) -> OcrError {
    OcrError::Engine(e.to_string())
}

/// ORT plus one pair's sessions and tokenizer. Helper process only.
pub(crate) struct Translator {
    pair: &'static Pair,
    encoder: Session,
    decoder: Session,
    tokenizer: Unigram,
    lang: Option<u32>,
}

impl Translator {
    /// `ort_dir` holds ONNX Runtime (OCR's folder).
    pub(crate) fn load(pair: &'static Pair, ort_dir: &Path) -> Result<Translator, OcrError> {
        ort::init_from(ort_dir.join(models::ORT.name))
            .map_err(ocr::load_err)?
            .commit();
        let dir = pair.dir();
        // Every batch has a new shape: memory patterns would only add RAM.
        // No spin-waiting threads after a run (they slowed the app, see OCR).
        let session = |f: &ModelFile| {
            Session::builder()
                .and_then(|b| b.with_memory_pattern(false).map_err(Into::into))
                .and_then(|b| b.with_intra_threads(THREADS).map_err(Into::into))
                .and_then(|b| b.with_intra_op_spinning(false).map_err(Into::into))
                .and_then(without_arena)
                .and_then(|mut b| b.commit_from_file(dir.join(f.name)))
                .map_err(engine_err)
        };
        let spm = std::fs::read(dir.join(pair.spm.name))?;
        let vocab = std::fs::read(dir.join(pair.vocab.name))?;
        let tokenizer = Unigram::load(&spm, &vocab).map_err(engine_err)?;
        let lang = match pair.lang_token {
            Some(t) => Some(
                tokenizer
                    .id(t)
                    .ok_or_else(|| engine_err(format!("{t} is not in the vocabulary")))?,
            ),
            None => None,
        };
        Ok(Translator {
            pair,
            encoder: session(&pair.encoder)?,
            decoder: session(&pair.decoder)?,
            tokenizer,
            lang,
        })
    }

    /// One translation per text, in order; "" stays "".
    pub(crate) fn translate(&mut self, texts: &[String]) -> Result<Vec<String>, OcrError> {
        let ids: Vec<Vec<i64>> = texts.iter().map(|t| self.encode(t)).collect();
        // Alike lengths share a batch (less padding); the order is restored.
        let mut order: Vec<usize> = (0..texts.len())
            .filter(|&i| !texts[i].trim().is_empty())
            .collect();
        order.sort_by_key(|&i| ids[i].len());
        let mut out = vec![String::new(); texts.len()];
        for chunk in order.chunks(BATCH) {
            let rows: Vec<&[i64]> = chunk.iter().map(|&i| ids[i].as_slice()).collect();
            for (&i, tokens) in chunk.iter().zip(self.run(&rows)?) {
                out[i] = clean(&self.tokenizer.decode(&tokens));
            }
        }
        Ok(out)
    }

    /// Source ids: the language token, the text's pieces, `</s>`.
    fn encode(&self, text: &str) -> Vec<i64> {
        let pieces = self.tokenizer.encode(text);
        let mut ids: Vec<i64> = (self.lang.into_iter())
            .chain(pieces)
            .chain([self.pair.eos])
            .map(i64::from)
            .collect();
        if ids.len() > MAX_TOKENS {
            ids.truncate(MAX_TOKENS - 1);
            ids.push(self.pair.eos.into());
        }
        ids
    }

    /// Encoder once, then greedy decoding with past keys/values.
    fn run(&mut self, rows: &[&[i64]]) -> Result<Vec<Vec<u32>>, OcrError> {
        let p = self.pair;
        let (b, n) = (rows.len(), rows.iter().map(|r| r.len()).max().unwrap_or(1));
        let mut ids = vec![i64::from(p.pad); b * n];
        let mut mask = vec![0i64; b * n];
        for (r, row) in rows.iter().enumerate() {
            ids[r * n..r * n + row.len()].copy_from_slice(row);
            mask[r * n..r * n + row.len()].fill(1);
        }
        let mask = Tensor::from_array(([b, n], mask)).map_err(engine_err)?;
        let ids = Tensor::from_array(([b, n], ids)).map_err(engine_err)?;
        let hidden = self
            .encoder
            .run(ort::inputs!["input_ids" => ids, "attention_mask" => &mask])
            .map_err(engine_err)?
            .remove("last_hidden_state")
            .ok_or_else(|| engine_err("the encoder gave no output"))?;

        // past.{layer}.{decoder,encoder}.{key,value}: empty before step 0
        let names: Vec<String> = (0..p.layers)
            .flat_map(|i| {
                [
                    "decoder.key",
                    "decoder.value",
                    "encoder.key",
                    "encoder.value",
                ]
                .map(|k| format!("{i}.{k}"))
            })
            .collect();
        let empty = || Tensor::<f32>::from_array(([b, p.heads, 0, p.head_dim], Vec::new()));
        let mut past: Vec<DynValue> = Vec::with_capacity(names.len());
        for _ in &names {
            past.push(empty().map_err(engine_err)?.into_dyn());
        }
        let max_len = (2 * n + 10).min(MAX_TOKENS);
        let decoder = &mut self.decoder;
        let mut first = true;
        greedy(b, p.pad, p.eos, max_len, |tokens| {
            let input = Tensor::from_array(([b, 1], tokens.to_vec())).map_err(engine_err)?;
            let branch = Tensor::from_array(([1], vec![!first])).map_err(engine_err)?;
            let mut inputs: Vec<(std::borrow::Cow<str>, SessionInputValue)> = vec![
                ("input_ids".into(), input.into()),
                ("encoder_attention_mask".into(), (&mask).into()),
                ("encoder_hidden_states".into(), (&hidden).into()),
                ("use_cache_branch".into(), branch.into()),
            ];
            for (name, value) in names.iter().zip(&past) {
                inputs.push((format!("past_key_values.{name}").into(), value.into()));
            }
            let mut out = decoder.run(inputs).map_err(engine_err)?;
            let (shape, logits) = out["logits"]
                .try_extract_tensor::<f32>()
                .map_err(engine_err)?;
            let (seq, vocab) = (shape[1] as usize, shape[2] as usize);
            let last: Vec<f32> = (0..b)
                .flat_map(|r| &logits[(r * seq + seq - 1) * vocab..][..vocab])
                .copied()
                .collect();
            for (name, slot) in names.iter().zip(past.iter_mut()) {
                // the encoder's keys/values are made once, on the first step
                if first || name.contains("decoder") {
                    *slot = out
                        .remove(format!("present.{name}"))
                        .ok_or_else(|| engine_err(format!("no present.{name}")))?;
                }
            }
            first = false;
            Ok(last)
        })
    }
}

/// Greedy decoding of `rows` sequences. `step(previous token per row)`
/// returns rows x vocab next-token logits; a row ends at `eos`, all end by
/// `max_len` tokens. `pad` (the start token) is never chosen. Ids exclude
/// `eos`.
fn greedy(
    rows: usize,
    pad: u32,
    eos: u32,
    max_len: usize,
    mut step: impl FnMut(&[i64]) -> Result<Vec<f32>, OcrError>,
) -> Result<Vec<Vec<u32>>, OcrError> {
    let mut out = vec![Vec::new(); rows];
    let mut done = vec![false; rows];
    let mut tokens = vec![i64::from(pad); rows];
    for _ in 0..max_len {
        let logits = step(&tokens)?;
        let vocab = logits.len() / rows.max(1);
        if vocab == 0 || logits.len() != rows * vocab {
            return Err(engine_err("the decoder gave no logits"));
        }
        for r in 0..rows {
            let row = &logits[r * vocab..][..vocab];
            let best = (row.iter().enumerate())
                .filter(|&(i, _)| i != pad as usize)
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(eos, |(i, _)| i as u32);
            tokens[r] = best.into();
            if done[r] {
                continue;
            }
            if best == eos {
                done[r] = true;
            } else {
                out[r].push(best);
            }
        }
        if done.iter().all(|d| *d) {
            break;
        }
    }
    Ok(out)
}

/// The model's text as the pipeline wants it: trimmed, and no `[[`/`]]`
/// (they would become accent spans; MT never marks names).
fn clean(text: &str) -> String {
    text.replace("[[", "").replace("]]", "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake decoder: plays `script` (one token per row per step) and
    /// checks it is fed back the previous tokens.
    fn scripted(
        script: Vec<Vec<u32>>,
        vocab: usize,
    ) -> impl FnMut(&[i64]) -> Result<Vec<f32>, OcrError> {
        let mut step = 0;
        let mut prev: Option<Vec<i64>> = None;
        move |tokens: &[i64]| {
            match &prev {
                None => assert!(
                    tokens.iter().all(|&t| t == 9),
                    "starts with pad: {tokens:?}"
                ),
                Some(p) => assert_eq!(tokens, p.as_slice(), "fed back"),
            }
            let row = &script[step.min(script.len() - 1)];
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
        // row 0: 3 4 eos; row 1: 5 eos; then both keep getting tokens
        let script = vec![vec![3, 5], vec![4, 0], vec![0, 7], vec![6, 6]];
        let out = greedy(2, 9, 0, 50, scripted(script, 10)).unwrap();
        assert_eq!(out, vec![vec![3, 4], vec![5]]);
        // never ends: capped
        let out = greedy(1, 9, 0, 4, scripted(vec![vec![2]], 10)).unwrap();
        assert_eq!(out, vec![vec![2, 2, 2, 2]]);
        // a decoder that answers nothing is an error, not a panic
        assert!(greedy(2, 9, 0, 4, |_| Ok(vec![1.0])).is_err());
    }

    #[test]
    fn pairs_by_language_code() {
        assert_eq!(pair("ar").map(|p| p.target), Some("ar"));
        assert_eq!(pair("AR-sa").map(|p| p.target), Some("ar"));
        assert!(pair("fr").is_none() && pair("").is_none());
        let p = pair("ar").unwrap();
        let files = p.files();
        // OCR's files first (ORT), then the pair's, in its own folder
        assert_eq!(files[0].1.name, models::ORT.name);
        let own = files.iter().filter(|(path, _)| path.starts_with(p.dir()));
        assert_eq!(own.count(), 4);
        let total: u64 = files.iter().map(|(_, f)| f.size).sum();
        assert!(p.missing_bytes() <= total);
        // what the consent says: the pair alone is about 120 MB
        let pair_bytes: u64 = [&p.encoder, &p.decoder, &p.spm, &p.vocab]
            .map(|f| f.size)
            .iter()
            .sum();
        assert_eq!(pair_bytes, 113_896_629);
    }

    #[test]
    fn cleaned_output_has_no_markup() {
        assert_eq!(clean("  أحضر [[السيف]] ]] "), "أحضر السيف");
    }

    fn rss_mb(field: &str) -> u64 {
        let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        (s.lines().find(|l| l.starts_with(field)))
            .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            .map_or(0, |kb| kb / 1024)
    }

    /// The 20 game lines of spike S7 with the real model, in process (models
    /// downloaded on first run, about 155 MB with OCR's), then the same
    /// through the helper process. Build the binary first:
    /// `cargo build --release && cargo test --release mt::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_model_translates_game_lines() {
        let p = pair("ar").unwrap();
        p.ensure(|_, _| {}).unwrap();
        let lines: Vec<String> = GAME_LINES.iter().map(|(en, _)| en.to_string()).collect();
        let t = std::time::Instant::now();
        let mut tr = Translator::load(p, &ocr::models_dir()).unwrap();
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
        assert!(out.iter().all(|t| !t.is_empty() && t.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))));
        assert_eq!(out[0], "لعبة جديدة");

        // the helper process: target/<profile>/deps/<test> -> target/<profile>/opencircle
        let exe = std::env::current_exe().unwrap();
        let exe = exe.parent().unwrap().parent().unwrap().join("opencircle");
        let t = std::time::Instant::now();
        let child = ocr::worker::translate_with(&exe, &lines, "ar").unwrap();
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
}
