//! Offline translation: no key, no account, nothing leaves the PC. OPUS-MT
//! (Helsinki-NLP's Marian transformer models; attribution in `docs/NOTICE.md`) as int8
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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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
/// `pair!(own, ...)`: a pair Xenova never exported, converted by us the same
/// way from `Helsinki-NLP/opus-mt-<pair>` at commit `$rev`
/// (`scripts/convert-mt-models.py`) and hosted as release assets (`own!`).
/// The decoder of every pair is our lighter rewrite of that file (models-v2).
macro_rules! pair {
    ($src:literal, $tgt:literal, $($rest:tt)*) => {
        pair!(hf, $src, $tgt, $($rest)*)
    };
    ($url:ident, $src:literal, $tgt:literal, $pair:literal, $rev:literal,
     [$es:literal, $eh:literal], [$ds:literal, $dh:literal],
     [$ss:literal, $sh:literal], [$vs:literal, $vh:literal], $token:expr, $pad:literal) => {
        Pair {
            source: $src,
            target: $tgt,
            dir: concat!("OptimaMT-", $pair),
            encoder: $url!(
                $pair,
                $rev,
                "encoder_model_quantized.onnx",
                "onnx/encoder_model_quantized.onnx",
                $es,
                $eh
            ),
            // Every pair's decoder: the pinned one (Xenova's or ours) with
            // its output layer built once instead of in both branches of the
            // merged decoder (`scripts/lighten-mt-decoders.py`: same weights
            // and translations, ~120 MB less per loaded pair). Same name on
            // disk: a v1 decoder (another size) counts as missing, and its
            // download replaces it in place (`models::ensure`).
            decoder: release!(
                "models-v2",
                $pair,
                "decoder_model_merged_quantized.onnx",
                $ds,
                $dh
            ),
            spm: $url!($pair, $rev, "source.spm", "source.spm", $ss, $sh),
            vocab: $url!($pair, $rev, "vocab.json", "vocab.json", $vs, $vh),
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

/// A file of a pair we converted (`pair!(own, ...)`): an asset of the
/// models-v1 release.
macro_rules! own {
    ($pair:literal, $rev:literal, $name:literal, $path:literal, $size:literal, $sha:literal) => {
        release!("models-v1", $pair, $name, $size, $sha)
    };
}

/// An asset of the OpenCircle-models release `$tag`: `opus-mt-<pair>-<name>`
/// (one flat release for every pair).
macro_rules! release {
    ($tag:literal, $pair:literal, $name:literal, $size:literal, $sha:literal) => {
        ModelFile::plain(
            $name,
            $size,
            $sha,
            concat!(
                "https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/",
                $tag,
                "/opus-mt-",
                $pair,
                "-",
                $name
            ),
        )
    };
}

/// Every offline pair. Another one is one more entry (Xenova/opus-mt-*;
/// `jap` is Bible-only, so Japanese uses `ja`). The Chinese model writes
/// several Chinese scripts: `>>cmn_Hans<<` asks for Simplified.
#[rustfmt::skip]
pub static PAIRS: [Pair; 49] = [
    // Helsinki-NLP/opus-mt-en-ar
    pair!("en", "ar", "en-ar", "034a684356c19021a187cda4d7f823298b913921",
        [51_773_854, "53c354d87ced44b39085fe8a46a25f09ea4578d442dbce9663f3977228f8a04e"],
        [59_077_788, "59308c4ec1a61954badad066c7b85a7876549ec91d0a9b22fa218afd872fb48d"],
        [801_074, "81ac8f4d4ee271f4109eac67560a31f9c2d555463ba5eb95c32c6deee8ae819d"],
        [2_243_581, "8b74af9e4c1ec122b8d06362db6c38795e10ca2b0156b2110cf08d4716744b76"],
        Some(">>ara<<"), 62801),
    // Helsinki-NLP/opus-mt-en-fr
    pair!("en", "fr", "en-fr", "28726206f80896b90035bd99cccd5cc1e151f916",
        [50_090_398, "0a81bdba62f53223740a8c6c6f58e716eba8ea1f92dfa569caf27ed5e3b6a0f7"],
        [57_381_180, "96a6477026fdc00bd14f63e54ad02320cbf14ecec67ada79281e193c11a55167"],
        [778_395, "173e9f493a668fe396d599e28d414a201193094e6ffd7a4678e5aab0f6d3d838"],
        [1_458_196, "f2ba9c69ae20f96b8bd821239a9152be422394f980350b77907cffc183db5f2d"],
        None, 59513),
    // Helsinki-NLP/opus-mt-en-es
    pair!("en", "es", "en-es", "4b002a4c7edd54a7ced58877258b87f7efd3f892",
        [52_899_742, "03434edd1147e6c49e22066e9dc5888235e99cc35a6711d1c4ea5b06455d5da9"],
        [60_212_472, "4c9caf323108bc916b608b023ffd8cdf9ccd49ce1c0e62e63d931c81b9bc5084"],
        [801_636, "4dd547c24816a335e7b0b2e63376a8f1b3cbfc671eda5ab808dd44fdadaa8791"],
        [1_720_044, "b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163"],
        None, 65000),
    // Helsinki-NLP/opus-mt-en-de
    pair!("en", "de", "en-de", "1ca130c44c4c5441ef16d48aae521a424ab644f7",
        [49_366_942, "15834b45fabd2dfb8c6c029b3ca3e7289aeefd90ece798ce42bcf548d1bd3b8d"],
        [56_652_072, "ab0ae18ee7fca5ed4093c37d47aa8ccd03aca3074c1c15fafbb59ce49de49780"],
        [768_489, "678f2a1177d8389f67b66299762dcc4fc567e89b07e212ba91b0c56daecf47ce"],
        [1_389_436, "d5acea957b265a78554999144459c5e391e0df525864edc8287bc090290baa44"],
        None, 58100),
    // Helsinki-NLP/opus-mt-en-ru
    pair!("en", "ru", "en-ru", "e050376e960175e8ddc5cff85025dc8436cddd68",
        [51_628_446, "97f28d8295d231b7da721ded06fa8c034787df2122cd8f85b904abdc4d15bd53"],
        [58_931_244, "0f61b4ecd26e232bf0084a51ce104cd6933830bee65393492756b4b4929670a2"],
        [802_781, "16bebef1389a0b8ab452772c4e35b9e605e5713f8ac7baa71ca701394eaa086d"],
        [2_726_796, "5cf0d95d930d8d3e783c9e2f46a72f08b43a18060dab4ddefbcb66a733efedcb"],
        None, 62517),
    // Helsinki-NLP/opus-mt-en-zh
    pair!("en", "zh", "en-zh", "046f55aec303cdee3e0318604406d4df20f1e8ea",
        [52_899_742, "d3b7912bf6a9bd27e4c074c2df91d4ff3d5b4bc5f7f6c8d7cc9c805c98fbafee"],
        [60_212_472, "47ab6181e8b241c54dd17c9ec088e122d671c49425627e82bee7080d2de07eba"],
        [806_435, "5775ddc9e3ff2fae91554da56468ad35ff56edaba870fea74447bc7234bfdaa8"],
        [1_747_795, "22c957348eed495ee925afc40a36da3e387c8a34a734c8486967c2dca271613e"],
        Some(">>cmn_Hans<<"), 65000),
    // Helsinki-NLP/opus-mt-ar-en
    pair!("ar", "en", "ar-en", "09c7556866400dcc53562ba8d7035119e7d9a2c1",
        [51_790_238, "d9b0d612acff86cf87634b49bda566d1ef39b312837de549786ea18fecefc708"],
        [59_094_300, "3b580f8bb33683fe3f4552a21646f0d6a492ee1bb2d56780fdd720de31651945"],
        [917_407, "484f7210e5f4466f7e5b99b660717c6ef2a6f90746196daba507795b69dc02a2"],
        [2_257_246, "87b78c097c331672ca22a522c3ba24c5dc2cdff90495dcb52cf742635439a3a0"],
        None, 62833),
    // Helsinki-NLP/opus-mt-fr-en
    pair!("fr", "en", "fr-en", "6b166a182780e118c997879d0ad5be4b53671644",
        [50_090_398, "e727cb26ac6bf816394c49671af69c9ae5798868fed1e40849872415dffc1772"],
        [57_381_180, "bbbb96e87399efc2a1d9e878ccb9e56dccd481b9a082d1860d5cb9d71b878401"],
        [802_397, "78d0e717c77053f1c4b856d8661d9cb87c64f083a35418c087b9146300e4f585"],
        [1_458_196, "f2ba9c69ae20f96b8bd821239a9152be422394f980350b77907cffc183db5f2d"],
        None, 59513),
    // Helsinki-NLP/opus-mt-zh-en
    pair!("zh", "en", "zh-en", "39d480d52a9ea3065a1f117adfe4dbc55de10e6f",
        [52_899_742, "84d5e171b626bc8b6b220d022ac58696e9528c25deeacca62b5cbf4364547a99"],
        [60_212_472, "ef9bbdba03642c9db4be12107329c6e567167692641d0fe5fc7f668839cee614"],
        [804_677, "e27a3a1b539f4959ec72ea60e453f49156289f95d4e6000b29332efc45616203"],
        [1_747_906, "08a119a1defd522fa047cb5e3bfe3e89633e96caa38ced0dc9cee7ef1021a011"],
        None, 65000),
    // Helsinki-NLP/opus-mt-ja-en
    pair!("ja", "en", "ja-en", "1a906cfaaf7c8f4193f67f5885c082aa6dbd9d16",
        [50_705_822, "345262b16bcdda1468b0f3380c112b7ce79f731176b4b1d21f6edd5b2ae0d25c"],
        [58_001_412, "6366a67461afcd7f966f5b8b05f6050a9c74e5b6490f0d2f9ea25f8be29bae57"],
        [781_853, "d0b5c3b10b5959f056ff2c86e2f2356129242ff1fc72d3a4a34d6a8c0eee4e57"],
        [1_622_775, "a1667b43d021e09c76f385101bd473f20b5691a12cdca7dda43e4a8daff20021"],
        None, 60715),
    // Helsinki-NLP/opus-mt-es-en
    pair!("es", "en", "es-en", "eadfd7c658a9d8929ac3b8e996b68a68e2c7d480",
        [52_899_742, "c01e70f8455efc350831aa2af7ed187b16829241f0018ba07d1ba643a391bc18"],
        [60_212_472, "218b6fc3896b585876698f84d37eb309339d6f2b497e36e890e1cc93be5e3832"],
        [825_924, "e236ee6d866b635c0142114f8647f39831f9d92534aa2aad75c942f6a78ad0e3"],
        [1_720_044, "b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163"],
        None, 65000),
    // Helsinki-NLP/opus-mt-de-en
    pair!("de", "en", "de-en", "399dfd68706739fffd503f876093e455ae268a06",
        [49_366_942, "4cedda8f8c89b72a42b3c6cd1e7a27f2de24457093e3bf80cb3e46829641fcd8"],
        [56_652_072, "d9218390aec5c4588090dfa32d98bb796d7e89c4fe38f15611bf34e84669169b"],
        [796_845, "bbd1f495eea99c8e21ae086d9146e0fa7b096c3dfdd9ba07ab8b631889df5c9b"],
        [1_389_436, "d5acea957b265a78554999144459c5e391e0df525864edc8287bc090290baa44"],
        None, 58100),
    // Helsinki-NLP/opus-mt-ru-en
    pair!("ru", "en", "ru-en", "afe8c6c738ec81b6d033fd8f44f9678a639a7c67",
        [51_628_446, "c10df0466540534bb70c4402d539c349b8bfa9968184cad89f51408cf13e7eb7"],
        [58_931_244, "a4fcff5376935b15833ac213965e27c47e6490a1c33de42ba95224237874bde3"],
        [1_080_169, "745998e51ba5b058e38b7ac7765c25c43ed5c1c39cc92b27163b9b2e323c9d7c"],
        [2_726_796, "5cf0d95d930d8d3e783c9e2f46a72f08b43a18060dab4ddefbcb66a733efedcb"],
        None, 62517),
    // Helsinki-NLP/opus-mt-it-en
    pair!("it", "en", "it-en", "fd0b89b9c052adc1f2f64152f555aa17353728be",
        [60_773_278, "a025d2d85c90da23880116b67175e43e71921d8d2bf446b9f1eacdc86015aa4c"],
        [68_147_520, "a7a46ab70be9b525ef7d8a1370e6752ad6cdb9af5819650fc378db8a4b7e7e60"],
        [813_709, "1bd307e2756991d470e4b40172147f7889b46a323b1a9a2150e66274825b1f21"],
        [2_530_593, "08e5b03ab20b2b9b85d41aed1b36dce2650afcb6e96274a06b7869b2e06884d5"],
        None, 80378),
    // Helsinki-NLP/opus-mt-nl-en
    pair!("nl", "en", "nl-en", "82c42d95d1508037cbe7172d85fd4e940a2f3584",
        [53_937_566, "7b8b1e9071bed8e2394e3bd2f65b952700b1dd1293d087141134c8bac018653b"],
        [61_258_404, "94dd7329eaa3644b3110efcd97e55f1046b94ee3f81213360cd4fc8fd08d872b"],
        [813_866, "f538d23d4b30641cd97c608d2a83890d503aa1286a5560f19ef949404a786e94"],
        [1_794_274, "b1fba91c620564d90848c5f77b03f13d17949790f8e7c7741e4a8ea13b8b0d91"],
        None, 67027),
    // Helsinki-NLP/opus-mt-uk-en
    pair!("uk", "en", "uk-en", "b2ad6afa424aaeff8913ff350860b7a7b7c0a607",
        [51_151_774, "5e8c4c7e88742efa0a8e013f3897f17e29357d54b8c86a13472669546d6ed561"],
        [58_450_848, "450c899d01b3e4114c06d4a794d264f796ac6e0f231b9ae37126fd25c4aa41f8"],
        [1_007_605, "7abd810b2df512e6fe177f8732f6a0b3107614c84cc52aba20d805b4cb0d7b6b"],
        [2_490_886, "e8b45188b1db6a4e0f960813e1bd493948ab0bc4d57b069ea03e77a78b7567d4"],
        None, 61586),
    // Helsinki-NLP/opus-mt-pl-en
    pair!("pl", "en", "pl-en", "aeaf0e003b045248c28a559a5ce6027b54ebdaba",
        [52_095_390, "2416eb3c6db52a2d116539bf673b596e6a3d1453153b17778e85c9f55249c832"],
        [59_401_836, "12ef8c518a6fc1dfd5ef3f771c01917df858ecffb07357b3ffc0629e8ff2bdd9"],
        [834_921, "86b3b5f9de4f5ed9c46321bab1f865e3aa9ddf63094e89b49dd515aeafa79ce4"],
        [1_642_111, "bd1a4fc17902ffffd8266b1b734c67ce64ce2d9e37a9da8c1f160d1edef69794"],
        None, 63429),
    // Helsinki-NLP/opus-mt-hi-en
    pair!("hi", "en", "hi-en", "99853100ed9d932987a6816e8cf7b1b6742a6116",
        [50_916_254, "dc5a1b084a83019bbb9798809dea5f2d16cc30a9a257238ec93120b3c2892ebf"],
        [58_213_488, "607895853fccd0fa894b1c3fb68c30f235e2d0fb7f7854b3541ee72f52915ee0"],
        [1_057_690, "dd20c408ec4568361d32b41080e3f6fa5b82895135bd25c2747b2c145e42d07b"],
        [2_179_709, "83bf3ef5541c374f1796405a135ffa24d50665f1fcfdb5825f603c70166b4740"],
        None, 61126),
    // Helsinki-NLP/opus-mt-id-en
    pair!("id", "en", "id-en", "c38ef36c843b71177da903442835c3c870975fd6",
        [47_674_782, "c5ab889ecf81e120e53f0f39800c0609d8d77b6c4a32212d73a85e3cfeb66293"],
        [54_946_692, "ed7c0c6aa53da96d0633aabf223ba164afb0394a6c5646b9070b903940473132"],
        [800_687, "2a8fefe71c7f26cb0c6aa1b9f0cc0f8d18006b20fe41c547af7f25b9c8333465"],
        [1_366_984, "cda834905a5d478c54a73abc98cc9155dc58217baa130cdf165411265b760d6b"],
        None, 54795),
    // Helsinki-NLP/opus-mt-vi-en
    pair!("vi", "en", "vi-en", "541dfd72ec07e8f1563795e0224408ad8d729e80",
        [47_133_597, "fa7bd78e8bed6bdc03728f62f1592c4bb52d9e12795496604c1e990e102abdb2"],
        [54_401_279, "699ef5a7a2c6ba1d9fa05879dd13adaccbc496454b08e3fb6919ecf70744a51a"],
        [756_168, "9deb4f9e7bebd3876f4bf238a9eb52f2f95a1fba2210a4a8239167c55c15e67d"],
        [1_301_077, "f84f995b939f2411294d0723df4aaf7853d0bd07143b530edec0697c7d75aa5d"],
        None, 53738),
    // Helsinki-NLP/opus-mt-th-en
    pair!("th", "en", "th-en", "7f0406b972f742985cfbe3be8f61dead9f1bc0e3",
        [51_520_414, "37e31edabcadfca300cf13d4dd20087aa1a920cba7b20c738cba8ff44f320bbf"],
        [58_822_368, "d47e46270210fdb53dcb98f14d5b9ebea3e297145f9604924de44e5bda2613fb"],
        [1_213_743, "01ecb88d0f92bbb20c21257b92e93e301a10971ecd60971e8e637bc3eacd9124"],
        [2_528_379, "0c199f23b384bedb69a496cd8d901d5d298d05a676cde5db15219a60cd2f5847"],
        None, 62306),
    // Helsinki-NLP/opus-mt-ko-en
    pair!("ko", "en", "ko-en", "d9fa1ac6008242100fecb4fff9b0a5917a1be81f",
        [52_899_741, "210e1d2cb5b457cdeeffe288099cc9d1df9f13c35dc3d39fde4ba252af877380"],
        [60_212_471, "3a8bd3efa1e0d79cb74552b08ff8b204ee9a564aa680fc0f32b37677306231b4"],
        [841_805, "c9496f7c2be9aecb84c751ae9f35a875915dde8e3892f652a5c76811ab2a0f49"],
        [1_849_870, "97197044d4803c664a37f8ca2ced9c3405b8a3df6b262a12da22d990b0d756cd"],
        None, 65000),
    // Helsinki-NLP/opus-mt-en-it
    pair!("en", "it", "en-it", "075406e3c8c2c30634d4a1bd8f00c21d9e162011",
        [60_597_150, "b51c5110c7ba356ef01ee400d427afe162e657de1d7a2e9a40deedd811b49e6f"],
        [67_970_016, "eb870c207294754cf62b743d75a2b81994c9fecb7c0955f1d601c16674bb2c9f"],
        [789_468, "2efff2c37e51842c2817a6770280979b5dd803cebbe2928e576d42a888f89537"],
        [2_511_647, "40c98ffc0013f2f09ed9127418131738ad9c2de4d8f7f52d52532332bb2bf78c"],
        None, 80034),
    // Helsinki-NLP/opus-mt-en-nl
    pair!("en", "nl", "en-nl", "c3a0541231a7329b4492809fc2bd7ab7d2ceb0bc",
        [53_937_565, "0febb990f8ca8ed9fd3e0017dfcb55ec5293999f20ead81d2bb4370e9e3120ff"],
        [61_258_403, "c41791e07e4995f06c719eb8917140d8150f3404d8fecbdaee2287a74bcdd5f8"],
        [789_525, "d05781fde84b8793191528a7a176e1b17f96265388f602379ee6ae3139f59141"],
        [1_794_274, "b1fba91c620564d90848c5f77b03f13d17949790f8e7c7741e4a8ea13b8b0d91"],
        None, 67027),
    // Helsinki-NLP/opus-mt-en-uk
    pair!("en", "uk", "en-uk", "e6ca587292984b343fd829b6430ed55858496525",
        [51_151_774, "234c3738c258b3b7c913452a4ab6203b1a24f459355af8ef563dd4d94792d087"],
        [58_450_848, "ec57db15ca5e7466242367332b56840f36ee33319d6ffb34297b52bcd71f2e9a"],
        [808_645, "4dc9157e60a15b157c2f3d5f892379b03aed111c9fe8ac7cecba5f2aa56379ef"],
        [2_490_886, "e8b45188b1db6a4e0f960813e1bd493948ab0bc4d57b069ea03e77a78b7567d4"],
        None, 61586),
    // Helsinki-NLP/opus-mt-en-hi
    pair!("en", "hi", "en-hi", "7aad72006a1588a9c3485ed6203aed99eebbc872",
        [51_337_630, "3045cd6eaf06db96cff853204d6f0ffeb0c5af238a4f5a87a19fcaa94f7493c4"],
        [58_638_156, "f50e7418ea2bd0856a591e857bdb1dccfc0c411cab27aa8587b621f4fd610a94"],
        [812_240, "fd4e951487aed00bae6a6c2ee4ef5d8d1db05fd098b19b608046c9334b58d24d"],
        [2_221_738, "be02f5dfb85054d4f25089f6b347b69d32e98ed4c3d181ed0c789904eb1e3d3b"],
        None, 61949),
    // Helsinki-NLP/opus-mt-en-id
    pair!("en", "id", "en-id", "b3b41f654c2fb3286d007dee1da7b0f47fd27d82",
        [47_674_781, "9bfc9cf0f400f86ec11da24b2f56380fcfabab17fb2dbcf7bb4c1754b7076e24"],
        [54_946_691, "2067ad87ac34662df919d750f3417f976592577d68b2d21aef7dc0f298673613"],
        [795_925, "e88300911c2c573ec5526777a1e84bae698d20925b82dcef9c7248bb0e537ed0"],
        [1_366_984, "cda834905a5d478c54a73abc98cc9155dc58217baa130cdf165411265b760d6b"],
        None, 54795),
    // Helsinki-NLP/opus-mt-en-vi
    pair!("en", "vi", "en-vi", "30bcd461c0d7338ad106b36314d3eedc2d4e998c",
        [47_105_950, "dfcd7a32be3fdd21cbcf5d354520d626f1768fd6e205ca62a6a11a4641d31f36"],
        [54_373_416, "0a294c7468ce910fec5018b96245210a5863b7c7de49bf081d633a95e22b83aa"],
        [808_612, "fdd9ab486ca8ea060967105e30db7d8928729c78a67448711d2dd7c3153b37cd"],
        [1_299_764, "bcc8bbafeb0b6b58fecf244f8da996da9aa45dbea7871164f9c8ce480507f9fe"],
        None, 53684),
    // Helsinki-NLP/opus-mt-de-es
    pair!("de", "es", "de-es", "d339419ee3bda301bdcd00249d4436b74664ee7e",
        [51_005_342, "54a64cdcdef126647fe4922b33744f6643982084ee0a58b6d441bec5d17f7deb"],
        [58_303_272, "c91ebc9b40c62d542c6c7dc3005911fb08581a054f374c762c8ffe4927fb0615"],
        [828_663, "4c6cf9cacbfbf82db361e3e07df76172cce7d1ef683d4e7d090349c84246caf0"],
        [1_588_376, "d5789816a8a925181ad70fd1c452cd48d6658040e74208512ce18eee55e8e1f9"],
        None, 61300),
    // Helsinki-NLP/opus-mt-de-fr
    pair!("de", "fr", "de-fr", "e783da03d247c4d16778e938530e9b5d7bd1523d",
        [50_929_565, "90cba5278e60ad6be15594760e718135640185b041d5706cba7762ece724d489"],
        [58_226_903, "3fec3907056b8d37d383d309b131111e80ea8dc751606d68edd7f5d0d2797383"],
        [827_737, "965802053f4a59f81d48283bd1160ed49950124891f8f532d98945de4f5e58eb"],
        [1_586_444, "d17e2f6f5ab525264096f440f74a0a4656af422b8617e58bd2202ea0c283e3f9"],
        None, 61152),
    // Helsinki-NLP/opus-mt-es-de
    pair!("es", "de", "es-de", "7f493e4b81b684478396f37de4c1ebc28ecd5855",
        [51_005_342, "bb5765b2b22eb48c2865c65098558cfd7e45a45f2f20aafb1cdc85a1f65c4de3"],
        [58_303_272, "c0a41b0291d1faf1ecdc73b5270a46d6bb6d1e9f82f8d5a684c85a80655f0978"],
        [818_345, "d15fb47311fe1d385213350041a25dcb2f2b0ceb7d7102b7853b1405b33fd794"],
        [1_588_376, "d5789816a8a925181ad70fd1c452cd48d6658040e74208512ce18eee55e8e1f9"],
        None, 61300),
    // Helsinki-NLP/opus-mt-es-fr
    pair!("es", "fr", "es-fr", "3bef5f612825d503f6fb85e0527a1fe30f2578c2",
        [57_928_094, "f2c497ebe1c3eadb329b8f36b250e7f87f9a327f46615dc5637492702cd0d2af"],
        [65_280_108, "230ee7786e0277e48941514ae946d192fa5325af9a26cb5229c1ad0c0be779d7"],
        [818_689, "0807c7b3dabe387a083700920a4b406c7656db09922bf934c25c2fab24706c4d"],
        [2_487_351, "00298f6b835a284fc55779eb3af674807e71984169682d5156ed92184b3b0f0a"],
        None, 74821),
    // Helsinki-NLP/opus-mt-es-it
    pair!("es", "it", "es-it", "596c67cda7cb919f0d67221f3a31bd7df79f19d0",
        [49_843_102, "9e4ba8017de817880e4ad24712015e5935147479d01c25af757af24d61e3fab0"],
        [57_131_952, "c578aee5f9213e61187b93eceed72e8ba97f8beec1dbbea90e48b3ea8ce5118e"],
        [818_750, "65513650404974598f0fb0a3156a243c12ad57c38fcc3367c2c6aef7ed4d3865"],
        [1_534_392, "38a184f18ecef75da7e18105aeaeb0e3286cbfc66ab4c03a5238938c3fd1ee86"],
        None, 59030),
    // Helsinki-NLP/opus-mt-es-ru
    pair!("es", "ru", "es-ru", "268ccd60bde3cabde17068d482ca6930512f1a85",
        [52_095_390, "04702c95f2380c074b9458735fffe44b095ef1a7632f0e13a7f7c7440d995241"],
        [59_401_836, "a711177ea202844be123724adc3c177f78b640a07cc656d943af2e2959e0aa49"],
        [829_382, "b309d8a58c506172b66c39fdd552a89792f3fb256f09e0e9a3dbfc0faefa636f"],
        [2_746_317, "d81cea81bca9898107f3eb07dfaed23252d4f922dec7552af7039a821a756db1"],
        None, 63429),
    // Helsinki-NLP/opus-mt-fr-de
    pair!("fr", "de", "fr-de", "7eea1996b4480d073cf560fcba6d4480aaf9a2bb",
        [50_929_566, "76e81a715482c10d25fe3cbfe1129b236adcdc75bac4d9cee37bc97dc61ee8dd"],
        [58_226_904, "26ada70f131c8fffbea44462c190dede181fc4e6411ace33278ef50d71a0122f"],
        [812_399, "e9a9acb44b4f88235ca6cc9f0b4546f942ba34fb1b27937b11386cc8db63ee83"],
        [1_586_444, "d17e2f6f5ab525264096f440f74a0a4656af422b8617e58bd2202ea0c283e3f9"],
        None, 61152),
    // Helsinki-NLP/opus-mt-fr-es
    pair!("fr", "es", "fr-es", "962338be51033b548c13dbb6f4da3ddb8c8aba0e",
        [57_928_094, "946954149b0179a0fb60145a3ed85da2f58868ef2e6a442a1cb4301563b70ac3"],
        [65_280_108, "2ad44e9c44b09f738ae2d8412f276b93d1adb0b51f24c1caa0cf5c084b3417f9"],
        [811_963, "74ffc930c4af87e3bdec6dde479eb36f05152e1a657ec65dfb923baf4b5d27a9"],
        [2_487_351, "00298f6b835a284fc55779eb3af674807e71984169682d5156ed92184b3b0f0a"],
        None, 74821),
    // Helsinki-NLP/opus-mt-fr-ru
    pair!("fr", "ru", "fr-ru", "c2ec732b045ae9f0e1ae1ea9267840145a8c03fe",
        [52_580_253, "058326a2132a61b250d117ca78b4b3b6f53e81479c3d3b6bebf3c58850618743"],
        [59_890_487, "f76aaff1e74e9e0314a72a0707ef236e7e5b9a6c0b99138bf390a899f7248434"],
        [820_963, "e852342099cfac79243a4ee0184eeb426814a84bb44b0cdf50516795d696e373"],
        [2_787_675, "1b9868de182074d2fc6d606d935b1c80bb18539a2dfc62670ef977571e44651c"],
        None, 64376),
    // Helsinki-NLP/opus-mt-it-es
    pair!("it", "es", "it-es", "0eba27fe6da2208231ea5e7047b0f62241ffc2bb",
        [49_843_102, "3e0ef961235dc192d99555fe015ac0c28c0a93d006e98e6050a13dc45f45bb88"],
        [57_131_952, "935a8b889645360b605451a96c92ade40be45584496fb377e2d342ea69421376"],
        [813_114, "03264679b2a1831b20d3ef2cc946ac99e3acab74ad965d77c637e8253ea30cd3"],
        [1_534_392, "38a184f18ecef75da7e18105aeaeb0e3286cbfc66ab4c03a5238938c3fd1ee86"],
        None, 59030),
    // Helsinki-NLP/opus-mt-it-fr
    pair!("it", "fr", "it-fr", "7843e3b7137d19face19cb32e27256888d26fe84",
        [50_006_942, "02efa49882a03300453fa6816ebc965baff791b280b2d03ac93316739768dd19"],
        [57_297_072, "fe60a7e329f527bb4f0c1ac904355356a7881ba194a59b4c66c50dabac182ff1"],
        [813_967, "7bdce190b91dcd93a386b7f584dbed229fa78a89d16ed0742ad1f348d9082005"],
        [1_535_967, "0929e03c27b4b39c41e97ed11c677aa076433d5d46684df53324a1d20198178f"],
        None, 59350),
    // Helsinki-NLP/opus-mt-ru-es
    pair!("ru", "es", "ru-es", "9cb5fb9fae4d5c0c731dcc3ba74c4e0a87f15eb0",
        [52_095_390, "0e48e0ea0f45b967a2994b992027915e1a3983086c40bd11c01c762c0bd36718"],
        [59_401_836, "de5aca8325d90629fd63c7fd5a9f40307182407982535069248183bc6afa4500"],
        [1_060_731, "581d0ce357f10ff41e4ab32b5771068257470676ec0d555ba73f36e7e7a1c99c"],
        [2_746_317, "d81cea81bca9898107f3eb07dfaed23252d4f922dec7552af7039a821a756db1"],
        None, 63429),
    // Helsinki-NLP/opus-mt-ru-fr
    pair!("ru", "fr", "ru-fr", "c7275f468246036d24bfe14ce8465e94e1037af9",
        [52_580_253, "2b2801424d0f56825cee08950e945189444d08773131c2e3386a71ce1f87fc72"],
        [59_890_487, "fe803a860bdcfa0aab2cefa3e9095c867e8ca51c91e1824d7ab7219f96c62b16"],
        [1_063_848, "50a16a6e7a21936d7a5cd1764c61364660353e2cebb2ed78e55b97bd74e85771"],
        [2_787_675, "1b9868de182074d2fc6d606d935b1c80bb18539a2dfc62670ef977571e44651c"],
        None, 64376),
    // Helsinki-NLP/opus-mt-ru-uk
    pair!("ru", "uk", "ru-uk", "6d8a744ed6eafd9003bcc2b052c4575de0485e1e",
        [49_201_054, "5d1531b6d1bad2a7168f82d2c74acbf9a625e4f2913fe7ff476b61b6c7ec4d9a"],
        [56_484_888, "198db0b935a13776ee0b6d2282eb9ddba90166a664517ce9e9d8011791b821d0"],
        [1_049_819, "d989ee1653a22aa05280110ca2abaccb8a77b397a64f14576984f0a9f9a8d2ec"],
        [3_474_975, "e0350396e721f2863a8182e93cad07ba19166ca1cc5ed7d08acfde89003ffea1"],
        None, 57776),
    // Helsinki-NLP/opus-mt-uk-ru
    pair!("uk", "ru", "uk-ru", "cfb76ccd207734d7847679664fa4538cda5ed597",
        [49_201_054, "6b599e2f4fbf1ea36cf983f1c0f40fcb2bb0ad8f6e0d1a7591da46b5ca9bf48a"],
        [56_484_888, "a52c00a861b44955850227f06db554d1d853dc922df960f3bfd9c809e14147ca"],
        [1_031_670, "1d8d0a245c07efca75da76f0c68e9d61bc9667af1ee81d230faf14869653b234"],
        [3_474_975, "e0350396e721f2863a8182e93cad07ba19166ca1cc5ed7d08acfde89003ffea1"],
        None, 57776),
    // Helsinki-NLP/opus-mt-nl-fr
    pair!("nl", "fr", "nl-fr", "6c8ebb68522ffd80cf3474b2dc44f4b5f741e9cd",
        [49_131_934, "357de84fa0df0fe4b8e95adb9d0061118a2a9a4602863390e5e7c2ff505b1b7c"],
        [56_415_228, "5acf1e46d4b6e216e6692fb66f2ad35ecfca6432c31ecd7c6c2767cbb054e2bb"],
        [814_095, "a2452ac5c28605107318ee5369f7a46b63692f2dcb584397a89fbb876a5e321d"],
        [1_462_960, "fcdfbeacc62d75c9d235b0882e74409561bee894fdb0336e83a0a1d357c9fa25"],
        None, 57641),
    // Direct into and out of Arabic, converted by us (`own`): one model
    // instead of two through English, about twice as fast; on the 2026-11-03
    // matrix pages as good or better (de-ar was worse than the pivot: not
    // here). Helsinki-NLP/opus-mt-fr-ar
    pair!(own, "fr", "ar", "fr-ar", "5469dc759cd9fd856e45b0a916001efe44d1e700",
        [51_577_879, "f7c318194076e252bcd028a91f7534bc05e65181842ceda203fc56f8f6d9b3f0"],
        [58_880_688, "83e0d1115910d4830bc98bc28da5b3dadacc993de130f72c74c812d2740573f3"],
        [826_663, "f32f818f2a89130ada3ab6b09a69cae104ae20ca5c9f1c309c41dae86a7dcc4e"],
        [2_188_546, "2223539759870b832c9389e623bcbfefe8606c6ee6fedbce03a46c1add6e3ae3"],
        Some(">>ara<<"), 62418),
    // Helsinki-NLP/opus-mt-es-ar
    pair!(own, "es", "ar", "es-ar", "76efb7da47ad1ad8169635a847c1bd9d1c1815a7",
        [51_628_567, "c21a425be06db48edc6c633f016e327801d2682ce224e93c98ab531ecb858a56"],
        [58_931_772, "68a89aa78cbff2afaf70b8d86262ffc23b296c6a74b34c316b716648390b12c6"],
        [831_392, "d3e1ec508e9f334e1e5285ff6ac31ae92f139d2e6b4121169b7cc61b8f270667"],
        [2_180_565, "632bac6ab53069e30188fb40389fe6246789b46a30e01796df3780056921005f"],
        Some(">>ara<<"), 62517),
    // Helsinki-NLP/opus-mt-ar-fr
    pair!(own, "ar", "fr", "ar-fr", "e507d7bc850855f060e6bbe4751bb03141b99b7d",
        [52_645_910, "0ea1bb14d25530cd3d7a16da7570c94312cf79de8f42f6ff4eebfc1a47b3a154"],
        [59_957_063, "704cebff0f82550db204ac2d0be9c514d0d7dcbfbf3dfeccb5a04b3aeabc8bca"],
        [918_273, "562fad8433a217ccafb56a30bad44a8d976fde802ee09e8b237f9cb281d1dcc7"],
        [2_227_927, "f9e204a0b757316d939a4b332a83b2ab5ca31bb88fd3915787cb23bb25357c57"],
        None, 64504),
    // Helsinki-NLP/opus-mt-ar-es
    pair!(own, "ar", "es", "ar-es", "da86e973e723a8196adc90ed17119da2e096c776",
        [51_632_662, "b9239b444b07047f189aba81bbda97f91f499ba991cfb5f1c62e48fa047419d1"],
        [58_935_899, "843298b031f231baabdf2912a53e72acec769c170f8129fbb65cdc696ebb6a9d"],
        [923_345, "faf17ff693fa03d84e690faeb2cd2374587a125c732c612c0f0cdc6bb0ae4a06"],
        [2_182_313, "e90e6b186669b0d5f8e921adc0d348e52705e4aa8c8c625b466e0cdec17ad975"],
        None, 62525),
    // Helsinki-NLP/opus-mt-ar-de
    pair!(own, "ar", "de", "ar-de", "8e6cbcce68feaf4eed2b926eccd31bfca97fab05",
        [51_915_799, "d113e37d62a817df8859c9d7d7826277e9eaeab1ad7bee5b205d624e04fe536c"],
        [59_221_248, "8f3fe121b9d05be779238cf65d0449057ba751d6e7dcc4578ec9f380873d4f27"],
        [902_787, "4ce7a1936b414ccb43fd8d411c556c9ea57ebe1eb50fd963b9eecaf1813eaad1"],
        [2_128_799, "af08b451ddd621880b739d077f05044e991dfe11ca49f0435cc03edff0e90146"],
        None, 63078),
];

/// models-v1's decoders (pair, size, sha256): the files the app downloaded
/// before models-v2. One already on a user's PC keeps translating
/// (`Pair::needed`) until its update (`update`, started by the pair's next
/// Translate or Settings' Update now) puts the v2 one in its place: v1 is
/// never a final state (owner, 2026-11-11, "installed 100 %").
#[rustfmt::skip]
const V1_DECODERS: [(&str, u64, &str); 49] = [
    ("en-ar", 59_078_120, "c0e553b0c26611780c70417c9b4c1ca7169e9cdb18ba9e649197efb017af9f02"),
    ("en-fr", 57_381_512, "333b244bce16023df04541c8cf9fd60aec9b0569da393c4b831d561897b0bda8"),
    ("en-es", 60_212_804, "58ee5ddd6e22d1693d8722b90c1486afb93700a4376cf28fd74175052ad530ae"),
    ("en-de", 56_652_404, "8b46a825964cdd182fe47cc780f0fb87d357eae1f47cf20aa63c1c09be5b510c"),
    ("en-ru", 58_931_576, "7efefcd793a5663bc204224a92a123524f3dfe2ffad59e5f73d6734d1b11ad04"),
    ("en-zh", 60_212_804, "023be4f841f4c47cd65fffcbaa81c0d99d7f7e0138f7ba0e03fa220a4e688aff"),
    ("ar-en", 59_094_632, "cdb25ae736afe9d48035b0ea311ab4b25f315810fab7f3c6f6cb0d8d87af35fa"),
    ("fr-en", 57_381_512, "73bc7ac8e29c42e6f212ebcc29a2991d3646a04aabb642c0036aa54b40e4e1a9"),
    ("zh-en", 60_212_804, "c6b7f04ff1ba0fbd1bf6852599b4c0cad6fe512d57cd887f44ef36cf705424cb"),
    ("ja-en", 58_001_744, "b304d0014e4e1575437b6af95467b6cb54405d923732d8359113bd6dbbee93c0"),
    ("es-en", 60_212_804, "4cd91ab30240d295ce907b5100826031838c672005e57e524d711122d75605fa"),
    ("de-en", 56_652_404, "4712800584b2d77c2f9e620bf29523e2b20b62f65ade5011ee01780e2b0ea5d3"),
    ("ru-en", 58_931_576, "25e7ff97b201bce64ab9c39749761d657b70b8602bcbc1ce01c67f713e90ffdf"),
    ("it-en", 68_147_852, "aebc2789cd2ef8062fbda020bac90b61e68458ba98a4c43cea63c1d2310ba2b8"),
    ("nl-en", 61_258_736, "ad4df9a08f16952c0d9ee06807f54d62fcd07a9109c2c0b8ea05a4422d7bf4d7"),
    ("uk-en", 58_451_180, "ffa6d28d5fa0ff4ed930969d9ca4bac2d80ff11172198e93dc6332db49ddb36b"),
    ("pl-en", 59_402_168, "9ad9da5bce473ca06a9b0a3dbf0793675827e5668ab501e5dc65dac8cecd93ba"),
    ("hi-en", 58_213_820, "52655659006c8ce742cc761282d49b3be4e73bfbf080f1b6a546aa96170d5813"),
    ("id-en", 54_947_024, "d65eeab370c2873bdf22a7b2b311f0f636aa693b8edd55c60b6b1db8473268dd"),
    ("vi-en", 54_401_611, "540edbcc08bab0b42c03576a759fd9b4d5fc99996ac3178630733916dc2325cb"),
    ("th-en", 58_822_700, "65b9ea697b1b3fc318ab8376d321dcba21b389c61939d13c21bf8b9044300082"),
    ("ko-en", 60_212_803, "e131732653a6d3cec42a8b65f8844980042b036bdaca931446bc4b3530bc3506"),
    ("en-it", 67_970_348, "f9db146ed7665ea7d437dc4df564e3af5df923366de4d52aaf2c4b4e3e4ae4f1"),
    ("en-nl", 61_258_735, "5f8034fddc4cdcb6fa6c3b64a06e1cd9b1955c0c6dcd13e0e47610ab99ac4a99"),
    ("en-uk", 58_451_180, "0892312ea1d359fb749b16deb5005246eb17974bd2b5424344b0e9ae83e0a142"),
    ("en-hi", 58_638_488, "8baba43a14b15ca4543d30ab36e7d34f2e516fb9af7e0b83b77a5e6a6f66d198"),
    ("en-id", 54_947_023, "0a59ebfe85137a21055d9f73d9f83a59487f0cff31db452c7db3b6cdeca8e7c2"),
    ("en-vi", 54_373_748, "1cfc6a84f9c189e6928c9710b281fae3f1f42d4ffbba109e7b4ec8e31ceb06dd"),
    ("de-es", 58_303_604, "8b426d5cafcc60359eeb3577d4becd021be84c4a9eefabef811a6b0dd10e0082"),
    ("de-fr", 58_227_235, "2b834e8f8fc9c5159c36127fca1126224299848e04b8aa1f2936255ba339f435"),
    ("es-de", 58_303_604, "cb6d365ceaefd8141ec6cc3dfa4e2c332d03ffc891ea732efb714e230a94cc2b"),
    ("es-fr", 65_280_440, "f6e43796a68566c95ab387277c223726ac988b307e875c239b340424eb057a92"),
    ("es-it", 57_132_284, "11ac94f23d5c31305d1ff16a8018292eec9b61d5514dd365de856bc6a744ef3f"),
    ("es-ru", 59_402_168, "70fed4fafbc55053da784680b37745224210da4678fbaf69d92c1f6dec3f4e00"),
    ("fr-de", 58_227_236, "d3f4dbe5fbdeae2668a189edfc17edd87876f6df7ff5282803d535c5980bd4a4"),
    ("fr-es", 65_280_440, "ff991f24d534a62db615697ed9555db80be28fbf9e5677d8ebb1f78c6aabc2c6"),
    ("fr-ru", 59_890_819, "6f2c1c7baf1c3d7ceed89708bfeabecb4c9987047b6046b85aa71c55580f4512"),
    ("it-es", 57_132_284, "147aa9f62b543cb7a7a119aabc89ce26b7defab212fb2486b2bff94e09986dce"),
    ("it-fr", 57_297_404, "062ee6c4f6c2318114239a681849667492667e21b5617458e73a27b1d2db23b7"),
    ("ru-es", 59_402_168, "c16a3a885622460e9654ba5c22e09768c0165c1a564935ed0de20520df36ac1d"),
    ("ru-fr", 59_890_819, "3d25927b7b8f58d46b69782193a55663f5738af397734c834249840c835d51eb"),
    ("ru-uk", 56_485_220, "7b28ef096a0f4dc8eb989ebe6e3ab0b56095f812e7b86258087a8cc6aa8b100b"),
    ("uk-ru", 56_485_220, "2a99fed7dd89b22fee3752b8f6c7ac5e81d563635cf8198f49ca551f498daf76"),
    ("nl-fr", 56_415_560, "7915cb6d03674552b6f7a3bcd4fe04e4dedd2a16fc120b4bcab39a03c6e6ecb4"),
    ("fr-ar", 58_881_020, "58c62d87c8827fc1e58f6cb511b916d7ac43dfa6937bd3a5d0fe020d746f01ba"),
    ("es-ar", 58_932_104, "928ba379dd657b283045f4835b3244c2552597ffc1f227e8d600779b731f8ec3"),
    ("ar-fr", 59_957_395, "b7ae3aeb3918ecccb829da241eea01ae4d6e82ac5c453de8ac31c57845d8222c"),
    ("ar-es", 58_936_231, "fa2983e9b9284a68c37805ad47f4fee38e48d396e374f120dd21766a6acc9ef5"),
    ("ar-de", 59_221_580, "450b83f72b7cfc71f6ee4480bfa4dd11bc95db80f493ee4f98418d7ae78f09f1"),
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

/// The models that take `source` into `target`, in order: the direct pair,
/// else through English (`source` -> en -> `target`, two models, a bit
/// slower and looser). `None` = no way offline.
pub fn route(source: &str, target: &str) -> Option<Vec<&'static Pair>> {
    if let Some(p) = pair(source, target) {
        return Some(vec![p]);
    }
    Some(vec![pair(source, "en")?, pair("en", target)?])
}

/// The model of `route` to download first (the first one missing), else
/// its first: the one a download prompt names.
pub fn next(source: &str, target: &str) -> Option<&'static Pair> {
    let route = route(source, target)?;
    (route.iter().find(|p| p.missing_bytes() > 0))
        .or(route.first())
        .copied()
}

/// Whether text in `source` ("auto" = whatever is detected) can be
/// translated into `target` offline: for "auto", some route into `target`.
pub fn available(source: &str, target: &str) -> bool {
    match source {
        lang::AUTO => PAIRS.iter().any(|p| route(p.source, target).is_some()),
        s => route(s, target).is_some(),
    }
}

/// What to say when no offline model translates `source` into `target`
/// (never a silent switch to the AI: the user chooses).
pub fn unavailable(source: &str, target: &str) -> String {
    if lang::find(source).is_some_and(|s| lang::find(target).is_some_and(|t| s.code == t.code)) {
        return format!(
            "the text is already in {}. Pick another language to translate into",
            lang::name(target)
        );
    }
    // Detect: "into Portuguese", never "from Detect into Portuguese"
    let from = match source {
        lang::AUTO => String::new(),
        s => format!("from {} ", lang::name(s)),
    };
    format!(
        "there is no local model {from}into {} yet (coming in a future \
         update). Choose AI as the engine in Settings > Translate (and set it up \
         in Settings > Ask AI) to translate it now",
        lang::name(target)
    )
}

/// Moves every pair's old-named folder (`opus-mt-<pair>`) to its OptimaMT
/// name, once per start, before any helper runs (the app's start path): the
/// models are the same files, nothing is downloaded. One directory listing
/// when there is nothing to move.
pub fn adopt_old_folders() {
    adopt_old_folders_in(&models_root(), |old, new| std::fs::rename(old, new));
}

/// `adopt_old_folders` in `root`, moving with `mv` (a test locks one).
/// Only the old folder: it is moved when the new name is free (one atomic
/// rename, a `.part` of the lighter-model update goes with it). Both there (a
/// crash, or a downgrade and an upgrade): the one with more files at their
/// pinned size stays (the new one on a tie), the other is removed; never both.
/// A move that fails (a file open on Windows) leaves the old folder in use
/// (`Pair::dir`) and is tried again at the next start.
fn adopt_old_folders_in(root: &Path, mv: impl Fn(&Path, &Path) -> std::io::Result<()>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let pair = (name.strip_prefix(OLD_DIR_PREFIX))
            .and_then(|rest| (PAIRS.iter()).find(|p| p.dir.strip_prefix(DIR_PREFIX) == Some(rest)));
        let Some(pair) = pair else {
            continue;
        };
        let (old, new) = (entry.path(), root.join(pair.dir));
        if new.exists() {
            let old_wins = pair.intact(&old) > pair.intact(&new);
            let loser = if old_wins { &new } else { &old };
            if let Err(e) = std::fs::remove_dir_all(loser) {
                crate::log!(
                    "[opencircle] models: cannot remove {}: {e}",
                    loser.display()
                );
                continue;
            }
            if !old_wins {
                crate::log!("[opencircle] models: removed the old copy {name}");
                continue;
            }
        }
        match mv(&old, &new) {
            Ok(()) => crate::log!("[opencircle] models: {name} is now {}", pair.dir),
            Err(e) => crate::log!("[opencircle] models: {name} stays for now: {e}"),
        }
    }
}

/// Per-user folder (Linux `~/.local/share/OpenCircle/mt`, Windows
/// `%LOCALAPPDATA%\OpenCircle\mt`), beside OCR's.
fn models_root() -> PathBuf {
    crate::paths::mt()
}

/// A pair's folder name starts with this, then `<source>-<target>`: the
/// OptimaMT name (the Hugging Face repos are `OptimaMT/<source>-<target>`).
/// (`pair!` spells it as a literal.)
const DIR_PREFIX: &str = "OptimaMT-";

/// What the folders were called before v0.1.7 (the models are Helsinki-NLP's
/// OPUS-MT, which About and NOTICE credit).
const OLD_DIR_PREFIX: &str = "opus-mt-";

impl Pair {
    /// Its folder: the OptimaMT-named one, or, until `adopt_old_folders` could
    /// move it (a locked folder on Windows: tried again at the next start),
    /// the old-named one that is still there, so translation never breaks.
    fn dir(&self) -> PathBuf {
        let new = models_root().join(self.dir);
        match new.exists() {
            true => new,
            false => {
                let old = models_root().join(self.old_dir());
                if old.exists() { old } else { new }
            }
        }
    }

    /// Its folder's name before v0.1.7.
    fn old_dir(&self) -> String {
        format!(
            "{OLD_DIR_PREFIX}{}",
            self.dir.strip_prefix(DIR_PREFIX).unwrap_or(self.dir)
        )
    }

    /// How many of its files are here at the pinned size (the decoder: v2 or
    /// v1) in `folder`: which of two copies of a pair is the whole one.
    fn intact(&self, folder: &Path) -> usize {
        let v1 = self.v1().map(|f| f.size);
        let decoder = |size: u64| has_size(&folder.join(self.decoder.name), size);
        let files = [&self.encoder, &self.spm, &self.vocab]
            .iter()
            .filter(|f| has_size(&folder.join(f.name), f.size))
            .count();
        files + usize::from(decoder(self.decoder.size) || v1.is_some_and(decoder))
    }

    /// "English → French local translation".
    pub fn label(&self) -> String {
        format!(
            "{} → {} local translation",
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

    /// Its models-v1 decoder's pin (`V1_DECODERS`).
    fn v1(&self) -> Option<ModelFile> {
        V1_DECODERS
            .iter()
            .find(|f| self.dir.strip_prefix(DIR_PREFIX) == Some(f.0))
            .map(|&(_, size, sha)| ModelFile::plain(self.decoder.name, size, sha, ""))
    }

    /// `files` without the decoder while its v1 file (`V1_DECODERS`) is on
    /// disk, `v1_ok` with it and nothing else is missing (`without_v1`).
    fn needed(
        &'static self,
        v1_ok: impl Fn(&Path, &ModelFile) -> bool,
    ) -> Vec<(PathBuf, &'static ModelFile)> {
        without_v1(self.files(), &self.decoder, self.v1().as_ref(), v1_ok)
    }

    /// Its decoder on disk, for the update to v2.
    fn decoder(&'static self) -> Decoder {
        Decoder {
            path: self.dir().join(self.decoder.name),
            v2: &self.decoder,
            v1: self.v1().map(|f| f.size),
        }
    }

    /// Its decoder on disk is still the older, heavier v1 one (it
    /// translates; `update` replaces it). Sizes only.
    pub fn outdated(&'static self) -> bool {
        self.decoder().outdated()
    }

    /// Bytes `update` still downloads for it (0: current, or the v2 file
    /// is here, waiting to be swapped in). Sizes only.
    pub fn update_bytes(&'static self) -> u64 {
        self.decoder().to_fetch()
    }

    /// Bytes still to download (0 = ready). Cheap: sizes only.
    pub fn missing_bytes(&'static self) -> u64 {
        models::missing_bytes(&self.needed(|path, f| has_size(path, f.size)))
    }

    /// Bytes of everything this pair needs, present or not: what `ensure`'s
    /// progress counts to.
    pub fn full_size(&'static self) -> u64 {
        self.files().iter().map(|(_, f)| f.size).sum()
    }

    /// Downloads and verifies whatever is missing (OCR's files too), with
    /// one `progress(done, total)`. Only after the user agreed.
    /// A v1 decoder that verifies is kept; else the v2 one replaces it.
    pub fn ensure(&'static self, progress: impl FnMut(u64, u64)) -> Result<(), OcrError> {
        models::ensure(&self.needed(verified), progress)
    }
}

/// `files` without `decoder` when its previous version `v1` is on disk and
/// `v1_ok`, and every other file is there (sizes): an upgrading user keeps
/// translating with it, nothing downloaded. Any download of the pair (a
/// file missing) fetches the v2 decoder too, over the v1 file (one copy).
fn without_v1<'a>(
    mut files: Vec<(PathBuf, &'a ModelFile)>,
    decoder: &ModelFile,
    v1: Option<&ModelFile>,
    v1_ok: impl Fn(&Path, &ModelFile) -> bool,
) -> Vec<(PathBuf, &'a ModelFile)> {
    let Some(at) = files.iter().position(|(_, f)| std::ptr::eq(*f, decoder)) else {
        return files;
    };
    let others = (files.iter().enumerate()).all(|(i, (path, f))| i == at || has_size(path, f.size));
    if others && v1.is_some_and(|v1| v1_ok(&files[at].0, v1)) {
        files.remove(at);
    }
    files
}

/// One pair's decoder file: its pin (v2) and its older v1 file's size.
/// States on disk: missing (the pair's download fetches v2) -> current; or
/// v1 -> (`update`) v1 + `<name>.v2.part` (resumed after a stop, a crash or
/// a restart) -> v1 + `<name>.v2.onnx` (verified) -> (`swap`, when no
/// session has it loaded) current, the v1 file gone in the same rename.
struct Decoder {
    path: PathBuf,
    v2: &'static ModelFile,
    v1: Option<u64>,
}

impl Decoder {
    /// Where its v2 file lands while the v1 one is in use (`.v2.part` while
    /// it comes: the downloader's own temp file).
    fn staged(&self) -> PathBuf {
        self.path.with_extension("v2.onnx")
    }

    fn outdated(&self) -> bool {
        self.v1.is_some_and(|size| has_size(&self.path, size))
    }

    fn to_fetch(&self) -> u64 {
        match self.outdated() && !has_size(&self.staged(), self.v2.size) {
            true => self.v2.size,
            false => 0,
        }
    }

    /// The downloaded v2 file takes the v1 one's place: ONE rename, so one
    /// decoder at rest and never a half-written one. Not while a session of
    /// this process has it loaded (`InUse`): Windows can't replace a file in
    /// use, so the rule is the same everywhere. A current decoder's
    /// leftovers (a stale `.v2.*` file) go. `true` = current now.
    fn swap(&self) -> bool {
        let staged = self.staged();
        if has_size(&self.path, self.v2.size) {
            for leftover in [staged.with_extension("part"), staged] {
                if leftover.exists() {
                    let _ = std::fs::remove_file(leftover);
                }
            }
            return true;
        }
        if !self.outdated() || !has_size(&staged, self.v2.size) || InUse::has(&self.path) {
            return false;
        }
        match std::fs::rename(&staged, &self.path) {
            Ok(()) => {
                crate::log!(
                    "[opencircle] {} is the lighter decoder now",
                    self.path.display()
                );
                true
            }
            // (Windows: another process has it open) the next swap tries again
            Err(e) => {
                crate::log!("[opencircle] {} not swapped yet: {e}", self.path.display());
                false
            }
        }
    }
}

/// The decoder files this process has loaded (while a `Translator` lives).
static LOADED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A decoder file loaded while this lives (`Decoder::swap` waits).
struct InUse(PathBuf);

impl InUse {
    fn new(path: PathBuf) -> InUse {
        LOADED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(path.clone());
        InUse(path)
    }

    fn has(path: &Path) -> bool {
        (LOADED.lock().unwrap_or_else(|e| e.into_inner()).iter()).any(|p| p == path)
    }
}

impl Drop for InUse {
    fn drop(&mut self) {
        let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = loaded.iter().position(|p| *p == self.0) {
            loaded.remove(i);
        }
    }
}

/// One update at a time (two would write the same `.part` file).
static UPDATING: Mutex<()> = Mutex::new(());
/// Settings' Cancel: the running update stops, its `.part` kept.
static STOP: AtomicBool = AtomicBool::new(false);

/// The installed pairs whose decoder is still v1, and how many pairs are
/// installed (a v1 or v2 decoder): Settings > Translate's row. Sizes only.
pub fn installed() -> (Vec<&'static Pair>, usize) {
    let mut all = 0;
    let mut old = Vec::new();
    for p in &PAIRS {
        let d = p.decoder();
        if d.outdated() {
            old.push(p);
            all += 1;
        } else if has_size(&d.path, d.v2.size) {
            all += 1;
        }
    }
    (old, all)
}

/// Replaces the v1 decoder of each of `pairs` that still has one by the v2
/// file: downloaded beside it by the one downloader (retries, resumes,
/// sha256 against `PAIRS`), then swapped in when no helper runs; else the
/// helper swaps it before it next loads the pair, or the last hold's end
/// does (`swap_finished`). Only after a Translate use or Update now. A
/// failure changes nothing (v1 still translates); the next one continues.
pub fn update(pairs: &[&'static Pair], progress: impl FnMut(u64, u64)) -> Result<(), OcrError> {
    let decoders: Vec<Decoder> = pairs.iter().map(|p| p.decoder()).collect();
    update_decoders(&decoders, progress, |swap| {
        ocr::when_idle(|| swap.iter().for_each(|d| _ = d.swap()))
    })
}

/// `update` of `decoders`; `when_idle` runs the swap when nothing has them
/// loaded (tests: their own rule).
fn update_decoders(
    decoders: &[Decoder],
    progress: impl FnMut(u64, u64),
    when_idle: impl FnOnce(&[Decoder]),
) -> Result<(), OcrError> {
    let _one = UPDATING.lock().unwrap_or_else(|e| e.into_inner());
    STOP.store(false, Ordering::Relaxed);
    let mut files: Vec<(PathBuf, &ModelFile)> = Vec::new();
    for d in decoders.iter().filter(|d| d.outdated()) {
        if !files.iter().any(|(p, _)| *p == d.staged()) {
            files.push((d.staged(), d.v2));
        }
    }
    let got = models::ensure_until(&files, progress, &STOP);
    when_idle(decoders);
    got
}

/// Stops the running `update` (Settings' Cancel); the next one continues.
pub fn stop_update() {
    STOP.store(true, Ordering::Relaxed);
}

/// Swaps in every finished update (two file sizes per pair): app side when
/// the helper ends (`ocr::when_idle`).
pub fn swap_finished() {
    for p in &PAIRS {
        p.decoder().swap();
    }
}

fn has_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == size)
}

/// `path` is exactly `f` (size, then sha256).
fn verified(path: &Path, f: &ModelFile) -> bool {
    has_size(path, f.size)
        && std::fs::File::open(path)
            .and_then(crate::update::verify::sha256)
            .is_ok_and(|h| h.iter().map(|b| format!("{b:02x}")).collect::<String>() == f.sha256)
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
    /// Its decoder file stays in place while the sessions live.
    _decoder: InUse,
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
        // a finished update goes in now: nothing here has the pair loaded
        let decoder = pair.decoder();
        decoder.swap();
        let in_use = InUse::new(decoder.path);
        ocr::init_ort(ort_dir)?;
        let dir = pair.dir();
        // Every batch has a new shape: memory patterns would only add RAM.
        // No spin-waiting threads after a run (they slowed the app, see OCR).
        // The decoder's output layer is DequantizeLinear(int8 weights) -> MatMul:
        // ORT keeps a DequantizeLinear for QDQ fusion, so every decoding step
        // made the 62k x 512 float matrix again (~70 ms a step, 2.8x the
        // whole page). Without QDQ handling (these models have none: they use
        // MatMulInteger) it is folded once at load: ~130 MB more in the
        // helper, the same translations (once: the v2 decoders build it
        // outside the two branches, the original ones had it in both).
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
            _decoder: in_use,
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
/// (they would become accent spans; MT never marks names). Some OPUS-MT
/// models learnt from escaped text and write `&apos;` (ar-fr: "d &apos; un
/// compte"): written as the characters.
fn clean(text: &str) -> String {
    (text.replace("[[", "").replace("]]", ""))
        .replace(" &apos; ", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .trim()
        .to_string()
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
        assert!(available("it", "ar") && route("pt", "en").is_none());
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
        let why = unavailable(lang::AUTO, "pt");
        assert!(
            why.starts_with("there is no local model into Portuguese yet"),
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

    /// Every pair's decoder is the lighter models-v2 one, of another size
    /// than its v1 file (the two are told apart by size); encoders and
    /// tokenizers keep their v1 pins.
    #[test]
    fn every_decoder_is_the_lighter_v2_one() {
        for p in &PAIRS {
            let url = format!(
                "\"https://github.com/AmerAlreyahi/OpenCircle-models/releases/download/\
                 models-v2/opus-mt-{}-{}-decoder_model_merged_quantized.onnx\"",
                p.source, p.target
            );
            assert!(format!("{:?}", p.decoder).contains(&url), "{}", p.dir);
            let v1 = V1_DECODERS
                .iter()
                .find(|f| p.dir == format!("{DIR_PREFIX}{}", f.0));
            assert_ne!(p.decoder.size, v1.unwrap().1, "{}", p.dir);
            assert!(
                !format!("{:?}", p.encoder).contains("models-v2"),
                "{}",
                p.dir
            );
        }
    }

    /// A pinned file with `bytes` as its content.
    fn pinned(name: &'static str, bytes: &[u8]) -> &'static ModelFile {
        let hash = crate::update::verify::sha256(bytes).unwrap();
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
        let names =
            |l: Vec<(PathBuf, &ModelFile)>| l.iter().map(|(_, f)| f.name).collect::<Vec<_>>();
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
        let (v2, seen) =
            models::tests::fake_server("decoder.onnx", models::tests::payload(), answers);
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
        let removed = crate::paths::user_data_in(Some(&data), Some(&config));
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
        let mut tr = Translator::load(p, &ocr::models_dir()).unwrap();
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
            let mut tr = Translator::load(p, &ocr::models_dir()).unwrap();
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
                let mut tr = Translator::load(p, &ocr::models_dir()).unwrap();
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
