//! Text selection on the frozen screen: taps, drags, reading the screen's words.

use std::sync::Arc;
use std::time::Instant;

use iced::widget::image;
use iced::{Point, Rectangle, Task, Vector};

use crate::actions::{self, Crop, copy, text};
use crate::ocr::screen::Wants;
use crate::ocr::{self, TextBlock};

use crate::ui::app::background::background;
use crate::ui::app::message::{Await, Message, PickAct, Purpose, WordRead};
use crate::ui::app::word;
use crate::ui::overlay;

use super::{App, DOUBLE_CLICK, Reader, TextMode, WARM_AFTER};

impl App {
    /// A tap at `at` (image px): the word there becomes the selection. Lines read
    /// for an earlier tap are reused (a tap on the same line makes a range);
    /// otherwise OCR reads around the tap on a thread. Without the OCR models a
    /// tap does nothing (it never asks to download them).
    pub(in crate::ui) fn tap(&mut self, at: Point) -> Task<Message> {
        if self.o.selection.is_none() {
            self.o.stroke.clear(); // the tap's own dot
        }
        if self.reads_text() {
            if self.o.screen_text.knows(at.y) {
                return Task::none(); // read: no text here
            }
            // the word there once its band is read (a triple tap: the line)
            let unit = match self.o.clicks {
                Some((_, _, n)) if n >= 3 => word::Unit::Line,
                _ => word::Unit::Word,
            };
            self.o.awaits = Some((at, Await::Tap(unit)));
            return self.read_more();
        }
        // A click on a selection keeps it unless a word is found there.
        self.read_words(word::region(at, self.o.screen), at, false)
    }

    /// A press at `p` (image px) on the screen's text read there (main mode
    /// only): the caret there, or its word or line (`unit`).
    pub(in crate::ui) fn text_at(&self, p: Point, unit: word::Unit) -> Option<word::Picked> {
        let main = matches!(self.o.purpose, Purpose::Search) && self.o.translate.is_none();
        let all = Rectangle::with_size(self.o.screen);
        let read = main && self.o.screen_text.knows(p.y);
        read.then(|| word::Picked::new(self.o.screen_text.lines(), all, p, unit))?
    }

    /// Counts a press at `p` (image px) as the first, second or third click in
    /// a row (each within `DOUBLE_CLICK` of the last and near it): what it
    /// selects on text.
    pub(in crate::ui) fn click(&mut self, p: Point) -> word::Unit {
        let now = Instant::now();
        let n = match self.o.clicks {
            Some((at, last, n)) if now - at <= DOUBLE_CLICK && last.distance(p) <= 4.0 => n + 1,
            _ => 1,
        };
        self.o.clicks = Some((now, p, n));
        match n {
            1 => word::Unit::Caret,
            2 => word::Unit::Word,
            _ => word::Unit::Line,
        }
    }

    /// A press that began on text moved to `p` (image px): the words stretch
    /// there, unless the stroke loops, which makes it a circle after all.
    /// Into rows not read yet: they are read first (`read_more`).
    pub(in crate::ui) fn drag_text(&mut self, p: Point) -> Task<Message> {
        let zoom = self.o.view.zoom();
        let Some(stroke) = &mut self.o.text_press else {
            return Task::none();
        };
        stroke.push(p);
        if overlay::winds(stroke, zoom) {
            self.o.stroke = self.o.text_press.take().unwrap_or_default();
            self.o.drawing = true;
            self.o.word = None;
            self.o.selection = None;
            return Task::none();
        }
        if let Some(word) = &mut self.o.word
            && word.extend(p)
        {
            self.select_word();
        }
        self.read_more()
    }

    /// The screen's text has just been read while a press drew: one that
    /// began on text becomes a text selection to where the pointer is now
    /// (a loop stays a circle, `drag_text`).
    pub(in crate::ui) fn drag_became_text(&mut self) {
        let Some(&start) = self
            .o
            .stroke
            .first()
            .filter(|_| self.o.drawing && !self.o.adding)
        else {
            return;
        };
        let Some(word) = self.text_at(start, word::Unit::Caret) else {
            return;
        };
        let stroke = std::mem::take(&mut self.o.stroke);
        self.o.drawing = false;
        self.o.word = Some(word);
        self.o.text_used = true;
        self.select_word();
        self.o.text_press = Some(vec![start]);
        for &p in &stroke[1..] {
            match self.o.text_press {
                Some(_) => _ = self.drag_text(p), // (the caller reads on)
                None => self.o.stroke.push(p),    // it looped: a circle after all
            }
        }
    }

    /// Main mode without the OCR models: the note over the bar says so and
    /// offers them (`Reader::Missing`). Nothing is read at open (#123).
    pub(in crate::ui) fn note_reader(&mut self) {
        if matches!(self.o.purpose, Purpose::Search) && !self.o.ocr_ready && !self.system_text() {
            self.o.reader = Reader::Missing(ocr::download_size());
        }
    }

    /// Main mode's text can be read: Windows OCR or the OCR models here, no
    /// translation shown.
    pub(in crate::ui) fn reads_text(&self) -> bool {
        (self.o.ocr_ready || self.system_text())
            && self.o.frame.is_some()
            && matches!(self.o.purpose, Purpose::Search)
            && self.o.translate.is_none()
    }

    /// Windows OCR reads this opening's text (#62): decided at its first read,
    /// until then what `ocr::route` says now.
    pub(in crate::ui) fn system_text(&self) -> bool {
        let route = || ocr::route(self.o.behaviour.text_reader, ocr::system::known());
        self.o.text_by.unwrap_or_else(route) == ocr::Source::System
    }

    /// After the opening animation: Windows OCR reads the whole screen (#62)
    /// if it didn't start at the capture, or the held helper starts and loads its models (gently), so the first
    /// text read is quick. Nothing during it.
    pub(in crate::ui) fn warm(&mut self) -> Task<Message> {
        if !self.reads_text() {
            return Task::none();
        }
        if self.system_text() {
            return self.read_more();
        }
        let held = self
            .o
            .helper
            .as_ref()
            .map(Arc::downgrade)
            .unwrap_or_default();
        let opening = self.opening;
        background(move |tx| {
            // after the bar has settled too; closed meanwhile: nothing starts
            std::thread::sleep(WARM_AFTER);
            if let Some(_held) = held.upgrade()
                && let Err(e) = ocr::warm()
            {
                crate::log!("[opencircle] the text reader didn't start: {e}");
            }
            let _ = tx.unbounded_send(Message::Warmed(opening));
        })
    }

    /// Main mode: reads the next band of the screen's text, if none is being
    /// read: where the user touches text (a press, a drag going on into
    /// unread rows, a tap, a Shift+click) first; then, once text was used
    /// (or Ctrl+A waits), the rest, nearest the pointer first.
    pub(in crate::ui) fn read_more(&mut self) -> Task<Message> {
        if self.o.reading.is_some() || !self.reads_text() {
            return Task::none();
        }
        let pressed = (self.o.drawing && !self.o.adding)
            .then(|| self.o.stroke.first().copied())
            .flatten();
        let dragged = self.o.text_press.as_ref().and_then(|s| s.last().copied());
        let touches: Vec<f32> = [dragged, pressed, self.o.awaits.map(|(p, _)| p)]
            .into_iter()
            .flatten()
            .map(|p| p.y)
            .collect();
        let source = match self.system_text() {
            true => ocr::Source::System,
            false => ocr::Source::Helper,
        };
        let wants = Wants {
            source,
            started: self.o.text_by.is_some(),
            touches: &touches,
            waits: self.o.want_all,
            text_used: self.o.text_used,
            pointer: self.o.pointer.y,
        };
        let next = self.o.screen_text.schedule(self.o.screen, &wants);
        if source == ocr::Source::Helper || next.is_some() {
            self.o.text_by = Some(source);
        }
        let Some((band, gently)) = next else {
            return Task::none();
        };
        // Windows OCR: the whole screen at once, the first time (from the
        // capture, `open`; else after the opening animation, `warm`)
        let band = band.unwrap_or(Rectangle::with_size(self.o.screen));
        self.read_band(band, gently)
    }

    /// OCR reads `band` (image px) of the frame on a thread: at once where the
    /// user waits for it, else gently (the helper on at most two cores, below
    /// normal priority).
    pub(in crate::ui) fn read_band(&mut self, band: Rectangle, gently: bool) -> Task<Message> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return Task::none();
        };
        let (width, height, pixels) = (*width, *height, pixels.clone());
        let Some([x0, y0, ..]) = actions::crop_bounds(width, height, self.o.scale, band) else {
            return Task::none();
        };
        // only the lines it is read for (frame px in the crop)
        let Some((top, bottom)) = self.o.screen_text.unread(band, self.o.screen) else {
            return Task::none();
        };
        let at = |y: f32| ((y * self.o.scale) as u32).saturating_sub(y0);
        let rows = (at(top), at(bottom));
        self.o.reading = Some(band);
        let (scale, opening) = (self.o.scale, self.opening);
        let held = self
            .o
            .helper
            .as_ref()
            .map(Arc::downgrade)
            .unwrap_or_default();
        let system =
            (self.o.text_by == Some(ocr::Source::System)).then_some(self.o.behaviour.text_reader);
        background(move |tx| {
            // closed meanwhile: nothing to read for (and no helper started)
            let Some(_held) = held.upgrade() else {
                return;
            };
            let send = |read: Result<Vec<TextBlock>, ocr::OcrError>| {
                let lines = read.map(|blocks| {
                    let mut lines = word::lines(blocks, [x0, y0], scale);
                    word::measure(&mut lines, &pixels, width, height, scale);
                    lines
                });
                let _ = tx.unbounded_send(lines.map_err(|e| e.to_string()));
            };
            let Some(c) = actions::crop(&pixels, width, height, scale, band) else {
                return send(Err(ocr::OcrError::BadImage("an empty band".into())));
            };
            let Some(setting) = system else {
                return send(ocr::recognize_band(
                    &c.rgba, c.width, c.height, rows, gently,
                ));
            };
            // its language known now: a right-to-left one goes to the helper
            if ocr::route(setting, Some(ocr::system::language())) == ocr::Source::Helper {
                let rtl = "Windows OCR: a right-to-left language";
                return send(Err(ocr::OcrError::Engine(rtl.into())));
            }
            // fast, then (small text) doubled: each read replaces the last
            for (i, &k) in ocr::system::passes(scale, c.width, c.height)
                .iter()
                .enumerate()
            {
                match ocr::system::recognize(&c.rgba, c.width, c.height, k) {
                    Err(e) if i > 0 => crate::log!("[opencircle] Windows OCR's second read: {e}"),
                    read => send(read),
                }
            }
        })
        .map(move |read| Message::ScreenText(opening, band, read))
    }

    /// The held release (and the events after it) go on, now that the band
    /// under the press is read (or failed): a text selection or a stroke.
    pub(in crate::ui) fn release_held(&mut self) -> Task<Message> {
        let Some(events) = self.o.held.take() else {
            return Task::none();
        };
        let mut tasks = vec![self.on_overlay(overlay::Event::Release)];
        tasks.extend(events.into_iter().map(|e| self.on_overlay(e)));
        Task::batch(tasks)
    }

    /// More of the screen's text was read: a text selection takes in the new
    /// lines (a drag can go on into them), a press that began on text becomes
    /// a text selection, and a tap, Shift+click or Ctrl+A waiting for it is
    /// done.
    pub(in crate::ui) fn more_text(&mut self) -> Task<Message> {
        let all = Rectangle::with_size(self.o.screen);
        let o = &mut *self.o;
        if let Some(word) = &mut o.word {
            word.reread(o.screen_text.lines(), all);
            if let Some(&p) = o.text_press.as_ref().and_then(|s| s.last()) {
                word.extend(p);
            }
            self.select_word();
        }
        self.drag_became_text();
        let mut done = Task::none();
        if let Some((at, wait)) = self.o.awaits
            && self.o.screen_text.knows(at.y)
        {
            self.o.awaits = None;
            match wait {
                Await::Tap(unit) => {
                    if let Some(word) = self.text_at(at, unit) {
                        self.o.word = Some(word);
                        self.o.text_used = true;
                        self.select_word();
                        done = self.selected();
                    }
                }
                Await::Extend => {
                    if let Some(word) = &mut self.o.word
                        && word.extend(at)
                    {
                        self.select_word();
                        done = self.selected();
                    }
                }
            }
        }
        if self.o.want_all && self.o.screen_text.is_whole(self.o.screen.height) {
            self.o.want_all = false;
            self.o.word = word::Picked::all(self.o.screen_text.lines(), all);
            self.select_word();
            done = self.selected();
        }
        done
    }

    /// The whole frozen frame (Ask before a selection).
    pub(in crate::ui) fn whole(&self) -> Option<Crop> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return None;
        };
        let all = Rectangle::with_size(self.o.screen);
        actions::crop(pixels, *width, *height, self.o.scale, all)
    }

    /// The picked words stretch to `at` (image px), where a drag ended or a Shift
    /// click landed. Past the lines already read, OCR reads a band across the
    /// screen first (on a thread, through the warm helper).
    pub(in crate::ui) fn extend_words(&mut self, at: Point) -> Task<Message> {
        if self.o.word.is_some() && self.reads_text() && !self.o.screen_text.knows(at.y) {
            self.o.awaits = Some((at, Await::Extend));
            return self.read_more();
        }
        let screen = self.o.screen;
        let Some(word) = &mut self.o.word else {
            return Task::none();
        };
        if !word.covers(at) {
            let band = word.band(at, screen);
            return self.read_words(band, at, true);
        }
        if word.extend(at) {
            self.select_word();
        }
        self.selected()
    }

    /// Reads the text in `region` (image px) on a thread, for the tap or drag
    /// end at `at`; the answer comes back as `Tapped`. Without the OCR models
    /// nothing happens (it never asks to download them).
    pub(in crate::ui) fn read_words(
        &mut self,
        region: Rectangle,
        at: Point,
        extend: bool,
    ) -> Task<Message> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return Task::none();
        };
        if !text::models_present() {
            return Task::none();
        }
        let (Some([x0, y0, ..]), Some(crop)) = (
            actions::crop_bounds(*width, *height, self.o.scale, region),
            actions::crop(pixels, *width, *height, self.o.scale, region),
        ) else {
            return Task::none();
        };
        self.taps = self.taps.wrapping_add(1);
        let what = WordRead {
            id: self.taps,
            at,
            origin: [x0, y0],
            region,
            extend,
        };
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let _ = tx.unbounded_send(read.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::Tapped(what, read))
    }

    /// OCR for tap or drag `what` is back: the word under the tap is the
    /// selection, or the picked words stretch to where the drag ended.
    pub(in crate::ui) fn on_tapped(
        &mut self,
        what: WordRead,
        read: Result<Vec<TextBlock>, String>,
    ) -> Task<Message> {
        if what.id != self.taps || self.o.frame.is_none() {
            return Task::none(); // pressed again meanwhile, or closed
        }
        let blocks = match read {
            Ok(blocks) => blocks,
            Err(e) => {
                crate::log!("[opencircle] reading the tapped word failed: {e}");
                return Task::none();
            }
        };
        let mut lines = word::lines(blocks, what.origin, self.o.scale);
        if let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        {
            word::measure(&mut lines, pixels, *width, *height, self.o.scale);
        }
        if what.extend {
            let Some(word) = &mut self.o.word else {
                return Task::none();
            };
            word.reread(lines, what.region);
            word.extend(what.at);
        } else {
            let Some(picked) = word::Picked::new(lines, what.region, what.at, word::Unit::Word)
            else {
                return Task::none(); // no text there
            };
            self.o.word = Some(picked);
        }
        self.select_word();
        self.selected()
    }

    /// The selected text as the (only) selection: a flat highlight per line
    /// (the stroke holds a top-left / bottom-right pair for each). Nothing
    /// selected yet (a caret): no selection.
    pub(in crate::ui) fn select_word(&mut self) {
        let o = &mut *self.o;
        o.others.clear();
        o.selection = None;
        o.stroke.clear();
        if let Some(word) = o.word.as_ref().filter(|w| !w.is_empty()) {
            o.selection = Some(overlay::Selection {
                area: word.area(),
                kind: overlay::Kind::Word,
            });
            o.stroke = (word.rects().iter())
                .flat_map(|r| [r.position(), r.position() + Vector::from(r.size())])
                .collect();
        }
    }

    /// Words picked in a snapped box (select-text mode), not still being dragged.
    pub(in crate::ui) fn picked_in_box(&self) -> bool {
        (self.o.text_mode.as_ref()).is_some_and(|m| m.picked.is_some() && !m.picking)
    }

    /// The "Select text" chip: OCR reads the box (the warm helper) and every
    /// word in it becomes pickable. Without the OCR models the download card
    /// asks before downloading them.
    pub(in crate::ui) fn select_text(&mut self) -> Task<Message> {
        if self.o.text_mode.take().is_some() {
            return Task::none(); // "Done"
        }
        if !text::models_present() {
            self.o.reader = Reader::Missing(ocr::download_size());
            return Task::none();
        }
        let area = self.area();
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return Task::none();
        };
        let (Some([x0, y0, ..]), Some(crop)) = (
            actions::crop_bounds(*width, *height, self.o.scale, area),
            actions::crop(pixels, *width, *height, self.o.scale, area),
        ) else {
            return Task::none();
        };
        let (scale, frame) = (self.o.scale, (*width, *height, pixels.clone()));
        self.o.panel = None;
        self.o.text_mode = Some(TextMode {
            lines: None,
            boxes: Vec::new(),
            picked: None,
            picking: false,
        });
        background(move |tx| {
            let read = ocr::recognize_in_child(&crop.rgba, crop.width, crop.height);
            let lines = read.map(|blocks| {
                let mut lines = word::lines(blocks, [x0, y0], scale);
                word::measure(&mut lines, &frame.2, frame.0, frame.1, scale);
                lines
            });
            let _ = tx.unbounded_send(lines.map_err(|e| e.to_string()));
        })
        .map(move |read| Message::TextLines(area, read))
    }

    /// OCR read the box `area` for select-text mode (dropped if the selection
    /// moved on or the mode was left).
    pub(in crate::ui) fn on_text_lines(
        &mut self,
        area: Rectangle,
        read: Result<Vec<word::Line>, String>,
    ) {
        let same = self.area() == area;
        let Some(mode) = self.o.text_mode.as_mut().filter(|_| same) else {
            return;
        };
        match read {
            Ok(lines) => {
                mode.boxes = word::boxes(&lines);
                mode.lines = Some(lines);
            }
            Err(e) => {
                crate::log!("[opencircle] select text failed: {e}");
                self.o.text_mode = None;
            }
        }
    }

    /// A press at `p` (image px) picks words: select-text mode is on, read, and
    /// `p` is in the box.
    pub(in crate::ui) fn picks_text_at(&self, p: Point) -> bool {
        self.o.text_mode.as_ref().is_some_and(|m| m.lines.is_some())
            && self.area().contains(p)
            && self.grab_at(p).is_none() // (a bracket or edge adjusts the box)
    }

    /// The select-text popup: Copy closes with the words copied; Translate and
    /// Search make the words' box the selection and translate it in place / search it.
    pub(in crate::ui) fn on_picked(&mut self, act: PickAct) -> Task<Message> {
        let picked = (self.o.text_mode.as_ref())
            .and_then(|m| m.picked.clone())
            .or_else(|| self.o.word.clone());
        let Some(picked) = picked else {
            return Task::none();
        };
        if act == PickAct::Copy {
            return self.copy_text(picked.text());
        }
        if act == PickAct::Search {
            return self.search_text(picked.text());
        }
        // Words picked in a snapped box: their box becomes the selection (a
        // word selection already is it).
        if self.o.text_mode.take().is_some() {
            self.o.morph = None;
            (self.o.selection, self.o.stroke) = {
                let (sel, stroke) = overlay::snap(picked.area());
                (Some(sel), stroke)
            };
            self.remember_region();
        }
        self.start_translate()
    }

    /// Closes the overlay with `text` copied (the "Copied" pill).
    pub(in crate::ui) fn copy_text(&mut self, text: String) -> Task<Message> {
        self.confirm_after(move || {
            (copy::text(text))
                .map(|()| copy::COPIED_PILL.to_string())
                .map_err(|e| {
                    crate::log!("copy text failed: {e}");
                    "Couldn't copy. Close what holds the clipboard and try again.".to_string()
                })
        })
    }

    /// Select-text mode's box, read: all its text in reading order (`None`
    /// before it is read, or with no text in it).
    pub(in crate::ui) fn box_text(&self) -> Option<String> {
        let lines = self.o.text_mode.as_ref()?.lines.as_ref()?;
        let text: Vec<&str> = (lines.iter().map(|l| l.text.trim()))
            .filter(|t| !t.is_empty())
            .collect();
        (!text.is_empty()).then(|| text.join("\n"))
    }

    /// Picked words (a word selection, or select-text mode's picks): their text.
    pub(in crate::ui) fn picked_text(&self) -> Option<String> {
        let picked = (self.o.text_mode.as_ref()).and_then(|m| m.picked.as_ref());
        picked.or(self.o.word.as_ref()).map(word::Picked::text)
    }
}
