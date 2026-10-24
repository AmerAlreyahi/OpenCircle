//! The live translate mini-controller (Circle to Search's floating controller):
//! a small, draggable window on the layer's monitor. It takes clicks (the layer
//! doesn't) and, on Windows, is excluded from capture like the layer. The
//! language picker (source → target, `ui/picker.rs`), Pause / Resume, Hide
//! (collapses to a dot), Stop. This file is its state and view; `live_layer`
//! owns the window.
use iced::widget::{column, container, mouse_area, row};
use iced::{Center, Element, Size, mouse};

use super::picker::{self, CHIP_HEIGHT, Picker, SPACING};
use super::widgets;

/// Chip sizes (one width per kind, consistency rule) and the drag grip.
const CHIP_WIDTH: f32 = 76.0;
const GRIP_WIDTH: f32 = 20.0;
/// The minimised controller.
const DOT: f32 = 28.0;

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
}

/// What the layer must do after a message.
#[derive(Debug, PartialEq)]
pub enum Out {
    Drag,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    /// The source and target languages (start as the config's).
    pub picker: Picker,
    pub paused: bool,
    pub minimised: bool,
}

impl Controller {
    pub fn new(source: &str, target: &str, offline: bool) -> Controller {
        Controller {
            picker: Picker::new(source, target, offline),
            paused: false,
            minimised: false,
        }
    }

    pub fn update(&mut self, msg: Msg) -> Option<Out> {
        match msg {
            Msg::Drag => return Some(Out::Drag),
            Msg::Stop => return Some(Out::Stop),
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

    /// The status line: "Auto → AR", or paused.
    pub fn status(&self) -> String {
        match self.paused {
            true => "Live · paused".into(),
            false => format!("Live · {}", self.picker.label()),
        }
    }

    /// The window's size for what it shows now (logical px).
    pub fn size(&self) -> Size {
        if self.minimised {
            return Size::new(DOT, DOT);
        }
        let pad = 2.0 * widgets::SURFACE_PADDING;
        let list = if self.picker.is_open() {
            picker::LIST_HEIGHT
        } else {
            0.0
        };
        Size::new(
            GRIP_WIDTH + picker::WIDTH + 3.0 * CHIP_WIDTH + 4.0 * SPACING + pad,
            CHIP_HEIGHT + list + pad,
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
        if let Some(list) = self.picker.list(width) {
            let list = list.map(Msg::Picker);
            body =
                body.push(container(list).padding(iced::Padding::ZERO.left(GRIP_WIDTH + SPACING)));
        }
        widgets::solid_surface(body).into()
    }
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
