//! A window's own header (no OS title bar): the regular one, 40 px (Settings:
//! logo, title, a status slot, minimise / maximise / close), and the compact
//! one, 36 px (the setup: a mono title and close). `frame` puts a page under
//! the regular header with the resize strips along the edges.
use iced::font::Weight;
use iced::widget::{Space, button, column, container, image, mouse_area, row, stack, text};
use iced::window::Direction;
use iced::{Border, Center, Element, Fill, Length, mouse};

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
/// the caption buttons. `maximise`: `None` = a fixed-size window, no Maximise
/// button (Close takes its place); `Some(maximised now)` = Maximise / Restore.
/// The header's side padding and the gap after the logo.
const PAD: f32 = 14.0;
const LOGO_GAP: f32 = 10.0;

/// Where a menu bar's first opener starts, from the header's start edge (what
/// a menu hung under an opener aligns to).
pub const MENU_X: f32 = PAD + size::ICON + LOGO_GAP;

pub fn regular<'a, Message: Clone + 'a>(
    logo: image::Handle,
    title: &'a str,
    status: impl Into<Element<'a, Message>>,
    maximise: Option<bool>,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    bar(logo, None, title, false, None, status, maximise, on)
}

/// `regular` with `chip` (the version) after the title (Settings round 2).
pub fn regular_with_chip<'a, Message: Clone + 'a>(
    logo: image::Handle,
    title: &'a str,
    chip: Element<'a, Message>,
    status: impl Into<Element<'a, Message>>,
    maximise: Option<bool>,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    bar_sized(
        super::tokens::settings::HEADER,
        logo,
        None,
        title,
        false,
        Some(chip),
        status,
        maximise,
        on,
    )
}

/// `regular` with a menu bar (`menu`) between the logo and the title (Studio:
/// File / Edit / View); the title then is the document's name. Only the title
/// side drags the window.
pub fn with_menu<'a, Message: Clone + 'a>(
    logo: image::Handle,
    menu: Option<Element<'a, Message>>,
    title: &'a str,
    unsaved: bool,
    status: impl Into<Element<'a, Message>>,
    maximise: Option<bool>,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    bar(logo, menu, title, unsaved, None, status, maximise, on)
}

#[allow(clippy::too_many_arguments)]
fn bar<'a, Message: Clone + 'a>(
    logo: image::Handle,
    menu: Option<Element<'a, Message>>,
    title: &'a str,
    unsaved: bool,
    chip: Option<Element<'a, Message>>,
    status: impl Into<Element<'a, Message>>,
    maximise: Option<bool>,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    bar_sized(
        size::HEADER,
        logo,
        menu,
        title,
        unsaved,
        chip,
        status,
        maximise,
        on,
    )
}

#[allow(clippy::too_many_arguments)]
fn bar_sized<'a, Message: Clone + 'a>(
    h: f32,
    logo: image::Handle,
    menu: Option<Element<'a, Message>>,
    title: &'a str,
    unsaved: bool,
    chip: Option<Element<'a, Message>>,
    status: impl Into<Element<'a, Message>>,
    maximise: Option<bool>,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    const STATUS_GAP: f32 = 8.0;
    const DOT: f32 = 6.0;
    const GLYPH: f32 = 12.0;
    let mark = image(logo).width(size::ICON).height(size::ICON);
    let name_text = text(title)
        .size(12)
        .font(weight(Weight::Semibold))
        .color(c().text);
    let restore = maximise.map(|maximised| match maximised {
        true => Icon::Restore,
        false => Icon::Maximise,
    });
    let restore = restore.map(|glyph| caption(glyph, GLYPH, h, false, on(Caption::Maximise)));
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    let has_menu = menu.is_some();
    let name: Element<'a, Message> = match menu {
        None => row![mark, name_text]
            .push(chip)
            .spacing(LOGO_GAP)
            .align_y(Center)
            .into(),
        Some(menu) => {
            parts.push(
                // (the logo and the menus sit at the left in every language, in the English
                // order: a plain row, never mirrored; the menus' contents are the language's)
                container(row![mark, menu].spacing(LOGO_GAP).align_y(Center))
                    .padding([0.0, PAD])
                    .center_y(h)
                    .into(),
            );
            // the document's name, centred, with a dot while it has unsaved changes
            let dot = unsaved.then(|| {
                container(Space::new().width(DOT).height(DOT)).style(|_| container::Style {
                    background: Some(c().accent.into()),
                    border: Border::default().rounded(DOT / 2.0),
                    ..container::Style::default()
                })
            });
            crate::dir_row![name_text]
                .push(dot)
                .spacing(LOGO_GAP / 2.0)
                .align_y(Center)
                .into()
        }
    };
    let name = container(name)
        .padding([0.0, PAD])
        .center_y(h)
        .width(Fill)
        .align_x(match has_menu {
            true => iced::alignment::Horizontal::Center,
            false => iced::alignment::Horizontal::Left,
        });
    parts.extend([
        mouse_area(name).on_press(on(Caption::Drag)).into(), // mouse only: drags the window
        status.into(),
        Space::new().width(STATUS_GAP).into(),
    ]);
    let mut buttons = vec![caption(
        Icon::Minimise,
        GLYPH,
        h,
        false,
        on(Caption::Minimise),
    )];
    buttons.extend(restore);
    buttons.push(caption(Icon::Close, GLYPH, h, true, on(Caption::Close)));
    // (a plain row: a title bar is never mirrored, in any language: logo and title at the left,
    // the buttons at the right; only the text is the language's)
    let content: Element<'a, Message> = row(parts).width(Fill).height(h).align_y(Center).into();
    row![content, row(buttons)].height(h).align_y(Center).into()
}

/// `title` (mono, muted) that drags the window (`on_drag` on press), and the
/// close caption button.
pub fn compact<'a, Message: Clone + 'a>(
    title: &'a str,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message> {
    const H: f32 = size::HEADER_COMPACT;
    // (never mirrored: the title at the left, the close button at the right)
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
            .align_x(iced::alignment::Horizontal::Left)
        )
        .on_press(on_drag), // mouse only: drags the window
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
        .on_press(on_press) // mouse only: caption buttons are not in the Tab order (as native windows)
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
    let maximise = mode.resizable.then_some(mode.maximised);
    let header = regular(logo, title, Space::new(), maximise, move |c| to(c.into()));
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
            .on_press(on(Event::Resize(direction))) // mouse only: resizes the window
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

/// What a header keeps clear on each side of the window's middle for a centred part: the title and the
/// logo at the left, the caption buttons (and a status) at the right; the same both sides, so the part
/// is centred on the window itself.
pub const CENTRE_MARGIN: f32 = 200.0;

/// `header` with `centre` EXACTLY on the window's middle, as wide as `widest` and narrower when the window
/// is (never reaching the title or the buttons): a layer over the whole header that takes only the
/// presses of the part itself. `margin`: what it keeps clear on each side (at least `CENTRE_MARGIN`); the part
/// narrows (the window's smallest width leaves it 100 px) before it would reach anything.
pub fn with_centre<'a, Message: 'a>(
    header: Element<'a, Message>,
    centre: Element<'a, Message>,
    widest: f32,
    margin: f32,
) -> Element<'a, Message> {
    let part = container(centre).max_width(widest).width(Fill);
    let layer = container(part).padding([0.0, margin]).center(Fill);
    stack![header, layer]
        .height(super::tokens::settings::HEADER)
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_menu_bar_header_is_laid_out_the_same_in_arabic_as_in_english() {
        use crate::shared::kit::testing::shot;
        let size = Size::new(900.0, 40.0);
        let place = |ar: bool| -> Vec<(String, f32)> {
            let _dir = super::super::dir::scope(ar);
            let menu: Element<'static, ()> = row![
                iced::widget::text("File").width(60.0),
                iced::widget::text("Edit").width(60.0),
                iced::widget::text("View").width(60.0)
            ]
            .spacing(8.0)
            .into();
            let head = with_menu(
                super::super::logo::logo(16),
                Some(menu),
                "Title",
                false,
                Space::new(),
                Some(false),
                |_| (),
            );
            shot::texts_at(head, size)
                .into_iter()
                .map(|(r, t)| (t, r.x))
                .collect()
        };
        let (en, ar) = (place(false), place(true));
        // (the menus have fixed widths, so fonts don't matter; the title's own width differs a
        // little between the two languages' font settings on some systems: it is centred)
        let x = |v: &[(String, f32)], name: &str| v.iter().find(|(t, _)| t == name).unwrap().1;
        for name in ["File", "Edit", "View"] {
            assert_eq!(
                x(&en, name),
                x(&ar, name),
                "{name}: the same place in Arabic"
            );
        }
        assert!(
            (x(&en, "Title") - x(&ar, "Title")).abs() < 12.0,
            "the title stays centred"
        );
        for v in [&en, &ar] {
            assert!(x(v, "File") < x(v, "Edit") && x(v, "Edit") < x(v, "View"));
            assert!(x(v, "View") < x(v, "Title"));
        }
    }

    use super::*;
    use crate::shared::kit::testing::shot;
    use iced::Size;

    /// What one click at `at` reports, by name.
    fn pressing(at: (f32, f32), mode: Mode) -> Vec<String> {
        let page: Element<'_, Event> = text("page").into();
        let logo = crate::shared::kit::logo::logo(16);
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
    fn a_fixed_or_maximised_window_has_no_edges_and_a_fixed_one_no_maximise() {
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
        }
        // Fixed: close sits where maximise would be, one slot over.
        let fixed = Mode {
            resizable: false,
            maximised: false,
        };
        is(pressing((380.0, 20.0), fixed), &["Caption(Close)"]);
        is(pressing((330.0, 20.0), fixed), &["Caption(Minimise)"]);
    }
}
