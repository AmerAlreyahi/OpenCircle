//! The live translate mini-controller (Circle to Search's floating controller):
//! the one small, draggable widget of live mode, at the bottom right of the
//! layer's monitor (the layer shows nothing else; errors and the first-run
//! note are its notice line). It takes clicks (the layer
//! doesn't) and, on Windows, is excluded from capture like the layer. The
//! language picker (source → target, `ui/picker.rs`), Pause / Resume, Hide
//! (collapses to a dot), Stop; and, when the languages' models are missing,
//! the download question (live mode never steps without them). This file is
//! its state and view (kit parts only: the widget surface, the grip, the
//! language picker's openers and list, `.oc-btn`s); `live_layer` owns the
//! window.
use iced::widget::{column, container, row};
use iced::{Center, Element, Fill, Size};

use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::grip::{self, grip};
use crate::ui::kit::progress;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, radius};
use crate::ui::panel::note;
use crate::ui::picker::{self, CHIP_HEIGHT, Picker, SPACING};

/// The window's corners (`rounded-2xl`) and the room inside its edge (`p-2`).
pub const RADIUS: f32 = radius::WINDOW;
const PAD: f32 = 8.0;
/// Button cells (one width per kind, consistency rule) and the drag grip.
const CHIP_WIDTH: f32 = 76.0;
const GRIP_WIDTH: f32 = grip::SIZE.width;
/// The minimised controller.
const DOT: f32 = 28.0;
/// The download question under the controls: two lines, then the buttons.
const ASK_TEXT: f32 = 40.0;
const ASK_HEIGHT: f32 = SPACING + ASK_TEXT + SPACING + CHIP_HEIGHT;
/// The one-line notice (an error, the first-run note) above the controls.
const NOTICE_HEIGHT: f32 = 24.0;
/// Longest notice shown (one line).
const NOTICE_CHARS: usize = 64;

#[derive(Debug, Clone)]
pub enum Msg {
    /// Pressed on the grip: move the window with the pointer.
    Drag,
    /// The language picker.
    Picker(picker::Msg),
    Pause,
    Minimise,
    Restore,
    Stop,
    /// The download question's Download.
    Download,
}

/// What the layer must do after a message.
#[derive(Debug, PartialEq)]
pub enum Out {
    Drag,
    Stop,
    /// The user agreed to the download.
    Download,
}

/// Models the languages need are missing: what to download, and how it goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// "the text-recognition model (about 40 MB)".
    pub what: String,
    /// Downloading, 0..=1.
    pub progress: Option<f32>,
    /// The last attempt failed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    /// The source and target languages (start as the config's).
    pub picker: Picker,
    pub paused: bool,
    pub minimised: bool,
    /// The download question (live mode doesn't step while it is up).
    pub ask: Option<Ask>,
    /// One line about the layer: the last step's error, or the first-run note.
    pub notice: Option<String>,
}

impl Controller {
    pub fn new(source: &str, target: &str, offline: bool) -> Controller {
        Controller {
            picker: Picker::new(source, target, offline),
            paused: false,
            minimised: false,
            ask: None,
            notice: None,
        }
    }

    /// Sets the notice, cut to one line.
    pub fn set_notice(&mut self, text: Option<String>) {
        self.notice = text.map(|t| match t.chars().count() > NOTICE_CHARS {
            true => t.chars().take(NOTICE_CHARS - 1).chain(['…']).collect(),
            false => t,
        });
    }

    /// Asks before downloading `what` ("English → Arabic offline
    /// translation") of `bytes`.
    pub fn ask(&mut self, what: &str, bytes: u64) {
        let mb = bytes.div_ceil(10_000_000) * 10;
        self.picker.close();
        self.minimised = false;
        self.ask = Some(Ask {
            what: format!("{what} (about {mb} MB)"),
            progress: None,
            error: None,
        });
    }

    pub fn update(&mut self, msg: Msg) -> Option<Out> {
        match msg {
            Msg::Drag => return Some(Out::Drag),
            Msg::Stop => return Some(Out::Stop),
            Msg::Download => match &mut self.ask {
                Some(ask) if ask.progress.is_none() => {
                    ask.progress = Some(0.0);
                    ask.error = None;
                    return Some(Out::Download);
                }
                _ => {}
            },
            Msg::Picker(msg) => {
                self.picker.update(msg);
            }
            Msg::Pause => self.paused = !self.paused,
            Msg::Minimise => {
                self.minimised = true;
                self.picker.close();
            }
            Msg::Restore => self.minimised = false,
        }
        None
    }

    /// The window's size for what it shows now (logical px).
    pub fn size(&self) -> Size {
        if self.minimised {
            return Size::new(DOT, DOT);
        }
        let pad = 2.0 * PAD;
        let mut below = match (&self.ask, self.picker.is_open()) {
            (Some(_), _) => ASK_HEIGHT,
            (None, true) => picker::LIST_HEIGHT,
            (None, false) => 0.0,
        };
        if self.notice.is_some() {
            below += SPACING + NOTICE_HEIGHT;
        }
        Size::new(
            GRIP_WIDTH + picker::WIDTH + 3.0 * CHIP_WIDTH + 4.0 * SPACING + pad,
            CHIP_HEIGHT + below + pad,
        )
    }

    pub fn view<'a>(&'a self) -> Element<'a, Msg> {
        if self.minimised {
            return crate::ui::kit::button::dot(DOT, Msg::Restore);
        }
        let controls = row![
            container(grip(Msg::Drag)).center_y(CHIP_HEIGHT),
            self.picker.chips(CHIP_HEIGHT).map(Msg::Picker),
            cell(if self.paused { "Resume" } else { "Pause" }, Msg::Pause),
            cell("Hide", Msg::Minimise),
            cell("Stop", Msg::Stop),
        ]
        .spacing(SPACING)
        .align_y(Center);
        // The window grows upward from the controls (it sits at the bottom of
        // the screen), so what opens goes above them.
        let mut body = column![].spacing(SPACING);
        let width = self.size().width - 2.0 * PAD - GRIP_WIDTH - SPACING;
        let indent = |e: Element<'a, Msg>| {
            container(e).padding(iced::Padding::ZERO.left(GRIP_WIDTH + SPACING))
        };
        if let Some(ask) = &self.ask {
            body = body.push(indent(ask_view(ask, width)));
        } else if let Some(list) = self.picker.list(width) {
            body = body.push(indent(list.map(Msg::Picker)));
        }
        if let Some(notice) = &self.notice {
            let line = container(note(notice.clone()))
                .center_y(NOTICE_HEIGHT)
                .width(width)
                .clip(true);
            body = body.push(indent(line.into()));
        }
        let body = body.push(controls);
        let size = self.size();
        surface(Material::Widget, RADIUS, body)
            .padding(PAD)
            .width(size.width)
            .height(size.height)
            .into()
    }
}

/// A button (`.oc-btn`) centred in a `CHIP_WIDTH` cell.
fn cell<'a>(name: &'a str, msg: Msg) -> Element<'a, Msg> {
    container(button(label(name), Kind::Elevated, Some(msg)))
        .center_x(CHIP_WIDTH)
        .center_y(CHIP_HEIGHT)
        .into()
}

/// The download question: what and how big, then Download and Cancel (=
/// Stop), or the progress.
fn ask_view(ask: &Ask, width: f32) -> Element<'static, Msg> {
    let line: Element<'static, Msg> = match &ask.error {
        Some(e) => error(Role::NOTE, format!("Download failed: {e}")).into(),
        None => note(format!(
            "Live translate needs {}. Download it? Nothing you translate leaves your PC.",
            ask.what
        ))
        .into(),
    };
    let below: Element<'static, Msg> = match ask.progress {
        Some(done) => container(progress::bar(done)).center_y(CHIP_HEIGHT).into(),
        None => row![cell("Download", Msg::Download), cell("Cancel", Msg::Stop)]
            .spacing(SPACING)
            .into(),
    };
    column![container(line).height(ASK_TEXT).width(Fill), below]
        .spacing(SPACING)
        .width(width)
        .into()
}

/// Paints `rect` (x, y, w, h; clipped to the frame) black in an RGBA capture:
/// where a capture shows the controller (X11), live mode never reads its labels.
pub fn mask(rgba: &mut [u8], width: u32, height: u32, rect: [u32; 4]) {
    let [x, y, w, h] = rect;
    let (x0, x1) = (x.min(width), x.saturating_add(w).min(width));
    for row in y.min(height)..y.saturating_add(h).min(height) {
        let start = (row * width + x0) as usize * 4;
        let end = (row * width + x1) as usize * 4;
        if let Some(px) = rgba.get_mut(start..end) {
            px.as_chunks_mut::<4>().0.fill([0, 0, 0, 255]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::picker::Side;

    fn open(c: &mut Controller, side: Side) {
        c.update(Msg::Picker(picker::Msg::Open(side)));
    }

    #[test]
    fn pause_hide_and_stop() {
        let mut c = Controller::new("auto", "ar", false);
        assert_eq!(c.picker.label(), "Auto → AR");
        let full = c.size();
        assert_eq!(c.update(Msg::Pause), None);
        assert!(c.paused);
        assert_eq!(c.update(Msg::Pause), None);
        assert!(!c.paused, "Pause again = Resume");
        // Hide: a dot, and the list closes with it
        open(&mut c, Side::Target);
        c.update(Msg::Minimise);
        assert!(c.minimised && !c.picker.is_open());
        assert_eq!(c.size(), Size::new(DOT, DOT));
        c.update(Msg::Restore);
        assert_eq!(c.size(), full);
        assert_eq!(c.update(Msg::Drag), Some(Out::Drag));
        assert_eq!(c.update(Msg::Stop), Some(Out::Stop));
    }

    /// The picker in the controller; PNGs of it closed and open (headless
    /// renderer, `OPENCIRCLE_RENDER_DIR=<dir>`).
    #[test]
    fn the_language_picker_switches_source_and_target() {
        let mut c = Controller::new("auto", "ar", false);
        let closed = c.size();
        crate::ui::kit::testing::shot::save("live-controller", c.view(), closed);
        open(&mut c, Side::Target);
        assert!(c.size().height > closed.height, "the list needs room");
        assert_eq!(c.size().width, closed.width, "never wider");
        let texts = crate::ui::kit::testing::shot::texts(c.view());
        assert!(texts.iter().any(|t| t == "English"), "{texts:?}");
        let arabic = texts.iter().filter(|t| *t == "Arabic").count();
        assert_eq!(arabic, 1, "the chip only, not in the list: {texts:?}");
        let size = c.size();
        let px = crate::ui::kit::testing::shot::save("live-controller-picker", c.view(), size);
        assert!(px.chunks(4).any(|p| p != &px[..4]), "something drawn");
        c.update(Msg::Picker(picker::Msg::Query("fren".into())));
        crate::ui::kit::testing::shot::save("live-controller-search", c.view(), size);
        c.update(Msg::Picker(picker::Msg::First));
        assert_eq!(c.picker.target, "fr");
        assert_eq!(c.picker.label(), "Auto → FR");
        assert_eq!(c.size(), closed);
        open(&mut c, Side::Source);
        c.update(Msg::Picker(picker::Msg::Pick("ja".into())));
        assert_eq!(c.picker.label(), "JA → FR");
        // the view builds in every state
        c.update(Msg::Minimise);
        let _ = c.view();
    }

    /// The window is exactly what it shows: drawn in a bigger canvas, the
    /// surface's border is the last row / column of `size()`, and nothing
    /// is drawn past it (closed, list open, notice, download question, dot).
    #[test]
    fn the_window_is_exactly_the_size_of_what_is_visible() {
        let mut c = Controller::new("auto", "ar", false);
        let mut states = vec![("closed", c.clone())];
        c.update(Msg::Picker(picker::Msg::Open(Side::Target)));
        states.push(("list", c.clone()));
        c.set_notice(Some("x".repeat(500)));
        assert!(c.notice.as_ref().unwrap().chars().count() <= NOTICE_CHARS);
        states.push(("list+notice", c.clone()));
        c.picker.close();
        states.push(("notice", c.clone()));
        c.ask("the model", 40_000_000);
        states.push(("ask", c.clone()));
        c.minimised = true;
        c.notice = None;
        states.push(("dot", c.clone()));
        let bg = crate::ui::kit::themes::c().bg;
        let bg = [bg.r, bg.g, bg.b, bg.a].map(|v| (v * 255.0).round() as u8);
        for (name, c) in states {
            let size = c.size();
            let (w, h) = (size.width as u32 + 40, size.height as u32 + 40);
            let px = crate::ui::kit::testing::shot::render(c.view(), w, h);
            let at = |x: u32, y: u32| px[((y * w + x) * 4) as usize..][..4].to_vec();
            let (sw, sh) = (size.width as u32, size.height as u32);
            let (mx, my) = (sw / 2, sh / 2);
            if name != "dot" {
                assert_ne!(at(sw - 1, my), bg, "{name}: right edge");
                assert_ne!(at(mx, sh - 1), bg, "{name}: bottom edge");
            }
            for d in 2..20 {
                assert_eq!(at(sw - 1 + d, my), bg, "{name}: empty box right");
                assert_eq!(at(mx, sh - 1 + d), bg, "{name}: empty box below");
            }
        }
    }

    #[test]
    fn mask_blanks_only_its_rect_and_clips() {
        let mut rgba = vec![9; 4 * 3 * 4];
        mask(&mut rgba, 4, 3, [2, 1, 10, 10]);
        let black = |x: usize, y: usize| rgba[(y * 4 + x) * 4..][..4] == [0, 0, 0, 255];
        assert!(black(2, 1) && black(3, 2));
        assert!(!black(1, 1) && !black(2, 0));
        mask(&mut rgba, 4, 3, [9, 9, 2, 2]); // off the frame: nothing
    }

    /// The controller closed, with the target list open, with a notice, and
    /// asking for its download, in Sapphire and Daylight.
    #[test]
    fn every_controller_state_matches_its_golden_image() {
        use crate::ui::kit::testing::golden;
        use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
        let mut c = Controller::new("auto", "ar", true);
        let mut states = vec![("closed", c.clone())];
        open(&mut c, Side::Target);
        states.push(("list", c.clone()));
        c.picker.close();
        c.set_notice(Some("Nothing to translate on this screen yet.".into()));
        states.push(("notice", c.clone()));
        c.notice = None;
        c.ask("English → Arabic offline translation", 117_600_822);
        states.push(("ask", c.clone()));
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for (name, c) in &states {
                let file = format!("live-controller-{name}-{}", theme.id);
                failed.extend(golden::check("screens", &file, c.size(), &[1.0], || {
                    c.view()
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
