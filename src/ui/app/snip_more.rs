//! Snip toolbar's More: Copy text, Ask AI, Translate on the picked area
//! (`actions::snip_more` holds the decisions, `SNIP_MORE_SPEC.md` the design).
//! The work is main mode's: `actions::text` reads, `open_chat` asks (the
//! annotated picture), `translate::Session` translates; Copy text and
//! Translate see the clean capture with Blur / Redact / Spotlight-dimmed parts
//! painted out first.
use iced::widget::image;
use iced::{Rectangle, Task};

use crate::actions::snip_more::{self as more, Hidden, Read, Row};
use crate::actions::{Crop, text};
use crate::config;
use crate::ocr::{self, TextBlock};
use crate::snip::annotate::{self, Message as E};
use crate::ui::background;
use crate::ui::screens::snip_toolbar::SnipToolbar;
use crate::ui::screens::translate;

use super::message::{Message, Purpose};
use super::{App, Reader};

/// What More keeps for the picked area.
#[derive(Default)]
pub(in crate::ui) struct More {
    /// The area and hidden parts the last read was started for.
    key: Option<(Rectangle, Hidden)>,
    read: Read,
    /// The lines read, hidden parts already left out.
    blocks: Vec<TextBlock>,
    /// Which read answers (a newer area drops an older one's answer).
    reads: u32,
    /// A click that waits for the read (or for the models).
    then: Option<Row>,
    /// The picture as it was before the translation (Show original).
    original: Option<Vec<u8>>,
    /// The translation is pasted into the picture.
    shown: bool,
    /// The editor's `own_image` before the translation.
    was_own: bool,
}

impl App {
    /// The Snip toolbar is up on a picked area.
    pub(in crate::ui) fn more_editing(&self) -> bool {
        matches!(self.o.purpose, Purpose::Snip(_))
            && self.o.edit.as_ref().is_some_and(SnipToolbar::is_open)
    }

    /// The row a quick action's shortcut runs while Snip has an area.
    pub(in crate::ui) fn more_row_of(&self, qa: &config::QuickAction) -> Option<Row> {
        self.more_editing().then(|| more::row_of(qa)).flatten()
    }

    /// What the menu shows: the read, the user's own keys, where Ask AI goes.
    pub(in crate::ui) fn sync_more(&mut self) {
        let m = &self.o.more;
        let info = more::Info {
            read: m.read,
            caps: more::caps_from(&config::current()),
            browser: super::ai_chat::hands_off(),
            translated: m.shown,
        };
        if let Some(edit) = &mut self.o.edit {
            edit.annotator.more = info;
        }
    }

    /// More opened: its text rows' keys, and the area is read (once).
    pub(in crate::ui) fn more_opened(&mut self) -> Task<Message> {
        self.sync_more();
        self.read_area()
    }

    /// The area as it is read now: its place and its hidden parts (the clean
    /// capture's px).
    fn read_key(&self) -> Option<(Rectangle, Hidden)> {
        let doc = &self.o.edit.as_ref()?.annotator.doc;
        Some((doc.view(), annotate::hidden_in_clean(doc)))
    }

    /// Reads the area's text once per area / edit (a new key), never before
    /// More opens or a row is asked.
    fn read_area(&mut self) -> Task<Message> {
        let Some(key) = self.read_key() else {
            return Task::none();
        };
        if self.o.more.key.as_ref() == Some(&key) {
            return Task::none();
        }
        self.o.more.key = Some(key.clone());
        self.o.more.blocks.clear();
        let source = self.text_source();
        if source == ocr::Source::Helper && !text::models_present() {
            // the card asks on the click (Copy text); Translate asks its own
            self.o.more.read = Read::NoModels;
            self.sync_more();
            return Task::none();
        }
        let Some(edit) = &self.o.edit else {
            return Task::none();
        };
        let mut crop = annotate::clean(&edit.annotator.doc);
        more::mask(&mut crop, &key.1);
        self.o.more.read = Read::Reading;
        self.o.more.reads = self.o.more.reads.wrapping_add(1);
        self.sync_more();
        let (opening, id) = (self.opening, self.o.more.reads);
        let (setting, scale) = (self.o.behaviour.text_reader, self.o.scale);
        background(move |tx| {
            let _ = tx.unbounded_send(read_crop(&crop, setting, scale));
        })
        .map(move |read| Message::MoreRead(opening, id, read))
    }

    /// Who reads here: main mode's decision (`ocr::route`).
    fn text_source(&self) -> ocr::Source {
        ocr::route(self.o.behaviour.text_reader, Some(ocr::system::language()))
    }

    pub(in crate::ui) fn on_more_read(
        &mut self,
        opening: u32,
        id: u32,
        read: Result<Vec<TextBlock>, String>,
    ) -> Task<Message> {
        if opening != self.opening || id != self.o.more.reads || !self.more_editing() {
            return Task::none();
        }
        let hidden = (self.o.more.key.as_ref()).map_or_else(Hidden::default, |(_, h)| h.clone());
        match read {
            Ok(blocks) => {
                // a second guard after the paint-out: no line under a hidden part
                let kept = more::visible(blocks, &hidden);
                let any = !more::text_of(&kept).trim().is_empty();
                self.o.more.read = if any { Read::Text } else { Read::NoText };
                self.o.more.blocks = kept;
            }
            Err(e) => {
                crate::log!("[opencircle] More: the text was not read: {e}");
                // read again at the next ask
                (self.o.more.key, self.o.more.read) = (None, Read::Idle);
                self.o.more.then = None;
            }
        }
        self.sync_more();
        match self.o.more.then.take() {
            Some(row) => self.run_more(row),
            None => Task::none(),
        }
    }

    /// A row run by its shortcut (the toolbar's own path: a long capture
    /// running finishes first, the area is read if More never opened).
    pub(in crate::ui) fn run_more(&mut self, row: Row) -> Task<Message> {
        self.on_snip_toolbar(match row {
            Row::CopyText => E::CopyText,
            Row::AskAi => E::AskAi,
            Row::Translate => E::Translate,
        })
    }

    /// The text must be read before `row` can act: starts or awaits the read
    /// (the click is kept and runs when it is done). `None`: the read is here.
    fn after_read(&mut self, row: Row) -> Option<Task<Message>> {
        let key = self.read_key()?;
        if self.o.more.key.as_ref() != Some(&key) {
            let started = self.read_area();
            // no reader yet (its models are missing): nothing to wait for, the
            // row asks for them itself
            if self.o.more.read != Read::NoModels {
                self.o.more.then = Some(row);
                return Some(started);
            }
        }
        match self.o.more.read {
            Read::Idle | Read::Reading => {
                self.o.more.then = Some(row);
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Copy text: the area's text (clean capture, hidden parts out) on the
    /// clipboard with the Copied pill, as the bar's Copy text does.
    pub(in crate::ui) fn more_copy_text(&mut self) -> Task<Message> {
        if let Some(wait) = self.after_read(Row::CopyText) {
            return wait;
        }
        match self.o.more.read {
            Read::NoModels => {
                // the existing download card (beside the toolbar), then copy
                self.o.copy_after = true;
                self.o.reader = Reader::Missing(ocr::download_size());
                Task::none()
            }
            Read::Text => {
                let text = more::text_of(&self.o.more.blocks);
                self.copy_text(text)
            }
            _ => Task::none(), // no text: the row is off
        }
    }

    /// The models came: read again, then copy.
    pub(in crate::ui) fn more_after_download(&mut self) -> Task<Message> {
        self.o.more.key = None;
        self.o.more.then = Some(Row::CopyText);
        self.read_area()
    }

    /// Translate: on the clean capture (hidden parts painted out), in place
    /// inside the area; the picture the toolbar copies, saves and drags out is
    /// the translated one. While it is on the row reads Show original.
    pub(in crate::ui) fn more_translate(&mut self) -> Task<Message> {
        if self.o.more.shown || self.o.translate.is_some() {
            return self.show_original();
        }
        if let Some(wait) = self.after_read(Row::Translate) {
            return wait;
        }
        if self.o.more.read == Read::NoText {
            return Task::none();
        }
        let Some(edit) = &self.o.edit else {
            return Task::none();
        };
        let doc = &edit.annotator.doc;
        let view = doc.view();
        let (w, h) = (doc.image.width, doc.image.height);
        let edge = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
        let area = [
            edge(view.x.floor(), w),
            edge(view.y.floor(), h),
            edge((view.x + view.width).ceil(), w),
            edge((view.y + view.height).ceil(), h),
        ];
        // ponytail: a masked copy of the whole picture while it translates
        // (8 MB at 1080p); the translator wants a frame, not an area.
        let original = doc.image.rgba.clone();
        let mut frame = Crop {
            width: w,
            height: h,
            rgba: original.clone(),
        };
        more::mask(&mut frame, &annotate::hidden(doc));
        let frame = image::Handle::from_rgba(w, h, frame.rgba);
        self.o.more.was_own = edit.annotator.own_image;
        self.o.more.original = Some(original);
        self.o.ai_chat = None;
        let (session, task) = translate::Session::start(
            frame,
            area,
            self.o.scale,
            self.cache.clone(),
            text::models_present(),
            0,
        );
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.o.translate = Some((id, session));
        self.sync_more();
        task.map(move |msg| Message::Translate(id, msg))
    }

    /// The translation on the picture, as far as it is (called after each
    /// message of the session): pasted into the picture under the drawing,
    /// hidden parts as they were.
    pub(in crate::ui) fn sync_translation(&mut self) {
        if !self.more_editing() {
            return;
        }
        let Some((_, session)) = &self.o.translate else {
            return;
        };
        let Some((
            image::Handle::Rgba {
                width,
                pixels: from,
                ..
            },
            area,
            whole,
        )) = session.translated()
        else {
            return;
        };
        let (width, from) = (*width as usize, from.clone());
        let o = &mut *self.o;
        let (Some(original), Some(edit)) = (&o.more.original, o.edit.as_mut()) else {
            return;
        };
        let hidden = annotate::hidden(&edit.annotator.doc);
        let dst_w = edit.annotator.doc.image.width as usize;
        let [x0, y0, x1, y1] = area.map(|v| v as usize);
        let dst = &mut edit.annotator.doc.image.rgba;
        for y in y0..y1 {
            let src = match whole {
                true => (y * width + x0) * 4,
                false => (y - y0) * (x1 - x0) * 4,
            };
            let at = (y * dst_w + x0) * 4;
            let row = (x1 - x0) * 4;
            if dst.len() >= at + row && from.len() >= src + row {
                dst[at..at + row].copy_from_slice(&from[src..src + row]);
            }
        }
        more::keep_hidden(dst, original, dst_w, [x0, y0, x1, y1], &hidden);
        edit.annotator.own_image = true; // its pixels are no longer the frozen frame's
        edit.annotator.refresh();
        o.more.shown = true;
        self.sync_more();
    }

    /// Show original: the picture as it was; the translation goes.
    pub(in crate::ui) fn show_original(&mut self) -> Task<Message> {
        self.o.translate = None;
        let (original, was_own) = (self.o.more.original.take(), self.o.more.was_own);
        self.o.more.shown = false;
        if let (Some(original), Some(edit)) = (original, self.o.edit.as_mut()) {
            let image = &mut edit.annotator.doc.image.rgba;
            if image.len() == original.len() {
                image.copy_from_slice(&original);
            }
            edit.annotator.own_image = was_own;
            edit.annotator.refresh();
        }
        self.sync_more();
        Task::none()
    }

    /// The area is gone (Back, a new pick): nothing of More stays.
    pub(in crate::ui) fn more_reset(&mut self) {
        self.o.more = More::default();
        if matches!(self.o.purpose, Purpose::Snip(_)) {
            self.o.translate = None;
        }
    }
}

/// Reads `crop`'s lines as main mode's bar does: Windows OCR where
/// `ocr::route` says so (Windows), else the PP-OCR helper (Linux and the
/// languages Windows OCR cannot read).
fn read_crop(
    crop: &Crop,
    setting: config::TextReader,
    scale: f32,
) -> Result<Vec<TextBlock>, String> {
    // tests feed `on_more_read` themselves: no helper process, no models
    if cfg!(test) {
        return Err("no reader in tests".into());
    }
    if ocr::route(setting, Some(ocr::system::language())) == ocr::Source::System {
        let k = *ocr::system::passes(scale, crop.width, crop.height)
            .last()
            .unwrap_or(&1);
        match ocr::system::recognize(&crop.rgba, crop.width, crop.height, k) {
            Ok(blocks) => return Ok(blocks),
            Err(e) => crate::log!("[opencircle] Windows OCR (More): {e}; using the helper"),
        }
    }
    ocr::recognize_in_child(&crop.rgba, crop.width, crop.height).map_err(|e| e.to_string())
}

#[cfg(test)]
impl App {
    /// Tests and snapshots: which read is the latest.
    pub(in crate::ui) fn more_reads(&self) -> u32 {
        self.o.more.reads
    }

    /// Tests and snapshots: the session's translation arrives (the area's text
    /// lines drawn as translated bars on white) and goes into the picture.
    pub(in crate::ui) fn show_translated_for_test(&mut self) {
        let Some(edit) = &self.o.edit else {
            return;
        };
        let doc = &edit.annotator.doc;
        let (w, h, view) = (doc.image.width, doc.image.height, doc.view());
        let mut rgba = doc.image.rgba.clone();
        for y in view.y as u32..(view.y + view.height) as u32 {
            for x in view.x as u32..(view.x + view.width) as u32 {
                let bar = (y as f32 - view.y) as u32 % 26 < 9 && (x as f32 - view.x) < 420.0;
                let at = ((y * w + x) * 4) as usize;
                let c = if bar { [60, 70, 90] } else { [250, 250, 252] };
                rgba[at..at + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        if let Some((_, session)) = self.o.translate.as_mut() {
            session.show(image::Handle::from_rgba(w, h, rgba));
        }
        self.sync_translation();
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Rectangle};

    use crate::Trigger;
    use crate::actions::{copy, snip_more as more, text};
    use crate::labs::Labs;
    use crate::ocr::TextBlock;
    use crate::snip::annotate::{Edit, Item, Menu, Message as E, Shape};
    use crate::ui::app::App;
    use crate::ui::app::message::{Hiding, Message};
    use crate::ui::app::tests::{run_task, snipping_on};
    use crate::ui::kit::testing::shot;
    use crate::ui::screens::snip_toolbar::toolbar;

    const AREA: Rectangle = Rectangle {
        x: 100.0,
        y: 80.0,
        width: 300.0,
        height: 200.0,
    };

    /// Snip with an area picked and its toolbar up; the text reader's models "here".
    fn open() -> App {
        text::MODELS_HERE.set(Some(true));
        let mut app = snipping_on(800, 600, AREA);
        app.o.labs = Labs::all_on();
        app.select(AREA);
        let crop = app.crop().unwrap();
        let _ = app.snip_routed(crop, true, false);
        assert!(app.editing());
        app
    }

    fn block(text: &str, x: f32, y: f32) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds: [x, y, 100.0, 12.0],
            confidence: 1.0,
        }
    }

    fn texts(app: &App) -> Vec<String> {
        shot::texts(app.view(app.overlay))
    }

    /// The text rows and whether they are live (not dimmed).
    fn rows(app: &App) -> Vec<(more::Row, bool)> {
        let e = &app.o.edit.as_ref().unwrap().annotator;
        (toolbar::more_rows(e).into_iter())
            .filter_map(|r| r.of.map(|row| (row, r.message.is_some())))
            .collect()
    }

    fn open_more(app: &mut App) {
        let _ = app.on_snip_toolbar(E::Menu(Menu::More));
    }

    /// The area is read when More opens, once per area: opening again reads
    /// nothing; moving the area reads it again. Until More opens, nothing is
    /// read (the CPU stays at 0).
    #[test]
    fn the_area_is_read_when_more_opens_once() {
        let mut app = open();
        assert_eq!(app.o.more.read, more::Read::Idle, "nothing before More");
        open_more(&mut app);
        assert_eq!(app.o.more.read, more::Read::Reading);
        let (opening, first) = (app.opening, app.o.more.reads);
        let task = app.on_more_read(opening, first, Ok(vec![block("hello", 5.0, 5.0)]));
        run_task(&mut app, task);
        assert_eq!(app.o.more.read, more::Read::Text);
        // closed and opened again: the same read stands
        open_more(&mut app);
        open_more(&mut app);
        assert_eq!(app.o.more.reads, first, "not read again");
        // the area moves: it is read anew, and the old read's answer is dropped
        open_more(&mut app); // closed
        let _ = app.on_snip_toolbar(E::Nudge((1, 0), (false, false, false)));
        open_more(&mut app);
        assert_eq!(app.o.more.reads, first + 1, "the moved area is read anew");
        let _ = app.on_more_read(opening, first, Ok(vec![]));
        assert_eq!(app.o.more.read, more::Read::Reading, "stale answer ignored");
    }

    /// While it reads, a click on Copy text waits for the read; no text: Copy
    /// text and Translate are dimmed (arrow keys skip them), Ask AI is not,
    /// and the foot says why.
    #[test]
    fn reading_and_no_text_states() {
        let mut app = open();
        open_more(&mut app);
        let info = &app.o.edit.as_ref().unwrap().annotator.more;
        assert_eq!(info.read, more::Read::Reading);
        let _ = app.on_snip_toolbar(E::CopyText);
        assert_eq!(app.o.more.then, Some(more::Row::CopyText));
        let (opening, id) = (app.opening, app.o.more.reads);
        let task = app.on_more_read(opening, id, Ok(vec![]));
        run_task(&mut app, task);
        open_more(&mut app); // (the click closed the menu)
        assert_eq!(app.o.more.read, more::Read::NoText);
        assert_eq!(
            rows(&app),
            [
                (more::Row::CopyText, false),
                (more::Row::AskAi, true),
                (more::Row::Translate, false)
            ]
        );
        assert!(
            texts(&app)
                .iter()
                .any(|t| t == "No text found in this area")
        );
        // the arrow keys skip the dimmed rows: the first live one is Ask AI
        let _ = app.on_snip_toolbar(E::MenuStep(false));
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        let all = toolbar::more_rows(editor);
        assert_eq!(all[editor.row.unwrap()].of, Some(more::Row::AskAi));
        // Home / End: the first and the last live rows
        let _ = app.on_snip_toolbar(E::MenuEdge(true));
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        let last = &toolbar::more_rows(editor)[editor.row.unwrap()];
        assert_eq!(crate::app_language::tr(last.name), "Screen ruler");
        let _ = app.on_snip_toolbar(E::MenuEdge(false));
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        let first = &toolbar::more_rows(editor)[editor.row.unwrap()];
        assert_eq!(first.of, Some(more::Row::AskAi));
    }

    /// The rows show the user's own quick-action shortcuts as caps, one per
    /// key, from the config; the menu is 268 px wide (304 in Arabic).
    #[test]
    fn the_rows_show_the_users_own_keys() {
        let mut app = open();
        open_more(&mut app);
        let info = &app.o.edit.as_ref().unwrap().annotator.more;
        assert_eq!(info.caps, more::caps_from(&crate::config::current()));
        // (while it reads, the spinner takes the caps' place on the text rows)
        let (opening, id) = (app.opening, app.o.more.reads);
        let _ = app.on_more_read(opening, id, Ok(vec![block("hello", 5.0, 5.0)]));
        let shown = texts(&app);
        for cap in ["Ctrl", "Shift", "X", "E", "T"] {
            assert!(shown.iter().any(|t| t == cap), "{cap}: {shown:?}");
        }
        assert_eq!((toolbar::MORE_W, toolbar::MORE_W_AR), (268.0, 304.0));
    }

    /// Copy text copies what was read, minus every line under Blur or Redact:
    /// a hidden secret never reaches the clipboard.
    #[test]
    fn copy_text_leaves_out_a_redacted_line() {
        let mut app = open();
        // the user redacts the second line before opening More (image px)
        let redact = Item {
            shape: Shape::Redact {
                a: Point::new(100.0, 100.0),
                b: Point::new(300.0, 120.0),
            },
            colour: [0, 0, 0, 255],
            width: 1.0,
            outline: false,
            fill: false,
        };
        let editor = &mut app.o.edit.as_mut().unwrap().annotator;
        let n = editor.doc.items.len();
        editor.doc.exec(Edit::Insert(n, redact));
        open_more(&mut app);
        let (opening, id) = (app.opening, app.o.more.reads);
        // (the paint-out leaves the reader nothing; a reader that still finds
        // the line anyway is caught by the second guard)
        let blocks = vec![
            block("public line", 5.0, 2.0),
            block("secret password", 5.0, 22.0),
        ];
        let _ = app.on_more_read(opening, id, Ok(blocks));
        let _ = copy::copied::text();
        let task = app.on_snip_toolbar(E::CopyText);
        run_task(&mut app, task);
        let copied = copy::copied::text();
        assert_eq!(copied.as_deref(), Some("public line"), "{copied:?}");
    }

    /// Ask AI opens the chat over the picked area with the picture; Esc then
    /// closes chat and Snip together (a quick question).
    #[test]
    fn ask_ai_opens_the_chat_and_esc_closes_both() {
        let mut app = open();
        open_more(&mut app);
        let _ = app.on_snip_toolbar(E::AskAi);
        let menu = app.o.edit.as_ref().unwrap().annotator.menu;
        assert_eq!(menu, None, "a row closes the menu");
        assert!(
            app.o.ai_chat.is_some(),
            "the chat is up over the picked area"
        );
        assert!(app.editing(), "the snip stays picked");
        let _ = app.on_snip_toolbar(E::Back);
        assert!(app.hiding != Hiding::No, "Esc closes both");
    }

    /// Long capture: Translate is off with its reason; Copy text and Ask AI
    /// stay on (they finish the capture first).
    #[test]
    fn long_capture_turns_translate_off() {
        let mut app = open();
        app.o.edit.as_mut().unwrap().annotator.capturing = true;
        open_more(&mut app);
        assert_eq!(
            rows(&app),
            [
                (more::Row::CopyText, true),
                (more::Row::AskAi, true),
                (more::Row::Translate, false)
            ]
        );
        let shown = texts(&app);
        assert!(
            shown
                .iter()
                .any(|t| t == "Translate works on one screen at a time")
        );
    }

    /// During a long capture More still opens: Copy text and Ask AI finish the
    /// capture (as Copy and Save do), then act on the stitched page;
    /// Translate is off and says why.
    #[test]
    fn long_capture_finishes_first_for_copy_text_and_ask_ai() {
        use crate::ui::screens::long_capture::Finish;
        let mut app = open();
        let _ = app.on_snip_toolbar(E::Scroll);
        assert!(app.scroll().is_some(), "the long capture runs");
        // More opens meanwhile (the toolbar keeps it live)
        let _ = app.on_snip_toolbar(E::Menu(Menu::More));
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        assert_eq!(editor.menu, Some(Menu::More));
        assert_eq!(
            rows(&app),
            [
                (more::Row::CopyText, true),
                (more::Row::AskAi, true),
                (more::Row::Translate, false)
            ]
        );
        // Enter on Copy text (the first live row): the capture ends and copies text
        let _ = app.on_snip_toolbar(E::MenuStep(false));
        let _ = app.on_snip_toolbar(E::Activate);
        assert_eq!(app.scroll().and_then(|l| l.then), Some(Finish::CopyText));
        // Ask AI by its key: the same, for the chat
        let mut app = open();
        let _ = app.on_snip_toolbar(E::Scroll);
        let _ = app.on_snip_toolbar(E::AskAi);
        assert_eq!(app.scroll().and_then(|l| l.then), Some(Finish::AskAi));
        // Translate cannot
        let mut app = open();
        let _ = app.on_snip_toolbar(E::Scroll);
        let _ = app.on_snip_toolbar(E::Translate);
        assert_eq!(app.scroll().and_then(|l| l.then), None);
    }

    /// The user's quick-action key acts on the picked area while Snip is up
    /// (Ctrl+Shift+E = Ask AI), never as a new capture; the live toggle does not.
    #[test]
    fn the_quick_key_runs_the_row_on_the_area() {
        let mut app = open();
        let cfg = crate::config::Config::default();
        let ask = (cfg.quick_actions.iter())
            .find(|q| q.action == "ai_explain")
            .unwrap();
        assert_eq!(app.more_row_of(ask), Some(more::Row::AskAi));
        let live = cfg.quick_actions.iter().find(|q| q.live).unwrap();
        assert_eq!(app.more_row_of(live), None);
        let quick = Trigger::Quick(std::time::Instant::now(), ask.clone());
        let task = app.update(Message::Trigger(quick));
        run_task(&mut app, task);
        assert!(app.o.ai_chat.is_some());
    }

    /// Translate puts a session on the area (the picture's own pixels are the
    /// original kept); the row again (now Show original) cancels it and every
    /// pixel is as it was.
    #[test]
    fn show_original_puts_every_pixel_back() {
        let mut app = open();
        open_more(&mut app);
        let (opening, id) = (app.opening, app.o.more.reads);
        let _ = app.on_more_read(opening, id, Ok(vec![block("hola", 5.0, 5.0)]));
        let before = app
            .o
            .edit
            .as_ref()
            .unwrap()
            .annotator
            .doc
            .image
            .rgba
            .clone();
        let _ = app.on_snip_toolbar(E::Translate);
        assert!(
            app.o.translate.is_some() && !app.o.more.shown,
            "translating"
        );
        assert_eq!(app.o.more.original.as_deref(), Some(before.as_slice()));
        let _ = app.on_snip_toolbar(E::Translate);
        assert!(app.o.translate.is_none() && !app.o.more.shown);
        let after = &app.o.edit.as_ref().unwrap().annotator.doc.image.rgba;
        assert_eq!(after, &before, "nothing of the translation stays");
    }
}
