//! Snip's screen tools on the frozen screen (0.1.3-5): the colour picker
//! (N-75, `picker`) and the screen ruler (N-76, `ruler`). Each starts from
//! its Snip toolbar button or its own quick action (`opencircle run
//! colour_picker` / `ruler`, a Settings > Shortcuts row); both are Snip's,
//! behind its Labs switch. Over every frozen screen the pointer is on, they
//! read the frame in its true pixels (`frame px = logical px x scale`); the
//! toolbar sits at the top of the starting screen. Esc closes: back to the picked area when opened
//! from the toolbar, else the overlay closes.
use iced::widget::image;
use iced::widget::{Space, column, container, opaque, stack};
use iced::{Color, Element, Fill, Point, Rectangle, Size};

use crate::ui::kit::key_hint::{self, key_hint};
use crate::ui::kit::toast::{Tone, toast};
use crate::ui::kit::tokens::{radius, space};
use crate::ui::overlay;
use crate::ui::screens::editor::Editing;

pub mod picker;
pub mod ruler;

pub use crate::editor::ScreenTool as Which;

/// The glyphs the tools show (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[
    picker::ICON,
    ruler::ICON,
    ruler::LINE,
    ruler::ACROSS,
    ruler::DOWN,
    ruler::BOX,
    CLOSE,
];

/// lucide "x".
const CLOSE: char = '\u{e1b1}';
/// The toolbar's distance from the screen's top (or bottom) edge.
const EDGE_GAP: f32 = 24.0;
/// The Copied pill shows this long.
pub const COPIED_MS: u64 = 1500;

/// The quick action id that opens `which` (the config's and the CLI's).
pub fn for_action(id: &str) -> Option<Which> {
    match id {
        "colour_picker" => Some(Which::Picker),
        "ruler" => Some(Which::Ruler),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The ruler's mode switch.
    Mode(ruler::Mode),
    /// A recent colour's swatch: copied again (its place in the list).
    Recopy(usize),
    /// Copied pill `.0` has shown long enough.
    CopiedGone(u32),
    /// The toolbar's ×, and Esc.
    Close,
}

/// The running tool (`Snip::tool`).
pub struct Tool {
    pub which: Which,
    /// The pointer, the starting screen's logical px (as `Snip::pointer`);
    /// the arrows move it by physical px.
    pub at: Option<Point>,
    pub ruler: ruler::Ruler,
    /// Opened from the Snip toolbar on this picked area and its editor: Esc
    /// goes back to them (the annotations kept).
    pub back: Option<(Rectangle, Box<Editing>)>,
    copied: Option<(u32, String)>,
    copies: u32,
}

impl Tool {
    pub fn new(which: Which, back: Option<(Rectangle, Box<Editing>)>) -> Self {
        Tool {
            which,
            at: None,
            ruler: ruler::Ruler::default(),
            back,
            copied: None,
            copies: 0,
        }
    }

    /// The Copied pill says `what`; its number (for `CopiedGone`).
    pub fn copied(&mut self, what: String) -> u32 {
        self.copies = self.copies.wrapping_add(1);
        self.copied = Some((self.copies, what));
        self.copies
    }

    /// Pill `id` has shown long enough (a newer one stays).
    pub fn copied_gone(&mut self, id: u32) {
        if self.copied.as_ref().is_some_and(|(n, _)| *n == id) {
            self.copied = None;
        }
    }

    /// The colour under the pointer (`frame` / `scale` / `view`: the screen it is on).
    pub fn colour(
        &self,
        frame: &image::Handle,
        scale: f32,
        view: overlay::View,
    ) -> Option<[u8; 3]> {
        let (pixels, w, h) = rgba(frame)?;
        picker::colour_at(pixels, w, h, frame_px(self.at?, scale, view))
    }

    /// The ruler's measure now (`frame` / `scale` / `view`: the pointer's screen).
    pub fn measure(
        &self,
        frame: &image::Handle,
        scale: f32,
        view: overlay::View,
    ) -> Option<ruler::Measure> {
        let (pixels, w, h) = rgba(frame)?;
        let px = |p: Point| frame_px(p, scale, view);
        self.ruler
            .measure(self.at.map(px), px, |at, axis| {
                ruler::run(pixels, w, h, at, axis)
            })
            .map(|m| m.at(scale))
    }

    /// The tool over one screen's window (`screen` big; `frame` / `scale` /
    /// `view` as `Snip::guides`): the loupe or the measure while the pointer
    /// is on it; on the starting screen (`toolbar`) also the toolbar, its
    /// hint (`hinting`) and the Copied pill. `recent`: the picked colours,
    /// newest first; `look`: the active look's pair.
    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        frame: &'a image::Handle,
        scale: f32,
        view: overlay::View,
        screen: Size,
        toolbar: bool,
        hinting: bool,
        recent: &[[u8; 3]],
        look: (Color, Color),
    ) -> Element<'a, Msg> {
        // (a stack is as big as its first layer: the whole window)
        let mut layers = stack![Space::new().width(Fill).height(Fill)];
        let here = self
            .at
            .map(|p| view.to_screen(p))
            .filter(|p| Rectangle::with_size(screen).contains(*p));
        let measure = here.and(self.measure(frame, scale, view));
        if let Some(at) = here {
            layers = layers.push(match self.which {
                Which::Picker => picker::loupe(frame, scale, at, look),
                Which::Ruler => ruler::marks(measure.clone(), scale, at, screen, look),
            });
        }
        if !toolbar {
            return layers.into();
        }
        let bar: Element<'a, Msg> = match self.which {
            Which::Picker => {
                let under = self.colour(frame, scale, view);
                picker::toolbar(under, recent, false)
            }
            Which::Ruler => ruler::toolbar(self.ruler.mode, measure.as_ref(), false),
        };
        // under the toolbar: the Copied pill, the hint
        // (a click on it is never a pick or a measure)
        let mut top = column![opaque(bar)]
            .spacing(space::GAP)
            .align_x(iced::Center);
        if let Some((_, what)) = &self.copied {
            top = top.push(toast(what, Tone::Ok, radius::PILL));
        }
        if hinting {
            top = top.push(key_hint(hint(self.which), false));
        }
        // ponytail: always at the top (the overlay sees every move, also over
        // the toolbar, so one that moved away from the pointer could never be
        // clicked); the pixels under it are reached with the arrows.
        let placed = container(top).center_x(Fill).padding([EDGE_GAP, 0.0]);
        layers.push(placed).into()
    }
}

/// `frame`'s pixels and size (an RGBA frame).
fn rgba(frame: &image::Handle) -> Option<(&[u8], u32, u32)> {
    match frame {
        image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } => Some((pixels, *width, *height)),
        _ => None,
    }
}

/// `p` (the starting screen's logical px) in the frame px of the screen `view`
/// maps it to (`scale` = that screen's physical px per logical px).
pub fn frame_px(p: Point, scale: f32, view: overlay::View) -> (i32, i32) {
    let q = view.to_screen(p);
    ((q.x * scale).floor() as i32, (q.y * scale).floor() as i32)
}

/// The key hints, by tool.
pub fn hint(which: Which) -> &'static [key_hint::Group<'static>] {
    match which {
        Which::Picker => &picker::HINT,
        Which::Ruler => &ruler::HINT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_lands_on_its_true_pixel_at_any_scale() {
        let view = overlay::View::new(1920, 1080, Size::new(1920.0, 1080.0));
        assert_eq!(frame_px(Point::new(10.5, 3.2), 1.0, view), (10, 3));
        // 150 %: the frame is 1.5x the logical screen (the view maps 1:1 logical)
        let view = overlay::View::new(2880, 1620, Size::new(1920.0, 1080.0));
        assert_eq!(frame_px(Point::new(100.0, 200.0), 1.5, view), (150, 300));
        // an arrow step of one physical px at 150 % lands on the next pixel
        let next = Point::new(100.0 + 1.0 / 1.5, 200.0);
        assert_eq!(frame_px(next, 1.5, view), (151, 300));
    }

    #[test]
    fn each_tool_has_its_quick_action() {
        assert_eq!(for_action("colour_picker"), Some(Which::Picker));
        assert_eq!(for_action("ruler"), Some(Which::Ruler));
        assert_eq!(for_action("snip"), None);
        for id in ["colour_picker", "ruler"] {
            assert!(crate::hotkey::QuickId::from_name(id).is_known(), "{id}");
        }
    }

    #[test]
    fn the_copied_pill_goes_after_its_time_unless_a_newer_one_came() {
        let mut t = Tool::new(Which::Picker, None);
        let first = t.copied("Copied #1E90FF".into());
        let second = t.copied("Copied #FFFFFF".into());
        t.copied_gone(first);
        assert!(t.copied.is_some(), "the newer pill stays");
        t.copied_gone(second);
        assert!(t.copied.is_none());
    }
}
