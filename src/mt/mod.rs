//! Offline translation: no key, no account, nothing leaves the PC. OPUS-MT
//! (Helsinki-NLP's Marian transformer models; attribution in `NOTICE`) as int8
//! ONNX, greedy decoding on the ONNX Runtime that OCR already downloads.
//! Like OCR it runs only in the helper process (`ocr::worker`), so ORT and
//! the ~600 MB it peaks at never stay in the app: the app calls
//! [`translate`]. One model per direction (`PAIRS`), downloaded the first
//! time that direction is used, after the user agreed.
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
use crate::translate::lang;
use unigram::Unigram;

/// One direction (`source` -> `target`): its pinned files and the model's shape.
#[derive(Debug)]
pub struct Pair {
    /// Language codes, as in the language table (`translate::lang`).
    pub source: &'static str,
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

/// A Helsinki-NLP OPUS-MT pair as Xenova (transformers.js) exported it to
/// ONNX and int8-quantized it: `Xenova/opus-mt-<pair>` at a pinned commit.
/// Sizes and SHA-256 from the Hugging Face API; pad from its config.json
/// (all of them: 6 layers, 8 heads, d_model 512, eos 0).
macro_rules! pair {
    ($src:literal, $tgt:literal, $pair:literal, $rev:literal,
     [$es:literal, $eh:literal], [$ds:literal, $dh:literal],
     [$ss:literal, $sh:literal], [$vs:literal, $vh:literal], $token:expr, $pad:literal) => {
        Pair {
            source: $src,
            target: $tgt,
            dir: concat!("opus-mt-", $pair),
            encoder: hf!(
                $pair,
                $rev,
                "encoder_model_quantized.onnx",
                "onnx/encoder_model_quantized.onnx",
                $es,
                $eh
            ),
            decoder: hf!(
                $pair,
                $rev,
                "decoder_model_merged_quantized.onnx",
                "onnx/decoder_model_merged_quantized.onnx",
                $ds,
                $dh
            ),
            spm: hf!($pair, $rev, "source.spm", "source.spm", $ss, $sh),
            vocab: hf!($pair, $rev, "vocab.json", "vocab.json", $vs, $vh),
            lang_token: $token,
            pad: $pad,
            eos: 0,
            layers: 6,
            heads: 8,
            head_dim: 64,
        }
    };
}

/// A file of `Xenova/opus-mt-<pair>` at a pinned commit.
macro_rules! hf {
    ($pair:literal, $rev:literal, $name:literal, $path:literal, $size:literal, $sha:literal) => {
        ModelFile::plain(
            $name,
            $size,
            $sha,
            concat!(
                "https://huggingface.co/Xenova/opus-mt-",
                $pair,
                "/resolve/",
                $rev,
                "/",
                $path
            ),
        )
    };
}

/// Every offline pair. Another one is one more entry (Xenova/opus-mt-*;
/// `jap` is Bible-only, so Japanese uses `ja`). The Chinese model writes
/// several Chinese scripts: `>>cmn_Hans<<` asks for Simplified.
pub static PAIRS: [Pair; 10] = [
    // Helsinki-NLP/opus-mt-en-ar
    pair!(
        "en",
        "ar",
        "en-ar",
        "034a684356c19021a187cda4d7f823298b913921",
        [
            51_773_854,
            "53c354d87ced44b39085fe8a46a25f09ea4578d442dbce9663f3977228f8a04e"
        ],
        [
            59_078_120,
            "c0e553b0c26611780c70417c9b4c1ca7169e9cdb18ba9e649197efb017af9f02"
        ],
        [
            801_074,
            "81ac8f4d4ee271f4109eac67560a31f9c2d555463ba5eb95c32c6deee8ae819d"
        ],
        [
            2_243_581,
            "8b74af9e4c1ec122b8d06362db6c38795e10ca2b0156b2110cf08d4716744b76"
        ],
        Some(">>ara<<"),
        62801
    ),
    // Helsinki-NLP/opus-mt-en-fr
    pair!(
        "en",
        "fr",
        "en-fr",
        "28726206f80896b90035bd99cccd5cc1e151f916",
        [
            50_090_398,
            "0a81bdba62f53223740a8c6c6f58e716eba8ea1f92dfa569caf27ed5e3b6a0f7"
        ],
        [
            57_381_512,
            "333b244bce16023df04541c8cf9fd60aec9b0569da393c4b831d561897b0bda8"
        ],
        [
            778_395,
            "173e9f493a668fe396d599e28d414a201193094e6ffd7a4678e5aab0f6d3d838"
        ],
        [
            1_458_196,
            "f2ba9c69ae20f96b8bd821239a9152be422394f980350b77907cffc183db5f2d"
        ],
        None,
        59513
    ),
    // Helsinki-NLP/opus-mt-en-es
    pair!(
        "en",
        "es",
        "en-es",
        "4b002a4c7edd54a7ced58877258b87f7efd3f892",
        [
            52_899_742,
            "03434edd1147e6c49e22066e9dc5888235e99cc35a6711d1c4ea5b06455d5da9"
        ],
        [
            60_212_804,
            "58ee5ddd6e22d1693d8722b90c1486afb93700a4376cf28fd74175052ad530ae"
        ],
        [
            801_636,
            "4dd547c24816a335e7b0b2e63376a8f1b3cbfc671eda5ab808dd44fdadaa8791"
        ],
        [
            1_720_044,
            "b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163"
        ],
        None,
        65000
    ),
    // Helsinki-NLP/opus-mt-en-de
    pair!(
        "en",
        "de",
        "en-de",
        "1ca130c44c4c5441ef16d48aae521a424ab644f7",
        [
            49_366_942,
            "15834b45fabd2dfb8c6c029b3ca3e7289aeefd90ece798ce42bcf548d1bd3b8d"
        ],
        [
            56_652_404,
            "8b46a825964cdd182fe47cc780f0fb87d357eae1f47cf20aa63c1c09be5b510c"
        ],
        [
            768_489,
            "678f2a1177d8389f67b66299762dcc4fc567e89b07e212ba91b0c56daecf47ce"
        ],
        [
            1_389_436,
            "d5acea957b265a78554999144459c5e391e0df525864edc8287bc090290baa44"
        ],
        None,
        58100
    ),
    // Helsinki-NLP/opus-mt-en-ru
    pair!(
        "en",
        "ru",
        "en-ru",
        "e050376e960175e8ddc5cff85025dc8436cddd68",
        [
            51_628_446,
            "97f28d8295d231b7da721ded06fa8c034787df2122cd8f85b904abdc4d15bd53"
        ],
        [
            58_931_576,
            "7efefcd793a5663bc204224a92a123524f3dfe2ffad59e5f73d6734d1b11ad04"
        ],
        [
            802_781,
            "16bebef1389a0b8ab452772c4e35b9e605e5713f8ac7baa71ca701394eaa086d"
        ],
        [
            2_726_796,
            "5cf0d95d930d8d3e783c9e2f46a72f08b43a18060dab4ddefbcb66a733efedcb"
        ],
        None,
        62517
    ),
    // Helsinki-NLP/opus-mt-en-zh
    pair!(
        "en",
        "zh",
        "en-zh",
        "046f55aec303cdee3e0318604406d4df20f1e8ea",
        [
            52_899_742,
            "d3b7912bf6a9bd27e4c074c2df91d4ff3d5b4bc5f7f6c8d7cc9c805c98fbafee"
        ],
        [
            60_212_804,
            "023be4f841f4c47cd65fffcbaa81c0d99d7f7e0138f7ba0e03fa220a4e688aff"
        ],
        [
            806_435,
            "5775ddc9e3ff2fae91554da56468ad35ff56edaba870fea74447bc7234bfdaa8"
        ],
        [
            1_747_795,
            "22c957348eed495ee925afc40a36da3e387c8a34a734c8486967c2dca271613e"
        ],
        Some(">>cmn_Hans<<"),
        65000
    ),
    // Helsinki-NLP/opus-mt-ar-en
    pair!(
        "ar",
        "en",
        "ar-en",
        "09c7556866400dcc53562ba8d7035119e7d9a2c1",
        [
            51_790_238,
            "d9b0d612acff86cf87634b49bda566d1ef39b312837de549786ea18fecefc708"
        ],
        [
            59_094_632,
            "cdb25ae736afe9d48035b0ea311ab4b25f315810fab7f3c6f6cb0d8d87af35fa"
        ],
        [
            917_407,
            "484f7210e5f4466f7e5b99b660717c6ef2a6f90746196daba507795b69dc02a2"
        ],
        [
            2_257_246,
            "87b78c097c331672ca22a522c3ba24c5dc2cdff90495dcb52cf742635439a3a0"
        ],
        None,
        62833
    ),
    // Helsinki-NLP/opus-mt-fr-en
    pair!(
        "fr",
        "en",
        "fr-en",
        "6b166a182780e118c997879d0ad5be4b53671644",
        [
            50_090_398,
            "e727cb26ac6bf816394c49671af69c9ae5798868fed1e40849872415dffc1772"
        ],
        [
            57_381_512,
            "73bc7ac8e29c42e6f212ebcc29a2991d3646a04aabb642c0036aa54b40e4e1a9"
        ],
        [
            802_397,
            "78d0e717c77053f1c4b856d8661d9cb87c64f083a35418c087b9146300e4f585"
        ],
        [
            1_458_196,
            "f2ba9c69ae20f96b8bd821239a9152be422394f980350b77907cffc183db5f2d"
        ],
        None,
        59513
    ),
    // Helsinki-NLP/opus-mt-zh-en
    pair!(
        "zh",
        "en",
        "zh-en",
        "39d480d52a9ea3065a1f117adfe4dbc55de10e6f",
        [
            52_899_742,
            "84d5e171b626bc8b6b220d022ac58696e9528c25deeacca62b5cbf4364547a99"
        ],
        [
            60_212_804,
            "c6b7f04ff1ba0fbd1bf6852599b4c0cad6fe512d57cd887f44ef36cf705424cb"
        ],
        [
            804_677,
            "e27a3a1b539f4959ec72ea60e453f49156289f95d4e6000b29332efc45616203"
        ],
        [
            1_747_906,
            "08a119a1defd522fa047cb5e3bfe3e89633e96caa38ced0dc9cee7ef1021a011"
        ],
        None,
        65000
    ),
    // Helsinki-NLP/opus-mt-ja-en
    pair!(
        "ja",
        "en",
        "ja-en",
        "1a906cfaaf7c8f4193f67f5885c082aa6dbd9d16",
        [
            50_705_822,
            "345262b16bcdda1468b0f3380c112b7ce79f731176b4b1d21f6edd5b2ae0d25c"
        ],
        [
            58_001_744,
            "b304d0014e4e1575437b6af95467b6cb54405d923732d8359113bd6dbbee93c0"
        ],
        [
            781_853,
            "d0b5c3b10b5959f056ff2c86e2f2356129242ff1fc72d3a4a34d6a8c0eee4e57"
        ],
        [
            1_622_775,
            "a1667b43d021e09c76f385101bd473f20b5691a12cdca7dda43e4a8daff20021"
        ],
        None,
        60715
    ),
];

/// Lines per decoder run (sorted by length first; a line that ended leaves
/// the run). Each decoding step costs about the same for 1 line or 32, so
/// fewer, fuller runs are faster: the 32 sentences of a release-notes page
/// took 1.2 s in one run, 1.6 s in runs of 8, 5.1 s one by one (2 cores).
// ponytail: dynamic int8 quantization scales by the whole batch, so a line's
// wording can change with its neighbours (the cache keeps the first one).
// Translate one line per run if that ever matters more than speed.
const BATCH: usize = 32;
/// CPU threads per session: a game is running too. 4 took 0.37 s for the 20
/// lines, 8 took 0.31 s, all 20 (ORT's default) 0.6 s.
const THREADS: usize = 4;
/// Source tokens kept per line, and the most tokens a translation gets.
const MAX_TOKENS: usize = 200;

/// The offline model from `source` into `target` (codes as the language
/// table finds them: "ar-SA" is Arabic), if there is one. Not for "auto":
/// the caller picks the source first.
pub fn pair(source: &str, target: &str) -> Option<&'static Pair> {
    let (s, t) = (lang::find(source)?, lang::find(target)?);
    PAIRS
        .iter()
        .find(|p| p.source == s.code && p.target == t.code)
}

/// Whether text in `source` ("auto" = whatever is detected) can be
/// translated into `target` offline: for "auto", some pair into `target`.
pub fn available(source: &str, target: &str) -> bool {
    match source {
        lang::AUTO => PAIRS
            .iter()
            .any(|p| lang::find(target).is_some_and(|t| t.code == p.target)),
        s => pair(s, target).is_some(),
    }
}

/// What to say when no offline model translates `source` into `target`
/// (never a silent switch to the AI: the user chooses).
pub fn unavailable(source: &str, target: &str) -> String {
    format!(
        "there is no offline model from {} into {}. Choose AI as the engine in \
         Settings > Translate (and set it up in Settings > AI) to translate it",
        lang::name(source),
        lang::name(target)
    )
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

    /// "English → French offline translation".
    pub fn label(&self) -> String {
        format!(
            "{} → {} offline translation",
            lang::name(self.source),
            lang::name(self.target)
        )
    }

    /// Everything translating this pair needs: OCR's files for its source
    /// language (ONNX Runtime first), then the pair's own.
    fn files(&'static self) -> Vec<(PathBuf, &'static ModelFile)> {
        let dir = self.dir();
        let mut files = models::ocr_files(models::recognizer_for(self.source));
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

/// App side: translates `texts` with `pair` in the helper process. The
/// models must be present (`Pair::ensure`).
pub fn translate(texts: &[String], pair: &'static Pair) -> Result<Vec<String>, OcrError> {
    ocr::translate_in_child(texts, pair)
}

/// No ORT memory arena. glibc: ~350 MB less in a warm helper at the same
/// speed (each run allocates ~200 MB; the helper's malloc settings in
/// `ocr::worker` keep that memory between runs instead of the arena).
/// Windows (Win10 VM, dense page, 3 runs each): helper 437 -> 418 MB working
/// set (-4 %), total time 1.85 -> 1.84 s (no change).
fn without_arena(b: SessionBuilder) -> ort::Result<SessionBuilder> {
    (b.with_execution_providers([ort::ep::CPU::default().build()])).map_err(Into::into)
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
        if pair.missing_bytes() > 0 {
            return Err(OcrError::Missing(pair.label()));
        }
        ocr::init_ort(ort_dir)?;
        let dir = pair.dir();
        // Every batch has a new shape: memory patterns would only add RAM.
        // No spin-waiting threads after a run (they slowed the app, see OCR).
        // The decoder's output layer is DequantizeLinear(int8 weights) -> MatMul:
        // ORT keeps a DequantizeLinear for QDQ fusion, so every decoding step
        // made the 62k x 512 float matrix again (~70 ms a step, 2.8x the
        // whole page). Without QDQ handling (these models have none: they use
        // MatMulInteger) it is folded once at load: ~130 MB more in the
        // helper, the same translations.
        let session = |f: &ModelFile| {
            Session::builder()
                .and_then(|b| b.with_memory_pattern(false).map_err(Into::into))
                .and_then(|b| b.with_intra_threads(THREADS).map_err(Into::into))
                .and_then(|b| b.with_intra_op_spinning(false).map_err(Into::into))
                .and_then(|b| {
                    (b.with_config_entry("session.disable_quant_qdq", "1")).map_err(Into::into)
                })
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
        let mut mask = Tensor::from_array(([b, n], mask))
            .map_err(engine_err)?
            .into_dyn();
        let ids = Tensor::from_array(([b, n], ids)).map_err(engine_err)?;
        let mut hidden = self
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
        greedy(b, p.pad, p.eos, max_len, |tokens, keep| {
            // rows that ended leave the batch: their inputs and past go
            if keep.len() < mask.shape()[0] as usize {
                mask = rows_of::<i64>(&mask, keep)?;
                hidden = rows_of::<f32>(&hidden, keep)?;
                for v in past.iter_mut() {
                    *v = rows_of::<f32>(v, keep)?;
                }
            }
            let b = tokens.len();
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

/// `value` (rows first) with only the rows `keep`, in that order.
fn rows_of<T>(value: &DynValue, keep: &[usize]) -> Result<DynValue, OcrError>
where
    T: ort::value::PrimitiveTensorElementType + Clone + std::fmt::Debug + 'static,
{
    let (shape, data) = value.try_extract_tensor::<T>().map_err(engine_err)?;
    let mut dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
    let row = data.len() / dims[0].max(1);
    let kept: Vec<T> = (keep.iter())
        .flat_map(|&k| data[k * row..(k + 1) * row].iter().cloned())
        .collect();
    dims[0] = keep.len();
    Ok(Tensor::from_array((dims, kept))
        .map_err(engine_err)?
        .into_dyn())
}

/// Greedy decoding of `rows` sequences. `step(previous token per row still
/// running, which rows of the last step's those are)` returns their next-token
/// logits (rows x vocab); a row ends at `eos` and leaves the batch (a batch
/// costs what its rows need, not what its longest one does), all end by
/// `max_len` tokens. `pad` (the start token) is never chosen. Ids exclude
/// `eos`.
fn greedy(
    rows: usize,
    pad: u32,
    eos: u32,
    max_len: usize,
    mut step: impl FnMut(&[i64], &[usize]) -> Result<Vec<f32>, OcrError>,
) -> Result<Vec<Vec<u32>>, OcrError> {
    let mut out = vec![Vec::new(); rows];
    // the rows still running, the last step's rows they are, their tokens
    let mut running: Vec<usize> = (0..rows).collect();
    let mut keep = running.clone();
    let mut tokens = vec![i64::from(pad); rows];
    for _ in 0..max_len {
        let logits = step(&tokens, &keep)?;
        let vocab = logits.len() / running.len().max(1);
        if vocab == 0 || logits.len() != running.len() * vocab {
            return Err(engine_err("the decoder gave no logits"));
        }
        let mut next = (Vec::new(), Vec::new(), Vec::new());
        for (at, &r) in running.iter().enumerate() {
            let row = &logits[at * vocab..][..vocab];
            let best = (row.iter().enumerate())
                .filter(|&(i, _)| i != pad as usize)
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(eos, |(i, _)| i as u32);
            if best != eos {
                out[r].push(best);
                next.0.push(r);
                next.1.push(at);
                next.2.push(best.into());
            }
        }
        (running, keep, tokens) = next;
        if running.is_empty() {
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
        assert!(pair("fr", "de").is_none() && pair("en", "zh-Hant").is_none());
        assert!(pair("", "ar").is_none() && pair(lang::AUTO, "ar").is_none());
        assert!(available(lang::AUTO, "fr") && available(lang::AUTO, "en"));
        assert!(!available(lang::AUTO, "ko") && !available("fr", "de"));
        let why = unavailable("fr", "ja");
        assert!(why.contains("from French into Japanese") && why.contains("Choose AI"));
        let p = pair("en", "ar").unwrap();
        assert_eq!(p.label(), "English → Arabic offline translation");
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
            assert_eq!(p.dir, format!("opus-mt-{}-{}", p.source, p.target));
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
        assert_eq!(bytes, 113_896_629);
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
        let p = pair("en", "ar").unwrap();
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
        let child = ocr::worker::translate_with(&exe, &lines, p).unwrap();
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
