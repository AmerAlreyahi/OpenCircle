//! Text (Copy text): read the text in the crop and copy it. OCR runs in a helper
//! process (`crate::ocr`); its models are downloaded once, only after the user
//! agrees (the download card; the app asks before it ever gets here).
use super::{Action, Crop, Key, Run};
use crate::ocr::{self, TextBlock};

pub const ACTION: Action = Action {
    id: "text",
    label: "Copy text",
    key: Key {
        ctrl: false,
        letter: 't',
    },
    icon: '\u{e197}', // lucide "type"
    run: Run::Finish(copy_read),
};

/// The text reader's models aren't here: Copy text never fetches them itself
/// (only the download card does, on the user's click).
pub const NEEDS_READER: &str = "Get the text reader first: press Copy text in the overlay.";

/// Reads `crop`'s text and copies it (the pill's text, or why not). Never
/// downloads: without the models it says so (the overlay shows the download
/// card before it gets here; the quick action and `run text` get this answer).
fn copy_read(crop: Crop) -> Result<String, String> {
    if !models_present() {
        return Err(NEEDS_READER.into());
    }
    let text = read(&crop, |_| {})?;
    if text.trim().is_empty() {
        return Err("No text found in the selection.".into());
    }
    super::copy::text(text).map(|()| super::copy::COPIED_PILL.to_string())
}

/// How reading is going.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    /// Model download, 0..=1.
    Download(f32),
    Reading,
}

/// The models for the configured source language are on disk, so the user
/// already agreed to their download (another source asks again).
// ponytail: "the files are there" stands for "the user agreed once". ensure_models
// still verifies every file and re-fetches only a damaged one.
pub fn models_present() -> bool {
    #[cfg(test)]
    if let Some(here) = MODELS_HERE.get() {
        return here;
    }
    ocr::ready()
}

#[cfg(test)]
thread_local! {
    /// Tests: whether `models_present` says the models are here (else the disk).
    pub static MODELS_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Blocking (download, helper process): call off the UI thread. Fetches any
/// missing model, then reads `crop`. `progress` gets whole-percent steps only.
pub fn read(crop: &Crop, mut progress: impl FnMut(Progress)) -> Result<String, String> {
    download(&mut progress)?;
    progress(Progress::Reading);
    let blocks =
        ocr::recognize_in_child(&crop.rgba, crop.width, crop.height).map_err(|e| e.to_string())?;
    Ok(join_lines(&blocks))
}

/// Blocking: fetches any missing OCR model (only after the user agreed),
/// reporting whole-percent `Progress::Download` steps.
pub fn download(mut progress: impl FnMut(Progress)) -> Result<(), String> {
    let mut shown = None;
    ocr::ensure_models(|done, total| {
        let percent = done * 100 / total.max(1);
        if shown != Some(percent) {
            shown = Some(percent);
            progress(Progress::Download(percent as f32 / 100.0));
        }
    })
    .map(drop)
    .map_err(|e| e.to_string())
}

/// OCR lines (already in reading order) as text: boxes on one row joined by
/// spaces, rows by newlines. A box continues the row when it lies right of the
/// previous box and its vertical centre is inside that box. A toolbar icon
/// read as a letter at either end of a row is left out (`icon_like`).
pub(crate) fn join_lines(blocks: &[TextBlock]) -> String {
    let mut rows: Vec<Vec<&TextBlock>> = Vec::new();
    let mut prev: Option<[f32; 4]> = None;
    for block in blocks {
        let [x, y, _, h] = block.bounds;
        let centre = y + h / 2.0;
        match (prev, rows.last_mut()) {
            (Some([px, py, _, ph]), Some(row)) if x > px && py <= centre && centre < py + ph => {
                row.push(block)
            }
            _ => rows.push(vec![block]),
        }
        prev = Some(block.bounds);
    }
    let gap = |a: &TextBlock, b: &TextBlock| b.bounds[0] - (a.bounds[0] + a.bounds[2]);
    for row in &mut rows {
        while row.len() > 1 && icon_like(row[0], gap(row[0], row[1]), &row[1].text) {
            row.remove(0);
        }
        while let [.., before, last] = row[..]
            && icon_like(last, gap(before, last), &before.text)
        {
            row.pop();
        }
    }
    let row = |r: &Vec<&TextBlock>| {
        let words: Vec<&str> = r.iter().map(|b| b.text.as_str()).collect();
        strip_before_url(&words.join(" "))
    };
    rows.iter().map(row).collect::<Vec<_>>().join("\n")
}

/// A box that is one glyph standing apart (a gap wider than its height) from
/// `next`, the rest of its row, and is no word: unsure, not a letter or
/// digit, a Latin letter that is no one-letter word, or next to a URL (an
/// address bar's reload and lock icons). "I", "a", an Arabic letter stay.
fn icon_like(b: &TextBlock, gap: f32, next: &str) -> bool {
    let mut chars = b.text.trim().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return false;
    };
    gap > b.bounds[3]
        && (b.confidence < 0.9
            || !c.is_alphanumeric()
            || (c.is_ascii_alphabetic() && !"aAI".contains(c))
            || is_url(next.split_whitespace().next().unwrap_or_default()))
}

fn is_url(word: &str) -> bool {
    word.contains("://") || word.starts_with("www.")
}

/// One box read with the icons in it ("C 8 https://..."): the lone glyphs
/// right before a URL that starts the row go.
fn strip_before_url(row: &str) -> String {
    let words: Vec<&str> = row.split(' ').collect();
    let lone = words.iter().take_while(|w| w.chars().count() == 1).count();
    match words.get(lone) {
        Some(w) if lone > 0 && is_url(w) => words[lone..].join(" "),
        _ => row.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str, x: f32, y: f32) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds: [x, y, 80.0, 20.0],
            confidence: 1.0,
        }
    }

    #[test]
    fn lines_join_in_reading_order() {
        // As OCR returns them: row by row, left to right within a row.
        let blocks = [
            block("Pastel de nata", 0.0, 5.0),
            block("1,40 €", 300.0, 2.0), // same row, a little higher
            block("Tosta mista", 0.0, 40.0),
            block("4,20 €", 300.0, 43.0),
            block("Note:", 0.0, 80.0),
            block("indented", 40.0, 120.0), // further right, but a row lower: a new row
        ];
        assert_eq!(
            join_lines(&blocks),
            "Pastel de nata 1,40 €\nTosta mista 4,20 €\nNote:\nindented"
        );
    }

    /// Gate 0.1.2 bug 3: an address bar's reload and lock icons read as "C" and
    /// "8" are left out; real one-letter words stay.
    #[test]
    fn icons_read_as_letters_are_left_out_and_one_letter_words_stay() {
        let at = |text: &str, x: f32, w: f32, confidence: f32| TextBlock {
            text: text.into(),
            bounds: [x, 50.0, w, 20.0],
            confidence,
        };
        let bar = [
            at("C", 10.0, 12.0, 0.95),
            at("8", 60.0, 12.0, 0.97),
            at(
                "https://en.wikipedia.org/wiki/Solar_System",
                106.0,
                266.0,
                0.99,
            ),
        ];
        assert_eq!(
            join_lines(&bar),
            "https://en.wikipedia.org/wiki/Solar_System"
        );
        // the same, read as one box
        let one = [at("C 8 https://en.wikipedia.org", 10.0, 300.0, 0.9)];
        assert_eq!(join_lines(&one), "https://en.wikipedia.org");
        // a one-letter word, even set apart, a glyph close to its word, an
        // Arabic letter: all kept
        let kept = [
            at("I", 0.0, 8.0, 0.99),
            at("think so", 40.0, 80.0, 0.99),
            at("و", 130.0, 8.0, 0.99),
        ];
        assert_eq!(join_lines(&kept), "I think so و");
        let close = [at("x", 0.0, 10.0, 0.5), at("2 = 4", 14.0, 50.0, 0.99)];
        assert_eq!(join_lines(&close), "x 2 = 4");
        assert_eq!(
            join_lines(&[at("Q", 0.0, 12.0, 0.5)]),
            "Q",
            "alone on its row"
        );
    }

    #[test]
    fn a_row_that_restarts_on_the_left_is_a_new_line() {
        // Overlapping heights (tight line spacing), but x goes back: a new row.
        let blocks = [block("one", 0.0, 0.0), block("two", 0.0, 8.0)];
        assert_eq!(join_lines(&blocks), "one\ntwo");
        assert_eq!(join_lines(&[]), "");
    }
}

#[cfg(test)]
mod copy_read_tests {
    /// Copy text without the models answers, and never fetches them (it used to
    /// download them unasked; in CI that put real models in the data folder).
    #[test]
    fn copy_text_never_downloads_the_models() {
        super::MODELS_HERE.set(Some(false));
        let crop = crate::actions::Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        assert_eq!(super::copy_read(crop), Err(super::NEEDS_READER.to_string()));
        super::MODELS_HERE.set(None);
    }
}
