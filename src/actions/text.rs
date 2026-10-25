//! Text: read the text in the crop. OCR runs in a helper process (`crate::ocr`);
//! its models are downloaded once, only after the user agrees in the panel.
use super::{Action, Crop, Key, Panel, Run};
use crate::ocr::{self, TextBlock};

pub const ACTION: Action = Action {
    id: "text",
    label: "Text",
    key: Key {
        ctrl: false,
        letter: 't',
    },
    icon: '\u{e197}', // lucide "type"
    run: Run::Panel(Panel::Text),
};

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
    ocr::ready()
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
/// previous box and its vertical centre is inside that box.
pub(crate) fn join_lines(blocks: &[TextBlock]) -> String {
    let mut out = String::new();
    let mut prev: Option<[f32; 4]> = None;
    for block in blocks {
        let [x, y, _, h] = block.bounds;
        let centre = y + h / 2.0;
        match prev {
            None => {}
            Some([px, py, _, ph]) if x > px && py <= centre && centre < py + ph => out.push(' '),
            Some(_) => out.push('\n'),
        }
        out.push_str(&block.text);
        prev = Some(block.bounds);
    }
    out
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

    #[test]
    fn a_row_that_restarts_on_the_left_is_a_new_line() {
        // Overlapping heights (tight line spacing), but x goes back: a new row.
        let blocks = [block("one", 0.0, 0.0), block("two", 0.0, 8.0)];
        assert_eq!(join_lines(&blocks), "one\ntwo");
        assert_eq!(join_lines(&[]), "");
    }
}
