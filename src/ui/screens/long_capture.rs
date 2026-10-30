//! Snip's long capture (N-73 point 5, design state 7): the picked area stays
//! outlined (dashed) and is cut out of the overlay, so the user scrolls the
//! app under it by hand; the toolbar shrinks to the counter, Cancel and
//! Done; a preview of the stitched image grows beside the area, pinned to
//! its newest rows; at the height limit the counter says so (7c). The
//! grabbing and stitching run on a thread (`capture::scroll::by_hand`).
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::{Space, canvas as draw, column, container, image, pin, row, stack};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::capture::scroll::Update;
use crate::capture::stitch::MAX_HEIGHT;
use crate::ui::kit::area;
use crate::ui::kit::button::{self, Kind};
use crate::ui::kit::chip;
use crate::ui::kit::divider;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::key_hint::{self, key_hint};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::text;
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::screens::bar::geometry;
use crate::ui::snip::Msg;

/// The toolbar while capturing (logical px): counter, divider, Cancel, Done.
pub const BAR: Size = Size::new(480.0, 46.0);
/// Cancel's and Done's widths.
const CANCEL_W: f32 = 92.0;
const DONE_W: f32 = 80.0;
const DIVIDER_H: f32 = 24.0;
/// Beside the area: the gap, the preview card's narrowest, the thin strip
/// taken when not even that fits, the card's padding and title row.
const GAP: f32 = 12.0;
const CARD_MIN: f32 = 240.0;
const STRIP: f32 = 120.0;
const PAD: f32 = 10.0;
const TITLE_H: f32 = 26.0;
/// The preview keeps 14 px text at 9 px or more (design 7b), and takes at
/// most this share of the screen's width.
const MIN_SCALE: f32 = 9.0 / 14.0;
const MAX_SHARE: f32 = 0.4;
/// What the hint says for its 2 s.
const HINT: [key_hint::Group<'static>; 3] = [
    (None, "Scroll the page yourself"),
    (Some("Enter"), "Done"),
    (Some("Esc"), "Cancel"),
];

/// The running capture (`Snip::scroll`).
pub struct Long {
    stop: Arc<AtomicBool>,
    /// Cancel was pressed: back to the picked area when the thread ends.
    pub cancelled: bool,
    /// The area, logical px on the overlay's screen.
    pub area: Rectangle,
    height: u32,
    full: bool,
    lost: bool,
    preview: Option<(image::Handle, Size)>,
}

impl Long {
    /// Capturing `area`; `stop` is shared with the grabbing thread.
    pub fn new(area: Rectangle, stop: Arc<AtomicBool>) -> Self {
        Long {
            stop,
            cancelled: false,
            area,
            height: 0,
            full: false,
            lost: false,
            preview: None,
        }
    }

    /// Ends the grabbing: the thread hands over the image.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn update(&mut self, u: Update) {
        (self.height, self.full, self.lost) = (u.height, u.full, u.lost);
        if let Some(p) = u.preview.filter(|p| p.width > 0 && p.height > 0) {
            let size = Size::new(p.width as f32, p.height as f32);
            self.preview = Some((image::Handle::from_rgba(p.width, p.height, p.rgba), size));
        }
    }

    /// The toolbar, the preview card and the hint (`hinting`) on a `screen`
    /// big overlay; `look` = the active look's pair, for the outline.
    pub fn view(&self, screen: Size, hinting: bool, look: (Color, Color)) -> Element<'_, Msg> {
        let at = layout(self.area, screen);
        let outline = Outline {
            area: self.area,
            look,
            solid: self.full,
        };
        let mut layers = stack![draw(outline).width(Fill).height(Fill)];
        let bar = toolbar(self.height, self.full, self.lost, false);
        layers = layers.push(pin(bar).position(at.bar.position()));
        if let Some(card) = at.card {
            layers = layers.push(pin(self.card(card)).position(card.position()));
        }
        // (a lost match shows in the counter: "Scroll back")
        if hinting && let Some(spot) = hint_at(at.bar, self.area, key_hint::width(&HINT), screen) {
            layers = layers.push(pin(key_hint(&HINT, false)).position(spot));
        }
        layers.into()
    }

    /// The preview card: title and count, then the newest rows at its width.
    fn card(&self, card: Rectangle) -> Element<'_, Msg> {
        let view = viewport(card);
        let count = chip::readout(px(self.height), false, None);
        let title = row![
            text(Role::CONTROL, "Long capture"),
            Space::new().width(Fill),
            count
        ]
        .align_y(iced::Center)
        .height(TITLE_H);
        let shown: Element<'_, Msg> = match &self.preview {
            // from the top while it fits, then pinned to the newest rows
            Some((handle, px)) => {
                let h = view.width * px.height / px.width;
                let picture = image(handle.clone()).width(view.width).height(h);
                let shown = container(picture).height(view.height).clip(true);
                match h > view.height {
                    true => shown.align_bottom(view.height).into(),
                    false => shown.into(),
                }
            }
            None => Space::new().height(view.height).into(),
        };
        surface(
            Material::Widget,
            radius::CARD,
            column![title, shown].spacing(space::GAP),
        )
        .padding(PAD)
        .width(card.width)
        .height(card.height)
        .into()
    }
}

/// The capture toolbar: dot, `3,240 px`, meter | Cancel, Done. `full`: the
/// limit (7c); `lost`: no match. `rtl`: mirrored.
pub fn toolbar<'a>(height: u32, full: bool, lost: bool, rtl: bool) -> Element<'a, Msg> {
    let ar = |latin: &'a str, arabic: &'a str| if rtl { arabic } else { latin };
    let warn = match () {
        _ if full => Some(ar("Limit reached", "بلغت الحد")),
        _ if lost => Some(ar("Scroll back", "مرّر للخلف")),
        _ => None,
    };
    let share = height as f32 / MAX_HEIGHT as f32;
    let label = match rtl {
        true => format!("{} بكسل", group(height)),
        false => px(height),
    };
    let cancel = button::tool(
        Icon::Close.glyph(),
        ar("Cancel", "إلغاء"),
        Kind::Elevated,
        CANCEL_W,
        false,
        Some(Msg::Cancel),
    );
    let done = button::tool(
        Icon::Check.glyph(),
        ar("Done", "تم"),
        Kind::Accent,
        DONE_W,
        false,
        Some(Msg::Stop),
    );
    let mut parts = vec![
        chip::counter(label, share, warn, rtl),
        Space::new().width(Fill).into(),
        divider::vertical(DIVIDER_H),
        cancel,
        done,
    ];
    if rtl {
        parts.reverse();
    }
    let line = iced::widget::Row::with_children(parts)
        .spacing(space::GAP)
        .padding([0.0, space::GAP])
        .align_y(iced::Center);
    surface(Material::Widget, radius::WINDOW, line)
        .padding(space::BAR_PAD)
        .width(BAR.width)
        .height(BAR.height)
        .into()
}

/// Where the toolbar and the preview card go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub bar: Rectangle,
    /// None: no room beside the area.
    pub card: Option<Rectangle>,
}

/// The toolbar docks as the picked one did (under, else over, else inside
/// the area); the card takes the wider free side, as tall as the area,
/// wide enough to read (`MIN_SCALE`) up to `MAX_SHARE` of the screen; a
/// thin strip when that side is narrow; none when there is no side.
pub fn layout(area: Rectangle, screen: Size) -> Layout {
    let bar = Rectangle::new(geometry::place(area, BAR, screen), BAR);
    let left = area.x - 2.0 * GAP;
    let right = screen.width - (area.x + area.width) - 2.0 * GAP;
    let side = left.max(right);
    let want = (area.width * MIN_SCALE).clamp(CARD_MIN, screen.width * MAX_SHARE);
    let width = match () {
        _ if side >= CARD_MIN => want.min(side),
        _ if side >= STRIP => STRIP,
        _ => return Layout { bar, card: None },
    };
    let top = area.y.max(GAP);
    let bottom = (area.y + area.height).min(screen.height - GAP);
    let height = (bottom - top).max(TITLE_H + 2.0 * PAD + 80.0);
    let top = top.min(screen.height - GAP - height).max(GAP);
    let x = match right >= left {
        true => area.x + area.width + GAP,
        false => area.x - GAP - width,
    };
    let card = Rectangle::new(Point::new(x, top), Size::new(width, height));
    Layout {
        bar,
        card: Some(card),
    }
}

/// The preview's own size inside `card` (logical px).
pub fn viewport(card: Rectangle) -> Size {
    Size::new(
        (card.width - 2.0 * PAD).max(1.0),
        (card.height - 2.0 * PAD - TITLE_H - space::GAP).max(1.0),
    )
}

/// The hint beside the toolbar, on its side away from the area, `width`
/// wide; none where it would cover the area or leave the screen.
fn hint_at(bar: Rectangle, area: Rectangle, width: f32, screen: Size) -> Option<Point> {
    let h = key_hint::HEIGHT;
    let y = match bar.y >= area.y + area.height {
        true => bar.y + bar.height + space::GAP,
        false => bar.y - space::GAP - h,
    };
    let x = (bar.center_x() - width / 2.0).clamp(GAP, (screen.width - GAP - width).max(GAP));
    let spot = Rectangle::new(Point::new(x, y), Size::new(width, h));
    let inside = y >= 0.0 && y + h <= screen.height;
    (inside && !spot.intersects(&area)).then_some(spot.position())
}

/// `3,240 px`.
fn px(height: u32) -> String {
    format!("{} px", group(height))
}

/// `3240` as `3,240`.
fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The area's outline: dashed while capturing, solid at the limit; drawn
/// outside the area (the live screen shows inside).
struct Outline {
    area: Rectangle,
    look: (Color, Color),
    solid: bool,
}

impl canvas::Program<Msg> for Outline {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        match self.solid {
            true => area::outline(&mut frame, self.area.expand(area::LINE), self.look),
            false => area::dashed(&mut frame, self.area, self.look),
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1280.0, 720.0);

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn the_card_takes_the_wider_side_and_never_covers_the_area() {
        // design 7b: area on the left, the card on its right
        let area = rect(80.0, 120.0, 560.0, 500.0);
        let at = layout(area, SCREEN);
        let card = at.card.unwrap();
        assert!(!card.intersects(&area) && card.x > area.x + area.width);
        assert_eq!(card.width, 360.0, "560 px at 9/14");
        assert_eq!((card.y, card.height), (120.0, 500.0));
        // area on the right: the card on its left
        let area = rect(700.0, 100.0, 500.0, 400.0);
        let card = layout(area, SCREEN).card.unwrap();
        assert!(card.x + card.width <= area.x);
        // little room: a strip; none: no card
        let area = rect(10.0, 50.0, 1120.0, 500.0);
        assert_eq!(layout(area, SCREEN).card.unwrap().width, STRIP);
        let area = rect(0.0, 0.0, 1280.0, 720.0);
        assert_eq!(layout(area, SCREEN).card, None);
        // the toolbar is on screen in every case
        let bar = layout(area, SCREEN).bar;
        assert!(bar.x >= 0.0 && bar.y + bar.height <= SCREEN.height);
    }

    #[test]
    fn the_hint_sits_beside_the_toolbar_away_from_the_area() {
        let area = rect(80.0, 120.0, 560.0, 400.0);
        let at = layout(area, SCREEN);
        let spot = hint_at(at.bar, area, 300.0, SCREEN).unwrap();
        assert!(spot.y > at.bar.y, "under the toolbar under the area");
        // a full-screen area: the toolbar is over it; no hint over the page
        let full = rect(0.0, 0.0, 1280.0, 720.0);
        let at = layout(full, SCREEN);
        assert_eq!(hint_at(at.bar, full, 300.0, SCREEN), None);
    }

    #[test]
    fn counts_read_with_thousands() {
        assert_eq!(px(0), "0 px");
        assert_eq!(px(3240), "3,240 px");
        assert_eq!(px(20_000), "20,000 px");
        assert_eq!(group(1_234_567), "1,234,567");
    }

    #[test]
    fn done_and_cancel_are_in_the_toolbar() {
        let texts = crate::ui::kit::testing::shot::texts(toolbar(3240, false, false, false));
        assert!(texts.iter().any(|t| t == "Done") && texts.iter().any(|t| t == "Cancel"));
        assert!(texts.iter().any(|t| t == "3,240 px"), "{texts:?}");
        let texts = crate::ui::kit::testing::shot::texts(toolbar(20_000, true, false, false));
        assert!(texts.iter().any(|t| t == "Limit reached"), "{texts:?}");
    }
}
