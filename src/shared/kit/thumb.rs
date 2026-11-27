//! The Steps strip's parts (STEPS_BOARD_SPEC, owner approved 2026-11-02):
//! `step(picture, number, look, on_delete)` = a numbered thumbnail: a 96x60
//! `track` well, radius 6, the capture fitted inside (never cropped or
//! upscaled), an 18 px `accent` badge with a 2 px `widget` ring 3 px in from
//! the top-start corner; under the cursor, lifted or focused a 1 px `accent`
//! ring (focused: an `accent_soft` well), and under the cursor the delete x
//! at the top-end corner.
//! `close(on_press)` = the strip's and a step's small x (20 px, `muted`;
//! `hover` fill under the cursor), one look for both.
use iced::widget::{button, container, image, pin, stack, text};
use iced::{Border, Center, ContentFit, Element, Fill, Size};

use super::icon::{FONT, Icon};
use super::text::font;
use super::themes::c;
use super::tokens::{Role, radius, widget_e};

/// A thumbnail's well (Studio walkthrough & legacy kit sheet).
pub const WELL: Size = Size::new(96.0, 60.0);
/// Style E Steps strip thumbnail well (`.stp`: 96x64).
pub const STRIP_WELL: Size = Size::new(widget_e::STRIP_THUMB_W, widget_e::STRIP_THUMB_H);
/// The grid cell's well and its badge.
const CELL_BADGE: f32 = 16.0;
/// The badge's side.
pub const BADGE: f32 = 18.0;
/// The Steps strip's badge (owner 2026-11-27: bigger than the design's 18 px, and "suitable based on
/// screen size, on my 5K monitor it looks small"): 20 logical px on a 1080-physical-px-tall screen,
/// growing with the monitor's PHYSICAL height (`physical_height` px, shown at `scale`) up to twice, so
/// a 5K screen at any scale shows the same share of the screen; never under 20 logical px.
pub fn strip_badge(physical_height: f32, scale: f32) -> f32 {
    (20.0 * (physical_height / 1080.0).clamp(1.0, 2.0) / scale.max(1.0))
        .max(20.0)
        .round()
}
/// The small x's side.
pub const X: f32 = 20.0;
/// The well's corners.
const CORNER: f32 = 6.0;
/// The badge's and the x's distance from the well's corner.
const INSET: f32 = 3.0;
/// The badge's ring.
const RING: f32 = 2.0;
/// The badge's number.
const NUMBER: f32 = 9.4;

/// How a thumbnail shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    /// Under the cursor: the ring and the delete x.
    pub hover: bool,
    /// Being dragged to another place: the ring.
    pub lifted: bool,
    /// The keyboard is on it: the ring on an `accent_soft` well.
    pub focused: bool,
    /// Right to left: the badge at the top-right, the x at the top-left.
    pub rtl: bool,
}

pub fn step<'a, Message: Clone + 'a>(
    picture: image::Handle,
    number: usize,
    look: Look,
    on_delete: Option<Message>,
) -> Element<'a, Message> {
    build(picture, number, look, on_delete, Frame::legacy(WELL, BADGE))
}

/// Style E Steps strip thumbnail (`.stp`): `picture` is already `strip_picture` (rounded once,
/// when the step is made, so a redraw never copies pixels); 96x64, `radius::TRACK` (10 px)
/// rounded picture filling the well, 1 px `rim` edge (2 px `accent` focused,
/// 1 px `accent` hovered/lifted), 18 px accent badge with 2 px `widget` ring
/// at top-start (`inset-inline-start: 5px`), hover delete x at top-end.
pub fn strip_step<'a, Message: Clone + 'a>(
    picture: image::Handle,
    number: usize,
    look: Look,
    on_delete: Option<Message>,
    badge: f32,
) -> Element<'a, Message> {
    build(
        picture,
        number,
        look,
        on_delete,
        Frame {
            well: STRIP_WELL,
            badge,
            corner: radius::TRACK,
            inset: widget_e::STRIP_BADGE_INSET,
            style_e: true,
        },
    )
}

/// A step's cell `size` wide (the Steps sidebar's: four to a row), with a 16 px badge.
pub fn cell<'a, Message: Clone + 'a>(
    picture: image::Handle,
    number: usize,
    look: Look,
    on_delete: Option<Message>,
    size: Size,
) -> Element<'a, Message> {
    build(
        picture,
        number,
        look,
        on_delete,
        Frame::legacy(size, CELL_BADGE),
    )
}

/// A step's picture for the strip: cropped to the well and corner-masked, once.
pub fn strip_picture(handle: &image::Handle) -> image::Handle {
    round_handle(handle, STRIP_WELL, radius::TRACK, true)
}

/// Returns a copy of an `Rgba` handle with its four corners softly alpha-masked
/// to `corner` logical px inside a `box_size` frame (the CPU renderer ignores
/// `image::Image::border_radius`). When `cover` is true, centre-crops to
/// `box_size`'s aspect ratio first so thumbnails fill their rounded frame evenly.
pub fn round_handle(
    handle: &image::Handle,
    box_size: Size,
    corner: f32,
    cover: bool,
) -> image::Handle {
    if corner <= 0.0 || box_size.width <= 0.0 || box_size.height <= 0.0 {
        return handle.clone();
    }
    let image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = handle
    else {
        return handle.clone();
    };
    let (w, h) = (*width, *height);
    if w == 0 || h == 0 || pixels.len() < (w as usize * h as usize * 4) {
        return handle.clone();
    }
    let (sx, sy, sw, sh) = if cover {
        let target = box_size.width / box_size.height;
        let current = w as f32 / h as f32;
        if current > target {
            let cw = ((h as f32 * target).round() as u32).clamp(1, w);
            ((w - cw) / 2, 0, cw, h)
        } else if current < target {
            let ch = ((w as f32 / target).round() as u32).clamp(1, h);
            (0, (h - ch) / 2, w, ch)
        } else {
            (0, 0, w, h)
        }
    } else {
        (0, 0, w, h)
    };
    let mut out = Vec::with_capacity(sw as usize * sh as usize * 4);
    for y in sy..sy + sh {
        let row = ((y * w + sx) * 4) as usize;
        out.extend_from_slice(&pixels[row..row + sw as usize * 4]);
    }
    let rx = (corner * sw as f32 / box_size.width).min(sw as f32 * 0.5);
    let ry = (corner * sh as f32 / box_size.height).min(sh as f32 * 0.5);
    if rx <= 0.0 || ry <= 0.0 {
        return image::Handle::from_rgba(sw, sh, out);
    }
    let rx_i = (rx.ceil() as u32).min(sw);
    let ry_i = (ry.ceil() as u32).min(sh);
    let r_min = rx.min(ry);
    let mask = |buf: &mut [u8], x: u32, y: u32, dx_px: f32, dy_px: f32| {
        if dx_px <= 0.0 || dy_px <= 0.0 {
            return;
        }
        let dist = ((dx_px / rx).hypot(dy_px / ry) - 1.0) * r_min;
        let cov = (0.5 - dist).clamp(0.0, 1.0);
        if cov < 1.0 {
            let i = ((y * sw + x) * 4) as usize + 3;
            buf[i] = (buf[i] as f32 * cov).round() as u8;
        }
    };
    for y in 0..ry_i {
        let py = y as f32 + 0.5;
        let by = sh - 1 - y;
        let dy = ry - py;
        for x in 0..rx_i {
            let px = x as f32 + 0.5;
            let bx = sw - 1 - x;
            let dx = rx - px;
            mask(&mut out, x, y, dx, dy);
            mask(&mut out, bx, y, dx, dy);
            mask(&mut out, x, by, dx, dy);
            mask(&mut out, bx, by, dx, dy);
        }
    }
    image::Handle::from_rgba(sw, sh, out)
}

#[derive(Clone, Copy)]
struct Frame {
    well: Size,
    badge: f32,
    corner: f32,
    inset: f32,
    style_e: bool,
}

impl Frame {
    const fn legacy(well: Size, badge: f32) -> Self {
        Self {
            well,
            badge,
            corner: CORNER,
            inset: INSET,
            style_e: false,
        }
    }
}

/// The place of a step that is being made: an empty well, `size` wide, with its number and a
/// quiet hairline edge; it becomes `cell` when the step arrives.
pub fn pending<'a, Message: 'a>(number: usize, size: Size) -> Element<'a, Message> {
    container(
        text(number.to_string())
            .size(NUMBER)
            .font(font(Role::BADGE))
            .color(c().text_muted),
    )
    .center(Fill)
    .width(size.width)
    .height(size.height)
    .style(move |_| container::Style {
        background: Some(c().track.into()),
        border: Border::default()
            .rounded(CORNER)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    })
    .into()
}

/// The place of a step that could not be made (F12): the empty well with a red edge
/// and a "!" mark; it stays until the user acts on it. (A solid edge: the renderer
/// draws no dashes.)
pub fn failed<'a, Message: 'a>(size: Size) -> Element<'a, Message> {
    container(
        text(Icon::Alert.glyph())
            .font(FONT)
            .size(NUMBER)
            .color(super::tokens::fixed::DANGER),
    )
    .center(Fill)
    .width(size.width)
    .height(size.height)
    .style(move |_| container::Style {
        background: Some(c().track.into()),
        border: Border::default()
            .rounded(CORNER)
            .width(1)
            .color(super::tokens::fixed::DANGER),
        ..container::Style::default()
    })
    .into()
}

fn build<'a, Message: Clone + 'a>(
    picture: image::Handle,
    number: usize,
    look: Look,
    on_delete: Option<Message>,
    frame: Frame,
) -> Element<'a, Message> {
    let well_size = frame.well;
    let ring = look.hover || look.lifted || look.focused;
    let base: Element<'a, Message> = if frame.style_e {
        let photo = image(picture)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill)
            .border_radius(frame.corner);
        let edge = container(iced::widget::Space::new())
            .width(well_size.width)
            .height(well_size.height)
            .style(move |_| container::Style {
                background: None,
                border: Border::default()
                    .rounded(frame.corner)
                    .width(if look.focused { 2 } else { 1 })
                    .color(if ring { c().accent } else { c().rim }),
                ..container::Style::default()
            });
        stack![
            container(photo)
                .width(well_size.width)
                .height(well_size.height),
            edge,
        ]
        .width(well_size.width)
        .height(well_size.height)
        .into()
    } else {
        container(image(picture).content_fit(ContentFit::ScaleDown))
            .center(Fill)
            .width(well_size.width)
            .height(well_size.height)
            .style(move |_| container::Style {
                background: Some(
                    match look.focused {
                        true => c().accent_soft,
                        false => c().track,
                    }
                    .into(),
                ),
                border: Border::default()
                    .rounded(frame.corner)
                    .width(if ring { 1 } else { 0 })
                    .color(c().accent),
                ..container::Style::default()
            })
            .into()
    };
    let badge_outer = if frame.style_e {
        frame.badge + 2.0 * RING
    } else {
        frame.badge
    };
    let badge_inset = if frame.style_e {
        frame.inset - RING
    } else {
        frame.inset
    };
    let number_size = match frame.style_e {
        true => frame.badge * 10.5 / 18.0,
        false => NUMBER,
    };
    let badge = container(
        text(number.to_string())
            .size(number_size)
            .font(font(Role::BADGE))
            .color(super::themes::on(c().accent)),
    )
    .center(badge_outer)
    .style(move |_| container::Style {
        background: Some(c().accent.into()),
        border: Border::default()
            .rounded(badge_outer / 2.0)
            .width(RING)
            .color(c().widget),
        ..container::Style::default()
    });
    let at_start = |inset: f32, side: f32| match look.rtl {
        true => well_size.width - inset - side,
        false => inset,
    };
    let at_end = |inset: f32, side: f32| match look.rtl {
        true => inset,
        false => well_size.width - inset - side,
    };
    let mut layers = stack![
        base,
        pin(badge)
            .x(at_start(badge_inset, badge_outer))
            .y(badge_inset)
    ];
    if let Some(delete) = on_delete.filter(|_| look.hover) {
        layers = layers.push(pin(close(delete)).x(at_end(frame.inset, X)).y(frame.inset));
    }
    layers
        .width(well_size.width)
        .height(well_size.height)
        .into()
}

/// The small x: a 20 px circle, a 12 px x in `text_muted` (`text` on `hover`
/// under the cursor), on the widget surface with a `rim` edge (it reads over
/// a thumbnail too).
pub fn close<'a, Message: Clone + 'a>(on_press: Message) -> Element<'a, Message> {
    round(Icon::Close.glyph(), Some(on_press))
}

fn round<'a, Message: Clone + 'a>(glyph: char, on_press: Option<Message>) -> Element<'a, Message> {
    use button::Status;
    let live = on_press.is_some();
    super::focus::press(
        button(
            container(text(glyph).font(FONT).size(12))
                .center(Fill)
                .align_y(Center),
        )
        .width(X)
        .height(X)
        .padding(0)
        .style(move |_, status| {
            let hot = matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: Some(if hot { c().hover } else { c().widget }.into()),
                text_color: match (live, hot) {
                    (false, _) => c().text_muted.scale_alpha(0.4),
                    (_, true) => c().text,
                    _ => c().text_muted,
                },
                border: Border::default()
                    .rounded(radius::FULL)
                    .width(1)
                    .color(c().rim),
                ..button::Style::default()
            }
        }),
        radius::FULL,
        on_press,
    )
    .into()
}

#[cfg(test)]
mod badge_tests {
    use super::*;

    /// Owner 2026-11-27: the Steps strip's number badge is bigger and grows with the screen's
    /// physical size (a 5K monitor at 1x, 1.5x or 2x scale).
    #[test]
    fn the_strip_badge_grows_with_the_monitors_physical_size() {
        assert_eq!(strip_badge(1080.0, 1.0), 20.0);
        assert_eq!(strip_badge(720.0, 1.0), 20.0); // never smaller
        assert_eq!(strip_badge(1080.0, 2.0), 20.0); // never under 20 logical px
        assert_eq!(strip_badge(2880.0, 1.0), 40.0); // 5K at 100 %: twice, no more
        assert_eq!(strip_badge(2880.0, 2.0), 20.0); // 5K at 200 %: 40 physical px
        assert_eq!(strip_badge(2880.0, 1.5), 27.0); // 5K at 150 %: 40 physical px
        assert!(strip_badge(1440.0, 1.0) > 25.0 && strip_badge(1440.0, 1.0) < 30.0);
        assert!(strip_badge(2880.0, 1.0) > BADGE);
    }
}
