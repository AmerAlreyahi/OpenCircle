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
pub static PAIRS: [Pair; 44] = [
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
    // Helsinki-NLP/opus-mt-es-en
    pair!(
        "es",
        "en",
        "es-en",
        "eadfd7c658a9d8929ac3b8e996b68a68e2c7d480",
        [
            52_899_742,
            "c01e70f8455efc350831aa2af7ed187b16829241f0018ba07d1ba643a391bc18"
        ],
        [
            60_212_804,
            "4cd91ab30240d295ce907b5100826031838c672005e57e524d711122d75605fa"
        ],
        [
            825_924,
            "e236ee6d866b635c0142114f8647f39831f9d92534aa2aad75c942f6a78ad0e3"
        ],
        [
            1_720_044,
            "b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163"
        ],
        None,
        65000
    ),
    // Helsinki-NLP/opus-mt-de-en
    pair!(
        "de",
        "en",
        "de-en",
        "399dfd68706739fffd503f876093e455ae268a06",
        [
            49_366_942,
            "4cedda8f8c89b72a42b3c6cd1e7a27f2de24457093e3bf80cb3e46829641fcd8"
        ],
        [
            56_652_404,
            "4712800584b2d77c2f9e620bf29523e2b20b62f65ade5011ee01780e2b0ea5d3"
        ],
        [
            796_845,
            "bbd1f495eea99c8e21ae086d9146e0fa7b096c3dfdd9ba07ab8b631889df5c9b"
        ],
        [
            1_389_436,
            "d5acea957b265a78554999144459c5e391e0df525864edc8287bc090290baa44"
        ],
        None,
        58100
    ),
    // Helsinki-NLP/opus-mt-ru-en
    pair!(
        "ru",
        "en",
        "ru-en",
        "afe8c6c738ec81b6d033fd8f44f9678a639a7c67",
        [
            51_628_446,
            "c10df0466540534bb70c4402d539c349b8bfa9968184cad89f51408cf13e7eb7"
        ],
        [
            58_931_576,
            "25e7ff97b201bce64ab9c39749761d657b70b8602bcbc1ce01c67f713e90ffdf"
        ],
        [
            1_080_169,
            "745998e51ba5b058e38b7ac7765c25c43ed5c1c39cc92b27163b9b2e323c9d7c"
        ],
        [
            2_726_796,
            "5cf0d95d930d8d3e783c9e2f46a72f08b43a18060dab4ddefbcb66a733efedcb"
        ],
        None,
        62517
    ),
    // Helsinki-NLP/opus-mt-it-en
    pair!(
        "it",
        "en",
        "it-en",
        "fd0b89b9c052adc1f2f64152f555aa17353728be",
        [
            60_773_278,
            "a025d2d85c90da23880116b67175e43e71921d8d2bf446b9f1eacdc86015aa4c"
        ],
        [
            68_147_852,
            "aebc2789cd2ef8062fbda020bac90b61e68458ba98a4c43cea63c1d2310ba2b8"
        ],
        [
            813_709,
            "1bd307e2756991d470e4b40172147f7889b46a323b1a9a2150e66274825b1f21"
        ],
        [
            2_530_593,
            "08e5b03ab20b2b9b85d41aed1b36dce2650afcb6e96274a06b7869b2e06884d5"
        ],
        None,
        80378
    ),
    // Helsinki-NLP/opus-mt-nl-en
    pair!(
        "nl",
        "en",
        "nl-en",
        "82c42d95d1508037cbe7172d85fd4e940a2f3584",
        [
            53_937_566,
            "7b8b1e9071bed8e2394e3bd2f65b952700b1dd1293d087141134c8bac018653b"
        ],
        [
            61_258_736,
            "ad4df9a08f16952c0d9ee06807f54d62fcd07a9109c2c0b8ea05a4422d7bf4d7"
        ],
        [
            813_866,
            "f538d23d4b30641cd97c608d2a83890d503aa1286a5560f19ef949404a786e94"
        ],
        [
            1_794_274,
            "b1fba91c620564d90848c5f77b03f13d17949790f8e7c7741e4a8ea13b8b0d91"
        ],
        None,
        67027
    ),
    // Helsinki-NLP/opus-mt-uk-en
    pair!(
        "uk",
        "en",
        "uk-en",
        "b2ad6afa424aaeff8913ff350860b7a7b7c0a607",
        [
            51_151_774,
            "5e8c4c7e88742efa0a8e013f3897f17e29357d54b8c86a13472669546d6ed561"
        ],
        [
            58_451_180,
            "ffa6d28d5fa0ff4ed930969d9ca4bac2d80ff11172198e93dc6332db49ddb36b"
        ],
        [
            1_007_605,
            "7abd810b2df512e6fe177f8732f6a0b3107614c84cc52aba20d805b4cb0d7b6b"
        ],
        [
            2_490_886,
            "e8b45188b1db6a4e0f960813e1bd493948ab0bc4d57b069ea03e77a78b7567d4"
        ],
        None,
        61586
    ),
    // Helsinki-NLP/opus-mt-pl-en
    pair!(
        "pl",
        "en",
        "pl-en",
        "aeaf0e003b045248c28a559a5ce6027b54ebdaba",
        [
            52_095_390,
            "2416eb3c6db52a2d116539bf673b596e6a3d1453153b17778e85c9f55249c832"
        ],
        [
            59_402_168,
            "9ad9da5bce473ca06a9b0a3dbf0793675827e5668ab501e5dc65dac8cecd93ba"
        ],
        [
            834_921,
            "86b3b5f9de4f5ed9c46321bab1f865e3aa9ddf63094e89b49dd515aeafa79ce4"
        ],
        [
            1_642_111,
            "bd1a4fc17902ffffd8266b1b734c67ce64ce2d9e37a9da8c1f160d1edef69794"
        ],
        None,
        63429
    ),
    // Helsinki-NLP/opus-mt-hi-en
    pair!(
        "hi",
        "en",
        "hi-en",
        "99853100ed9d932987a6816e8cf7b1b6742a6116",
        [
            50_916_254,
            "dc5a1b084a83019bbb9798809dea5f2d16cc30a9a257238ec93120b3c2892ebf"
        ],
        [
            58_213_820,
            "52655659006c8ce742cc761282d49b3be4e73bfbf080f1b6a546aa96170d5813"
        ],
        [
            1_057_690,
            "dd20c408ec4568361d32b41080e3f6fa5b82895135bd25c2747b2c145e42d07b"
        ],
        [
            2_179_709,
            "83bf3ef5541c374f1796405a135ffa24d50665f1fcfdb5825f603c70166b4740"
        ],
        None,
        61126
    ),
    // Helsinki-NLP/opus-mt-id-en
    pair!(
        "id",
        "en",
        "id-en",
        "c38ef36c843b71177da903442835c3c870975fd6",
        [
            47_674_782,
            "c5ab889ecf81e120e53f0f39800c0609d8d77b6c4a32212d73a85e3cfeb66293"
        ],
        [
            54_947_024,
            "d65eeab370c2873bdf22a7b2b311f0f636aa693b8edd55c60b6b1db8473268dd"
        ],
        [
            800_687,
            "2a8fefe71c7f26cb0c6aa1b9f0cc0f8d18006b20fe41c547af7f25b9c8333465"
        ],
        [
            1_366_984,
            "cda834905a5d478c54a73abc98cc9155dc58217baa130cdf165411265b760d6b"
        ],
        None,
        54795
    ),
    // Helsinki-NLP/opus-mt-vi-en
    pair!(
        "vi",
        "en",
        "vi-en",
        "541dfd72ec07e8f1563795e0224408ad8d729e80",
        [
            47_133_597,
            "fa7bd78e8bed6bdc03728f62f1592c4bb52d9e12795496604c1e990e102abdb2"
        ],
        [
            54_401_611,
            "540edbcc08bab0b42c03576a759fd9b4d5fc99996ac3178630733916dc2325cb"
        ],
        [
            756_168,
            "9deb4f9e7bebd3876f4bf238a9eb52f2f95a1fba2210a4a8239167c55c15e67d"
        ],
        [
            1_301_077,
            "f84f995b939f2411294d0723df4aaf7853d0bd07143b530edec0697c7d75aa5d"
        ],
        None,
        53738
    ),
    // Helsinki-NLP/opus-mt-th-en
    pair!(
        "th",
        "en",
        "th-en",
        "7f0406b972f742985cfbe3be8f61dead9f1bc0e3",
        [
            51_520_414,
            "37e31edabcadfca300cf13d4dd20087aa1a920cba7b20c738cba8ff44f320bbf"
        ],
        [
            58_822_700,
            "65b9ea697b1b3fc318ab8376d321dcba21b389c61939d13c21bf8b9044300082"
        ],
        [
            1_213_743,
            "01ecb88d0f92bbb20c21257b92e93e301a10971ecd60971e8e637bc3eacd9124"
        ],
        [
            2_528_379,
            "0c199f23b384bedb69a496cd8d901d5d298d05a676cde5db15219a60cd2f5847"
        ],
        None,
        62306
    ),
    // Helsinki-NLP/opus-mt-ko-en
    pair!(
        "ko",
        "en",
        "ko-en",
        "d9fa1ac6008242100fecb4fff9b0a5917a1be81f",
        [
            52_899_741,
            "210e1d2cb5b457cdeeffe288099cc9d1df9f13c35dc3d39fde4ba252af877380"
        ],
        [
            60_212_803,
            "e131732653a6d3cec42a8b65f8844980042b036bdaca931446bc4b3530bc3506"
        ],
        [
            841_805,
            "c9496f7c2be9aecb84c751ae9f35a875915dde8e3892f652a5c76811ab2a0f49"
        ],
        [
            1_849_870,
            "97197044d4803c664a37f8ca2ced9c3405b8a3df6b262a12da22d990b0d756cd"
        ],
        None,
        65000
    ),
    // Helsinki-NLP/opus-mt-en-it
    pair!(
        "en",
        "it",
        "en-it",
        "075406e3c8c2c30634d4a1bd8f00c21d9e162011",
        [
            60_597_150,
            "b51c5110c7ba356ef01ee400d427afe162e657de1d7a2e9a40deedd811b49e6f"
        ],
        [
            67_970_348,
            "f9db146ed7665ea7d437dc4df564e3af5df923366de4d52aaf2c4b4e3e4ae4f1"
        ],
        [
            789_468,
            "2efff2c37e51842c2817a6770280979b5dd803cebbe2928e576d42a888f89537"
        ],
        [
            2_511_647,
            "40c98ffc0013f2f09ed9127418131738ad9c2de4d8f7f52d52532332bb2bf78c"
        ],
        None,
        80034
    ),
    // Helsinki-NLP/opus-mt-en-nl
    pair!(
        "en",
        "nl",
        "en-nl",
        "c3a0541231a7329b4492809fc2bd7ab7d2ceb0bc",
        [
            53_937_565,
            "0febb990f8ca8ed9fd3e0017dfcb55ec5293999f20ead81d2bb4370e9e3120ff"
        ],
        [
            61_258_735,
            "5f8034fddc4cdcb6fa6c3b64a06e1cd9b1955c0c6dcd13e0e47610ab99ac4a99"
        ],
        [
            789_525,
            "d05781fde84b8793191528a7a176e1b17f96265388f602379ee6ae3139f59141"
        ],
        [
            1_794_274,
            "b1fba91c620564d90848c5f77b03f13d17949790f8e7c7741e4a8ea13b8b0d91"
        ],
        None,
        67027
    ),
    // Helsinki-NLP/opus-mt-en-uk
    pair!(
        "en",
        "uk",
        "en-uk",
        "e6ca587292984b343fd829b6430ed55858496525",
        [
            51_151_774,
            "234c3738c258b3b7c913452a4ab6203b1a24f459355af8ef563dd4d94792d087"
        ],
        [
            58_451_180,
            "0892312ea1d359fb749b16deb5005246eb17974bd2b5424344b0e9ae83e0a142"
        ],
        [
            808_645,
            "4dc9157e60a15b157c2f3d5f892379b03aed111c9fe8ac7cecba5f2aa56379ef"
        ],
        [
            2_490_886,
            "e8b45188b1db6a4e0f960813e1bd493948ab0bc4d57b069ea03e77a78b7567d4"
        ],
        None,
        61586
    ),
    // Helsinki-NLP/opus-mt-en-hi
    pair!(
        "en",
        "hi",
        "en-hi",
        "7aad72006a1588a9c3485ed6203aed99eebbc872",
        [
            51_337_630,
            "3045cd6eaf06db96cff853204d6f0ffeb0c5af238a4f5a87a19fcaa94f7493c4"
        ],
        [
            58_638_488,
            "8baba43a14b15ca4543d30ab36e7d34f2e516fb9af7e0b83b77a5e6a6f66d198"
        ],
        [
            812_240,
            "fd4e951487aed00bae6a6c2ee4ef5d8d1db05fd098b19b608046c9334b58d24d"
        ],
        [
            2_221_738,
            "be02f5dfb85054d4f25089f6b347b69d32e98ed4c3d181ed0c789904eb1e3d3b"
        ],
        None,
        61949
    ),
    // Helsinki-NLP/opus-mt-en-id
    pair!(
        "en",
        "id",
        "en-id",
        "b3b41f654c2fb3286d007dee1da7b0f47fd27d82",
        [
            47_674_781,
            "9bfc9cf0f400f86ec11da24b2f56380fcfabab17fb2dbcf7bb4c1754b7076e24"
        ],
        [
            54_947_023,
            "0a59ebfe85137a21055d9f73d9f83a59487f0cff31db452c7db3b6cdeca8e7c2"
        ],
        [
            795_925,
            "e88300911c2c573ec5526777a1e84bae698d20925b82dcef9c7248bb0e537ed0"
        ],
        [
            1_366_984,
            "cda834905a5d478c54a73abc98cc9155dc58217baa130cdf165411265b760d6b"
        ],
        None,
        54795
    ),
    // Helsinki-NLP/opus-mt-en-vi
    pair!(
        "en",
        "vi",
        "en-vi",
        "30bcd461c0d7338ad106b36314d3eedc2d4e998c",
        [
            47_105_950,
            "dfcd7a32be3fdd21cbcf5d354520d626f1768fd6e205ca62a6a11a4641d31f36"
        ],
        [
            54_373_748,
            "1cfc6a84f9c189e6928c9710b281fae3f1f42d4ffbba109e7b4ec8e31ceb06dd"
        ],
        [
            808_612,
            "fdd9ab486ca8ea060967105e30db7d8928729c78a67448711d2dd7c3153b37cd"
        ],
        [
            1_299_764,
            "bcc8bbafeb0b6b58fecf244f8da996da9aa45dbea7871164f9c8ce480507f9fe"
        ],
        None,
        53684
    ),
    // Helsinki-NLP/opus-mt-de-es
    pair!(
        "de",
        "es",
        "de-es",
        "d339419ee3bda301bdcd00249d4436b74664ee7e",
        [
            51_005_342,
            "54a64cdcdef126647fe4922b33744f6643982084ee0a58b6d441bec5d17f7deb"
        ],
        [
            58_303_604,
            "8b426d5cafcc60359eeb3577d4becd021be84c4a9eefabef811a6b0dd10e0082"
        ],
        [
            828_663,
            "4c6cf9cacbfbf82db361e3e07df76172cce7d1ef683d4e7d090349c84246caf0"
        ],
        [
            1_588_376,
            "d5789816a8a925181ad70fd1c452cd48d6658040e74208512ce18eee55e8e1f9"
        ],
        None,
        61300
    ),
    // Helsinki-NLP/opus-mt-de-fr
    pair!(
        "de",
        "fr",
        "de-fr",
        "e783da03d247c4d16778e938530e9b5d7bd1523d",
        [
            50_929_565,
            "90cba5278e60ad6be15594760e718135640185b041d5706cba7762ece724d489"
        ],
        [
            58_227_235,
            "2b834e8f8fc9c5159c36127fca1126224299848e04b8aa1f2936255ba339f435"
        ],
        [
            827_737,
            "965802053f4a59f81d48283bd1160ed49950124891f8f532d98945de4f5e58eb"
        ],
        [
            1_586_444,
            "d17e2f6f5ab525264096f440f74a0a4656af422b8617e58bd2202ea0c283e3f9"
        ],
        None,
        61152
    ),
    // Helsinki-NLP/opus-mt-es-de
    pair!(
        "es",
        "de",
        "es-de",
        "7f493e4b81b684478396f37de4c1ebc28ecd5855",
        [
            51_005_342,
            "bb5765b2b22eb48c2865c65098558cfd7e45a45f2f20aafb1cdc85a1f65c4de3"
        ],
        [
            58_303_604,
            "cb6d365ceaefd8141ec6cc3dfa4e2c332d03ffc891ea732efb714e230a94cc2b"
        ],
        [
            818_345,
            "d15fb47311fe1d385213350041a25dcb2f2b0ceb7d7102b7853b1405b33fd794"
        ],
        [
            1_588_376,
            "d5789816a8a925181ad70fd1c452cd48d6658040e74208512ce18eee55e8e1f9"
        ],
        None,
        61300
    ),
    // Helsinki-NLP/opus-mt-es-fr
    pair!(
        "es",
        "fr",
        "es-fr",
        "3bef5f612825d503f6fb85e0527a1fe30f2578c2",
        [
            57_928_094,
            "f2c497ebe1c3eadb329b8f36b250e7f87f9a327f46615dc5637492702cd0d2af"
        ],
        [
            65_280_440,
            "f6e43796a68566c95ab387277c223726ac988b307e875c239b340424eb057a92"
        ],
        [
            818_689,
            "0807c7b3dabe387a083700920a4b406c7656db09922bf934c25c2fab24706c4d"
        ],
        [
            2_487_351,
            "00298f6b835a284fc55779eb3af674807e71984169682d5156ed92184b3b0f0a"
        ],
        None,
        74821
    ),
    // Helsinki-NLP/opus-mt-es-it
    pair!(
        "es",
        "it",
        "es-it",
        "596c67cda7cb919f0d67221f3a31bd7df79f19d0",
        [
            49_843_102,
            "9e4ba8017de817880e4ad24712015e5935147479d01c25af757af24d61e3fab0"
        ],
        [
            57_132_284,
            "11ac94f23d5c31305d1ff16a8018292eec9b61d5514dd365de856bc6a744ef3f"
        ],
        [
            818_750,
            "65513650404974598f0fb0a3156a243c12ad57c38fcc3367c2c6aef7ed4d3865"
        ],
        [
            1_534_392,
            "38a184f18ecef75da7e18105aeaeb0e3286cbfc66ab4c03a5238938c3fd1ee86"
        ],
        None,
        59030
    ),
    // Helsinki-NLP/opus-mt-es-ru
    pair!(
        "es",
        "ru",
        "es-ru",
        "268ccd60bde3cabde17068d482ca6930512f1a85",
        [
            52_095_390,
            "04702c95f2380c074b9458735fffe44b095ef1a7632f0e13a7f7c7440d995241"
        ],
        [
            59_402_168,
            "70fed4fafbc55053da784680b37745224210da4678fbaf69d92c1f6dec3f4e00"
        ],
        [
            829_382,
            "b309d8a58c506172b66c39fdd552a89792f3fb256f09e0e9a3dbfc0faefa636f"
        ],
        [
            2_746_317,
            "d81cea81bca9898107f3eb07dfaed23252d4f922dec7552af7039a821a756db1"
        ],
        None,
        63429
    ),
    // Helsinki-NLP/opus-mt-fr-de
    pair!(
        "fr",
        "de",
        "fr-de",
        "7eea1996b4480d073cf560fcba6d4480aaf9a2bb",
        [
            50_929_566,
            "76e81a715482c10d25fe3cbfe1129b236adcdc75bac4d9cee37bc97dc61ee8dd"
        ],
        [
            58_227_236,
            "d3f4dbe5fbdeae2668a189edfc17edd87876f6df7ff5282803d535c5980bd4a4"
        ],
        [
            812_399,
            "e9a9acb44b4f88235ca6cc9f0b4546f942ba34fb1b27937b11386cc8db63ee83"
        ],
        [
            1_586_444,
            "d17e2f6f5ab525264096f440f74a0a4656af422b8617e58bd2202ea0c283e3f9"
        ],
        None,
        61152
    ),
    // Helsinki-NLP/opus-mt-fr-es
    pair!(
        "fr",
        "es",
        "fr-es",
        "962338be51033b548c13dbb6f4da3ddb8c8aba0e",
        [
            57_928_094,
            "946954149b0179a0fb60145a3ed85da2f58868ef2e6a442a1cb4301563b70ac3"
        ],
        [
            65_280_440,
            "ff991f24d534a62db615697ed9555db80be28fbf9e5677d8ebb1f78c6aabc2c6"
        ],
        [
            811_963,
            "74ffc930c4af87e3bdec6dde479eb36f05152e1a657ec65dfb923baf4b5d27a9"
        ],
        [
            2_487_351,
            "00298f6b835a284fc55779eb3af674807e71984169682d5156ed92184b3b0f0a"
        ],
        None,
        74821
    ),
    // Helsinki-NLP/opus-mt-fr-ru
    pair!(
        "fr",
        "ru",
        "fr-ru",
        "c2ec732b045ae9f0e1ae1ea9267840145a8c03fe",
        [
            52_580_253,
            "058326a2132a61b250d117ca78b4b3b6f53e81479c3d3b6bebf3c58850618743"
        ],
        [
            59_890_819,
            "6f2c1c7baf1c3d7ceed89708bfeabecb4c9987047b6046b85aa71c55580f4512"
        ],
        [
            820_963,
            "e852342099cfac79243a4ee0184eeb426814a84bb44b0cdf50516795d696e373"
        ],
        [
            2_787_675,
            "1b9868de182074d2fc6d606d935b1c80bb18539a2dfc62670ef977571e44651c"
        ],
        None,
        64376
    ),
    // Helsinki-NLP/opus-mt-it-es
    pair!(
        "it",
        "es",
        "it-es",
        "0eba27fe6da2208231ea5e7047b0f62241ffc2bb",
        [
            49_843_102,
            "3e0ef961235dc192d99555fe015ac0c28c0a93d006e98e6050a13dc45f45bb88"
        ],
        [
            57_132_284,
            "147aa9f62b543cb7a7a119aabc89ce26b7defab212fb2486b2bff94e09986dce"
        ],
        [
            813_114,
            "03264679b2a1831b20d3ef2cc946ac99e3acab74ad965d77c637e8253ea30cd3"
        ],
        [
            1_534_392,
            "38a184f18ecef75da7e18105aeaeb0e3286cbfc66ab4c03a5238938c3fd1ee86"
        ],
        None,
        59030
    ),
    // Helsinki-NLP/opus-mt-it-fr
    pair!(
        "it",
        "fr",
        "it-fr",
        "7843e3b7137d19face19cb32e27256888d26fe84",
        [
            50_006_942,
            "02efa49882a03300453fa6816ebc965baff791b280b2d03ac93316739768dd19"
        ],
        [
            57_297_404,
            "062ee6c4f6c2318114239a681849667492667e21b5617458e73a27b1d2db23b7"
        ],
        [
            813_967,
            "7bdce190b91dcd93a386b7f584dbed229fa78a89d16ed0742ad1f348d9082005"
        ],
        [
            1_535_967,
            "0929e03c27b4b39c41e97ed11c677aa076433d5d46684df53324a1d20198178f"
        ],
        None,
        59350
    ),
    // Helsinki-NLP/opus-mt-ru-es
    pair!(
        "ru",
        "es",
        "ru-es",
        "9cb5fb9fae4d5c0c731dcc3ba74c4e0a87f15eb0",
        [
            52_095_390,
            "0e48e0ea0f45b967a2994b992027915e1a3983086c40bd11c01c762c0bd36718"
        ],
        [
            59_402_168,
            "c16a3a885622460e9654ba5c22e09768c0165c1a564935ed0de20520df36ac1d"
        ],
        [
            1_060_731,
            "581d0ce357f10ff41e4ab32b5771068257470676ec0d555ba73f36e7e7a1c99c"
        ],
        [
            2_746_317,
            "d81cea81bca9898107f3eb07dfaed23252d4f922dec7552af7039a821a756db1"
        ],
        None,
        63429
    ),
    // Helsinki-NLP/opus-mt-ru-fr
    pair!(
        "ru",
        "fr",
        "ru-fr",
        "c7275f468246036d24bfe14ce8465e94e1037af9",
        [
            52_580_253,
            "2b2801424d0f56825cee08950e945189444d08773131c2e3386a71ce1f87fc72"
        ],
        [
            59_890_819,
            "3d25927b7b8f58d46b69782193a55663f5738af397734c834249840c835d51eb"
        ],
        [
            1_063_848,
            "50a16a6e7a21936d7a5cd1764c61364660353e2cebb2ed78e55b97bd74e85771"
        ],
        [
            2_787_675,
            "1b9868de182074d2fc6d606d935b1c80bb18539a2dfc62670ef977571e44651c"
        ],
        None,
        64376
    ),
    // Helsinki-NLP/opus-mt-ru-uk
    pair!(
        "ru",
        "uk",
        "ru-uk",
        "6d8a744ed6eafd9003bcc2b052c4575de0485e1e",
        [
            49_201_054,
            "5d1531b6d1bad2a7168f82d2c74acbf9a625e4f2913fe7ff476b61b6c7ec4d9a"
        ],
        [
            56_485_220,
            "7b28ef096a0f4dc8eb989ebe6e3ab0b56095f812e7b86258087a8cc6aa8b100b"
        ],
        [
            1_049_819,
            "d989ee1653a22aa05280110ca2abaccb8a77b397a64f14576984f0a9f9a8d2ec"
        ],
        [
            3_474_975,
            "e0350396e721f2863a8182e93cad07ba19166ca1cc5ed7d08acfde89003ffea1"
        ],
        None,
        57776
    ),
    // Helsinki-NLP/opus-mt-uk-ru
    pair!(
        "uk",
        "ru",
        "uk-ru",
        "cfb76ccd207734d7847679664fa4538cda5ed597",
        [
            49_201_054,
            "6b599e2f4fbf1ea36cf983f1c0f40fcb2bb0ad8f6e0d1a7591da46b5ca9bf48a"
        ],
        [
            56_485_220,
            "2a99fed7dd89b22fee3752b8f6c7ac5e81d563635cf8198f49ca551f498daf76"
        ],
        [
            1_031_670,
            "1d8d0a245c07efca75da76f0c68e9d61bc9667af1ee81d230faf14869653b234"
        ],
        [
            3_474_975,
            "e0350396e721f2863a8182e93cad07ba19166ca1cc5ed7d08acfde89003ffea1"
        ],
        None,
        57776
    ),
    // Helsinki-NLP/opus-mt-nl-fr
    pair!(
        "nl",
        "fr",
        "nl-fr",
        "6c8ebb68522ffd80cf3474b2dc44f4b5f741e9cd",
        [
            49_131_934,
            "357de84fa0df0fe4b8e95adb9d0061118a2a9a4602863390e5e7c2ff505b1b7c"
        ],
        [
            56_415_560,
            "7915cb6d03674552b6f7a3bcd4fe04e4dedd2a16fc120b4bcab39a03c6e6ecb4"
        ],
        [
            814_095,
            "a2452ac5c28605107318ee5369f7a46b63692f2dcb584397a89fbb876a5e321d"
        ],
        [
            1_462_960,
            "fcdfbeacc62d75c9d235b0882e74409561bee894fdb0336e83a0a1d357c9fa25"
        ],
        None,
        57641
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
    crate::paths::mt()
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

    /// Bytes of everything this pair needs, present or not: what `ensure`'s
    /// progress counts to.
    pub fn full_size(&'static self) -> u64 {
        self.files().iter().map(|(_, f)| f.size).sum()
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
