//! The live translate mini-controller (Circle to Search's floating controller):
//! a small, draggable window on the layer's monitor. It takes clicks (the layer
//! doesn't) and, on Windows, is excluded from capture like the layer. The
//! language picker (source → target, `ui/picker.rs`), Pause / Resume, Hide
//! (collapses to a dot), Stop; and, when the languages' models are missing,
//! the download question (live mode never steps without them). This file is
//! its state and view; `live_layer` owns the window.
use iced::widget::{column, container, mouse_area, row};
use iced::{Center, Element, Fill, Size, mouse};

use super::picker::{self, CHIP_HEIGHT, Picker, SPACING};
use super::widgets;

/// Chip sizes (one width per kind, consistency rule) and the drag grip.
const CHIP_WIDTH: f32 = 76.0;
const GRIP_WIDTH: f32 = 20.0;
/// The minimised controller.
const DOT: f32 = 28.0;
/// The download question under the controls: two lines, then the buttons.
const ASK_TEXT: f32 = 40.0;
const ASK_HEIGHT: f32 = SPACING + ASK_TEXT + SPACING + CHIP_HEIGHT;
const ASK_BUTTON: f32 = 96.0;

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
}

impl Controller {
    pub fn new(source: &str, target: &str, offline: bool) -> Controller {
        Controller {
            picker: Picker::new(source, target, offline),
            paused: false,
            minimised: false,
            ask: None,
        }
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

    /// The status line: "Auto → AR", paused, or waiting for the download.
    pub fn status(&self) -> String {
        match (&self.ask, self.paused) {
            (Some(_), _) => "Live · needs a download".into(),
            (None, true) => "Live · paused".into(),
            (None, false) => format!("Live · {}", self.picker.label()),
        }
    }

    /// The window's size for what it shows now (logical px).
    pub fn size(&self) -> Size {
        if self.minimised {
            return Size::new(DOT, DOT);
        }
        let pad = 2.0 * widgets::SURFACE_PADDING;
        let below = match (&self.ask, self.picker.is_open()) {
            (Some(_), _) => ASK_HEIGHT,
            (None, true) => picker::LIST_HEIGHT,
            (None, false) => 0.0,
        };
        Size::new(
            GRIP_WIDTH + picker::WIDTH + 3.0 * CHIP_WIDTH + 4.0 * SPACING + pad,
            CHIP_HEIGHT + below + pad,
        )
    }

    pub fn view(&self) -> Element<'_, Msg> {
        if self.minimised {
            return widgets::accent_dot(DOT, Msg::Restore).into();
        }
        let chip = |label: &'static str, msg| {
            widgets::chip(label, msg)
                .width(CHIP_WIDTH)
                .height(CHIP_HEIGHT)
        };
        let grip = mouse_area(
            container(widgets::caption("⋮⋮"))
                .center(GRIP_WIDTH)
                .height(CHIP_HEIGHT),
        )
        .on_press(Msg::Drag)
        .interaction(mouse::Interaction::Grab);
        let controls = row![
            grip,
            self.picker.chips(CHIP_HEIGHT).map(Msg::Picker),
            chip(if self.paused { "Resume" } else { "Pause" }, Msg::Pause),
            chip("Hide", Msg::Minimise),
            chip("Stop", Msg::Stop),
        ]
        .spacing(SPACING)
        .align_y(Center);
        let mut body = column![controls].spacing(SPACING);
        let width = self.size().width - 2.0 * widgets::SURFACE_PADDING - GRIP_WIDTH - SPACING;
        let indent = |e: Element<'static, Msg>| {
            container(e).padding(iced::Padding::ZERO.left(GRIP_WIDTH + SPACING))
        };
        if let Some(ask) = &self.ask {
            body = body.push(indent(ask_view(ask, width)));
        } else if let Some(list) = self.picker.list(width) {
            let list = list.map(Msg::Picker);
            body =
                body.push(container(list).padding(iced::Padding::ZERO.left(GRIP_WIDTH + SPACING)));
        }
        widgets::solid_surface(body).into()
    }
}

/// The download question: what and how big, then Download and Cancel (=
/// Stop), or the progress.
fn ask_view(ask: &Ask, width: f32) -> Element<'static, Msg> {
    let line = match &ask.error {
        Some(e) => widgets::error(format!("Download failed: {e}")),
        None => widgets::body(format!(
            "Live translate needs {}. Download it? Nothing you translate leaves your PC.",
            ask.what
        )),
    };
    let below: Element<'static, Msg> = match ask.progress {
        Some(done) => container(widgets::progress(done))
            .center_y(CHIP_HEIGHT)
            .into(),
        None => row![
            widgets::chip("Download", Msg::Download)
                .width(ASK_BUTTON)
                .height(CHIP_HEIGHT),
            widgets::chip("Cancel", Msg::Stop)
                .width(ASK_BUTTON)
                .height(CHIP_HEIGHT),
        ]
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
        assert_eq!(c.status(), "Live · Auto → AR");
        let full = c.size();
        assert_eq!(c.update(Msg::Pause), None);
        assert!(c.paused);
        assert_eq!(c.status(), "Live · paused");
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
        widgets::shot::save("live-controller", c.view(), closed);
        open(&mut c, Side::Target);
        assert!(c.size().height > closed.height, "the list needs room");
        assert_eq!(c.size().width, closed.width, "never wider");
        let texts = widgets::shot::texts(c.view());
        assert!(texts.iter().any(|t| t == "English"), "{texts:?}");
        let arabic = texts.iter().filter(|t| *t == "Arabic").count();
        assert_eq!(arabic, 1, "the chip only, not in the list: {texts:?}");
        let size = c.size();
        let px = widgets::shot::save("live-controller-picker", c.view(), size);
        assert!(px.chunks(4).any(|p| p != &px[..4]), "something drawn");
        c.update(Msg::Picker(picker::Msg::Query("fren".into())));
        widgets::shot::save("live-controller-search", c.view(), size);
        c.update(Msg::Picker(picker::Msg::First));
        assert_eq!(c.picker.target, "fr");
        assert_eq!(c.status(), "Live · Auto → FR");
        assert_eq!(c.size(), closed);
        open(&mut c, Side::Source);
        c.update(Msg::Picker(picker::Msg::Pick("ja".into())));
        assert_eq!(c.status(), "Live · JA → FR");
        // the view builds in every state
        c.update(Msg::Minimise);
        let _ = c.view();
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
}
