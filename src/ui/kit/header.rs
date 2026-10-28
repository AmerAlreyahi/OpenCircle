//! A window's own header (no OS title bar): the regular one, 40 px (Settings:
//! logo, title, a status slot, minimise / maximise / close), and the compact
//! one, 36 px (the setup: a mono title and close). `frame` puts a page under
//! the regular header with the resize strips along the edges.
use iced::font::Weight;
use iced::widget::{Space, button, column, container, image, mouse_area, row, stack, text};
use iced::window::Direction;
use iced::{Center, Element, Fill, Length, mouse};

use super::icon::{Icon, icon};
use super::surface::{Material, surface};
use super::text::{mono, weight};
use super::themes::c;
use super::tokens::{fixed, size};

/// What a regular header reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caption {
    /// Pressed on the free area: the window follows the pointer (or, the second
    /// press of a double-click, maximises).
    Drag,
    Minimise,
    /// Maximise, or restore when maximised.
    Maximise,
    Close,
}

/// The Settings window's header (`h-10 pl-3.5`): the 16 px `logo` and `title`
/// (12 px / 600) drag the window; `status` (a header pill) sits 8 px left of
/// the caption buttons.
pub fn regular<'a, Message: Clone + 'a>(
    logo: image::Handle,
    title: &'a str,
    status: impl Into<Element<'a, Message>>,
    maximised: bool,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    const H: f32 = size::HEADER;
    const PAD: f32 = 14.0;
    const LOGO_GAP: f32 = 10.0;
    const STATUS_GAP: f32 = 8.0;
    const GLYPH: f32 = 12.0;
    let name = row![
        image(logo).width(size::ICON).height(size::ICON),
        text(title)
            .size(12)
            .font(weight(Weight::Semibold))
            .color(c().text),
    ]
    .spacing(LOGO_GAP)
    .align_y(Center);
    let restore = if maximised {
        Icon::Restore
    } else {
        Icon::Maximise
    };
    row![
        mouse_area(container(name).padding([0.0, PAD]).center_y(H).width(Fill))
            .on_press(on(Caption::Drag)),
        status.into(),
        Space::new().width(STATUS_GAP),
        caption(Icon::Minimise, GLYPH, H, false, on(Caption::Minimise)),
        caption(restore, GLYPH, H, false, on(Caption::Maximise)),
        caption(Icon::Close, GLYPH, H, true, on(Caption::Close)),
    ]
    .height(H)
    .align_y(Center)
    .into()
}

/// `title` (mono, muted) that drags the window (`on_drag` on press), and the
/// close caption button.
pub fn compact<'a, Message: Clone + 'a>(
    title: &'a str,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message> {
    const H: f32 = size::HEADER_COMPACT;
    row![
        mouse_area(
            container(
                text(title)
                    .font(mono(Weight::Medium))
                    .size(11)
                    .color(c().text_muted)
            )
            .padding([0, 14])
            .center_y(H)
            .width(Fill)
        )
        .on_press(on_drag),
        caption(Icon::Close, 14.0, H, true, on_close),
    ]
    .height(H)
    .into()
}

/// `.win-caption-btn`: 46 px wide, `height` tall, the glyph in `text_sub`;
/// under the cursor `text` on `hover`, the close one red with a white glyph.
fn caption<'a, Message: Clone + 'a>(
    glyph: Icon,
    px: f32,
    height: f32,
    close: bool,
    on_press: Message,
) -> Element<'a, Message> {
    button(container(icon(glyph, px)).center(Fill))
        .width(size::CAPTION_BUTTON)
        .height(height)
        .padding(0)
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (background, text_color) = match (hot, close) {
                (true, true) => (Some(fixed::CLOSE_HOVER.into()), fixed::ON_CLOSE),
                (true, false) => (Some(c().hover.into()), c().text),
                (false, _) => (None, c().text_sub),
            };
            button::Style {
                background,
                text_color,
                ..button::Style::default()
            }
        })
        .into()
}

/// The resize strip along each edge of a normal window (logical px).
pub const EDGE: f32 = 6.0;

/// What a window's frame reports: its header's captions, and a press on an
/// edge or corner (the OS resizes from that side).
#[derive(Debug, Clone, Copy)]
pub enum Event {
    Caption(Caption),
    Resize(Direction),
}

impl From<Caption> for Event {
    fn from(caption: Caption) -> Event {
        Event::Caption(caption)
    }
}

/// What the window can do now: a fixed-size or maximised one has no edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub resizable: bool,
    pub maximised: bool,
}

/// `content` under the regular header (`logo`, `title`, no status), on the
/// window's surface, with the resize edges on top.
pub fn frame<'a, Message: Clone + 'a>(
    logo: image::Handle,
    title: &'a str,
    mode: Mode,
    content: impl Into<Element<'a, Message>>,
    on: impl Fn(Event) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let to = on.clone();
    let header = regular(logo, title, Space::new(), mode.maximised, move |c| {
        to(c.into())
    });
    let page = column![header, container(content).width(Fill).height(Fill)];
    let window = surface(Material::Window, 0.0, page)
        .width(Fill)
        .height(Fill)
        .padding(1);
    with_edges(window, mode, on)
}

/// `window` with the resize edges on top (only when it can resize now).
pub fn with_edges<'a, Message: Clone + 'a>(
    window: impl Into<Element<'a, Message>>,
    mode: Mode,
    on: impl Fn(Event) -> Message,
) -> Element<'a, Message> {
    if mode.resizable && !mode.maximised {
        stack![window.into(), edges(&on)].into()
    } else {
        window.into()
    }
}

/// The invisible strips along the edges and corners.
fn edges<'a, Message: Clone + 'a>(on: &impl Fn(Event) -> Message) -> Element<'a, Message> {
    use mouse::Interaction::{
        ResizingDiagonallyDown as Down, ResizingDiagonallyUp as Up,
        ResizingHorizontally as Horizontal, ResizingVertically as Vertical,
    };
    let zone = |direction, interaction, width: Length, height: Length| {
        mouse_area(Space::new().width(width).height(height))
            .on_press(on(Event::Resize(direction)))
            .interaction(interaction)
    };
    let corner = |direction, interaction| zone(direction, interaction, EDGE.into(), EDGE.into());
    let top = row![
        corner(Direction::NorthWest, Down),
        zone(Direction::North, Vertical, Fill, EDGE.into()),
        corner(Direction::NorthEast, Up),
    ];
    let middle = row![
        zone(Direction::West, Horizontal, EDGE.into(), Fill),
        Space::new().width(Fill),
        zone(Direction::East, Horizontal, EDGE.into(), Fill),
    ]
    .height(Fill);
    let bottom = row![
        corner(Direction::SouthWest, Up),
        zone(Direction::South, Vertical, Fill, EDGE.into()),
        corner(Direction::SouthEast, Down),
    ];
    column![top, middle, bottom].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::shot;
    use iced::Size;

    /// What one click at `at` reports, by name.
    fn pressing(at: (f32, f32), mode: Mode) -> Vec<String> {
        let page: Element<'_, Event> = text("page").into();
        let logo = crate::ui::kit::logo::logo(16);
        let view = frame(logo, "Settings", mode, page, |e| e);
        let size = Size::new(400.0, 300.0);
        let events = shot::click(view, size, iced::Point::new(at.0, at.1));
        events.iter().map(|e| format!("{e:?}")).collect()
    }

    fn is(got: Vec<String>, want: &[&str]) {
        assert_eq!(got, want);
    }

    const FREE: Mode = Mode {
        resizable: true,
        maximised: false,
    };

    #[test]
    fn header_hit_zones_drag_buttons_and_edges() {
        // Free header area: drag; the three caption buttons at the right end.
        is(pressing((200.0, 20.0), FREE), &["Caption(Drag)"]);
        is(pressing((284.0, 20.0), FREE), &["Caption(Minimise)"]);
        is(pressing((330.0, 20.0), FREE), &["Caption(Maximise)"]);
        is(pressing((380.0, 20.0), FREE), &["Caption(Close)"]);
        // The page below does nothing.
        is(pressing((200.0, 150.0), FREE), &[]);
        // Edges and corners resize from their side.
        let edge = |x, y| pressing((x, y), FREE);
        is(edge(2.0, 150.0), &["Resize(West)"]);
        is(edge(397.0, 150.0), &["Resize(East)"]);
        is(edge(200.0, 297.0), &["Resize(South)"]);
        is(edge(200.0, 2.0), &["Resize(North)"]);
        is(edge(2.0, 2.0), &["Resize(NorthWest)"]);
        is(edge(397.0, 2.0), &["Resize(NorthEast)"]);
        is(edge(2.0, 297.0), &["Resize(SouthWest)"]);
        is(edge(397.0, 297.0), &["Resize(SouthEast)"]);
    }

    #[test]
    fn a_fixed_or_maximised_window_has_no_edges() {
        for mode in [
            Mode {
                resizable: true,
                maximised: true,
            },
            Mode {
                resizable: false,
                maximised: false,
            },
        ] {
            is(pressing((2.0, 150.0), mode), &[]);
            is(pressing((200.0, 297.0), mode), &[]);
            is(pressing((200.0, 20.0), mode), &["Caption(Drag)"]);
            is(pressing((380.0, 20.0), mode), &["Caption(Close)"]);
        }
    }
}
