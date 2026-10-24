//! A Marian model's tokenizer: the SentencePiece unigram model it was trained
//! with (`source.spm`) segments the text, `vocab.json` numbers the pieces.
//! What Hugging Face's `MarianTokenizer` does (whitespace split, `▁` before
//! each word, Viterbi over piece scores, one `<unk>` per run of unknown
//! characters), in pure Rust: the `tokenizers` crate added 3.6 MB to the
//! release binary, and its `tokenizer.json` for this model segments `,"` and
//! CJK differently.
// ponytail: no NFKC (source.spm's nmt_nfkc rule), so ligatures like "ﬁ"
// become <unk>; OCR never reads them. Add unicode-normalization if needed.
use std::collections::HashMap;

pub struct Unigram {
    /// piece -> (id, log probability); id = `unk` for a piece the vocabulary lacks.
    ids: HashMap<String, (u32, f32)>,
    /// id -> piece (`vocab.json`), for decoding.
    pieces: Vec<String>,
    unk: u32,
    /// Score of an unknown character: the lowest piece score minus 10, as
    /// SentencePiece.
    unk_score: f32,
    /// Longest piece, in bytes (bounds the Viterbi search).
    max_len: usize,
    /// `</s>`, `<unk>`, `<pad>`: never in decoded text.
    special: [u32; 3],
}

impl Unigram {
    /// `spm` = the SentencePiece model (protobuf), `vocab` = vocab.json.
    pub fn load(spm: &[u8], vocab: &[u8]) -> Result<Unigram, String> {
        let vocab: HashMap<String, u32> =
            serde_json::from_slice(vocab).map_err(|e| e.to_string())?;
        let id = |p: &str| {
            vocab
                .get(p)
                .copied()
                .ok_or(format!("{p} is not in vocab.json"))
        };
        let (unk, special) = (id("<unk>")?, [id("</s>")?, id("<unk>")?, id("<pad>")?]);
        let mut pieces = vec![String::new(); vocab.values().max().map_or(0, |m| *m as usize + 1)];
        for (p, &i) in &vocab {
            pieces[i as usize] = p.clone();
        }
        let mut ids = HashMap::new();
        for (piece, score, kind) in spm_pieces(spm)? {
            // NORMAL and USER_DEFINED pieces match text; UNKNOWN, CONTROL, ... don't
            if kind == 1 || kind == 4 {
                let id = vocab.get(&piece).copied().unwrap_or(unk);
                ids.entry(piece).or_insert((id, score));
            }
        }
        if ids.is_empty() {
            return Err("the SentencePiece model has no pieces".into());
        }
        let min = ids.values().map(|(_, s)| *s).fold(f32::INFINITY, f32::min);
        Ok(Unigram {
            max_len: ids.keys().map(String::len).max().unwrap_or(1),
            ids,
            pieces,
            unk,
            unk_score: min - 10.0,
            special,
        })
    }

    /// The vocabulary id of `piece` (also a language token like `>>ara<<`).
    pub fn id(&self, piece: &str) -> Option<u32> {
        self.pieces
            .iter()
            .position(|p| p == piece)
            .map(|i| i as u32)
    }

    /// The ids of `text`'s pieces (no `</s>`).
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for word in text.split_whitespace() {
            self.viterbi(&format!("▁{word}"), &mut out);
        }
        out
    }

    /// The best-scoring split of `s` into pieces.
    fn viterbi(&self, s: &str, out: &mut Vec<u32>) {
        let ends: Vec<usize> = (s.char_indices().map(|(i, _)| i).skip(1))
            .chain([s.len()])
            .collect();
        // best[i] = (score, start of the last piece, its id) for s[..i]
        let mut best: Vec<Option<(f32, usize, u32)>> = vec![None; s.len() + 1];
        best[0] = Some((0.0, 0, 0));
        let mut start = 0;
        for (k, &next) in ends.iter().enumerate() {
            if let Some((score, ..)) = best[start] {
                let mut relax = |end: usize, total: f32, id: u32| {
                    if best[end].is_none_or(|(b, ..)| total > b) {
                        best[end] = Some((total, start, id));
                    }
                };
                let mut known = false;
                for &end in ends[k..].iter().take_while(|&&e| e - start <= self.max_len) {
                    if let Some(&(id, piece)) = self.ids.get(&s[start..end]) {
                        known |= end == next;
                        relax(end, score + piece, id);
                    }
                }
                if !known {
                    relax(next, score + self.unk_score, self.unk);
                }
            }
            start = next;
        }
        let mut ids = Vec::new();
        let mut end = s.len();
        while let Some((_, from, id)) = best[end].filter(|_| end > 0) {
            // a run of unknown characters is one <unk>
            if !(id == self.unk && ids.last() == Some(&self.unk)) {
                ids.push(id);
            }
            end = from;
        }
        out.extend(ids.into_iter().rev());
    }

    /// Text of `ids`, special tokens left out; `▁` is a space.
    pub fn decode(&self, ids: &[u32]) -> String {
        let text: String = (ids.iter())
            .filter(|id| !self.special.contains(id))
            .filter_map(|&id| self.pieces.get(id as usize))
            .map(String::as_str)
            .collect();
        text.replace('▁', " ").trim_start().to_string()
    }
}

/// (piece, score, type) of a SentencePiece `ModelProto`: field 1 repeats
/// `SentencePiece { 1: piece, 2: score (float), 3: type (default 1, NORMAL) }`.
fn spm_pieces(mut b: &[u8]) -> Result<Vec<(String, f32, u64)>, String> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let (field, value) = next_field(&mut b)?;
        if let (1, Field::Bytes(mut p)) = (field, value) {
            let (mut piece, mut score, mut kind) = (String::new(), 0.0, 1);
            while !p.is_empty() {
                match next_field(&mut p)? {
                    (1, Field::Bytes(s)) => piece = String::from_utf8_lossy(s).into_owned(),
                    (2, Field::Fixed32(f)) => score = f32::from_bits(f),
                    (3, Field::Varint(t)) => kind = t,
                    _ => {}
                }
            }
            out.push((piece, score, kind));
        }
    }
    Ok(out)
}

enum Field<'a> {
    Varint(u64),
    Fixed32(u32),
    Bytes(&'a [u8]),
    Fixed64,
}

/// One protobuf field: (number, value); advances `b`.
fn next_field<'a>(b: &mut &'a [u8]) -> Result<(u64, Field<'a>), String> {
    let key = varint(b)?;
    let value = match key & 7 {
        0 => Field::Varint(varint(b)?),
        1 => {
            take(b, 8)?;
            Field::Fixed64
        }
        2 => {
            let n = usize::try_from(varint(b)?).map_err(|e| e.to_string())?;
            Field::Bytes(take(b, n)?)
        }
        5 => Field::Fixed32(u32::from_le_bytes(
            take(b, 4)?.try_into().unwrap_or_default(),
        )),
        t => return Err(format!("unknown protobuf wire type {t}")),
    };
    Ok((key >> 3, value))
}

/// The next `n` bytes of `b`; advances it.
fn take<'a>(b: &mut &'a [u8], n: usize) -> Result<&'a [u8], String> {
    if b.len() < n {
        return Err("the SentencePiece model is cut short".into());
    }
    let (head, rest) = b.split_at(n);
    *b = rest;
    Ok(head)
}

fn varint(b: &mut &[u8]) -> Result<u64, String> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = b
            .split_first()
            .ok_or("the SentencePiece model is cut short")?;
        *b = rest;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("bad varint in the SentencePiece model".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SentencePiece model with `pieces` (piece, score, type).
    fn spm(pieces: &[(&str, f32, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (p, score, kind) in pieces {
            let mut m = vec![0x0a, p.len() as u8];
            m.extend(p.as_bytes());
            m.push(0x15); // field 2, fixed32
            m.extend(score.to_bits().to_le_bytes());
            m.extend([0x18, *kind as u8]); // field 3, varint
            out.extend([0x0a, m.len() as u8]);
            out.extend(m);
        }
        out.extend([0x12, 2, 0x08, 0x01]); // a trainer_spec to skip
        out
    }

    fn tiny() -> Unigram {
        let model = spm(&[
            ("<unk>", 0.0, 2),
            ("</s>", 0.0, 3),
            ("▁Quit", -1.0, 1),
            ("▁the", -1.0, 1),
            ("▁g", -2.0, 1),
            ("ame", -2.0, 1),
            ("▁", -3.0, 1),
            ("g", -4.0, 1),
            ("▁game", -9.0, 1),
            ("é", -4.0, 1),
            ("ñ", -4.0, 1), // segments, but vocab.json lacks it: <unk>
        ]);
        let vocab = r#"{"</s>":0,"<unk>":1,"▁Quit":2,"▁the":3,"▁g":4,"ame":5,"▁":6,"g":7,"▁game":8,"é":9,"<pad>":10,">>ara<<":11}"#;
        Unigram::load(&model, vocab.as_bytes()).unwrap()
    }

    #[test]
    fn tokenizer_round_trip() {
        let tok = tiny();
        // "▁g"+"ame" (-4) beats "▁game" (-9) and "▁"+"g"+... (unknown a, m, e)
        assert_eq!(tok.encode("Quit  the game"), [2, 3, 4, 5]);
        assert_eq!(tok.decode(&[2, 3, 4, 5, 0]), "Quit the game");
        // unknown characters: one <unk> per run, multi-byte ones too
        assert_eq!(tok.encode("gxyé"), [4, 1, 9]);
        assert_eq!(tok.encode("ق"), [6, 1]);
        assert_eq!(tok.encode("ñ"), [6, 1], "a piece the vocabulary lacks");
        assert_eq!(
            tok.decode(&[10, 6, 7, 1, 9]),
            "gé",
            "special tokens dropped"
        );
        assert_eq!(tok.encode(" \t "), Vec::<u32>::new());
        assert_eq!(
            (tok.id(">>ara<<"), tok.id("</s>"), tok.id("nope")),
            (Some(11), Some(0), None)
        );
    }

    #[test]
    fn bad_models_are_errors() {
        let vocab = br#"{"</s>":0,"<unk>":1,"<pad>":2}"#;
        assert!(Unigram::load(&spm(&[("a", -1.0, 1)]), b"[]").is_err());
        assert!(Unigram::load(&[], vocab).is_err(), "no pieces");
        assert!(Unigram::load(&[0x0a, 9, 0x0a], vocab).is_err(), "cut short");
        assert!(Unigram::load(&[0x0b], vocab).is_err(), "wire type 3");
        assert!(Unigram::load(&spm(&[("a", -1.0, 1)]), br#"{"a":0}"#).is_err());
    }
}
