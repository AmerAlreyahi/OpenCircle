//! Shared floating components. Every floating widget is built here, styled only
//! from `theme.rs`; screens never style a container or button themselves.
//!
//! Glass later: blur the frozen frame once per overlay and paint the blurred crop
//! behind each surface (no nested/live blur).
use iced::widget::text::IntoFragment;
use iced::widget::{
    Button, Container, Text, Tooltip, button, container, row, space, text, tooltip,
};
use iced::{Background, Border, Color, Element, gradient};

use super::theme;
use crate::config::BarStyle;

/// The fonts, the icon font and the UI font live in the kit.
pub use super::kit::icon::FONT as ICON_FONT;
#[cfg(test)]
pub use super::kit::icon::FONT_BYTES as ICON_FONT_BYTES;
pub use super::kit::text::{FONTS, ui_font};

/// How wide `text` is on one line at `size` in the UI font.
pub fn text_width(text: &str, size: f32) -> f32 {
    super::kit::text::width(text, size, ui_font())
}

/// The first of `texts` (longest first) that fits `room` px on one line at
/// `size`, else the last: a placeholder is never cut off, in any font.
pub fn fitting<'a>(texts: &[&'a str], size: f32, room: f32) -> &'a str {
    let last = texts.len() - 1;
    (texts.iter().position(|t| text_width(t, size) <= room)).map_or(texts[last], |i| texts[i])
}

/// A line of text is this many times its font size tall wherever its box
/// clips it (text fields): Arabic with every vowel mark stays inside
/// (Noto Sans Arabic inks about 1.6 times its size).
#[cfg(test)]
pub const LINE: f32 = 1.7;
/// Glyphs of the icon font used outside the action registry.
pub const STOP: char = '\u{e16a}'; // lucide "square"
pub const CLOSE: char = super::kit::icon::Icon::Close.glyph();
const MINUS: char = '\u{e11f}'; // lucide "minus" (window header: minimise)
const RESTORE: char = '\u{e0a2}'; // lucide "copy" (two squares: restore)
pub const DRAG: char = '\u{e0ee}'; // lucide "grip-vertical" (drag-out handle)

/// Every button is this tall (DESIGN.md: 44 px controls).
pub const CONTROL: f32 = 44.0;
/// Text buttons share one width (consistency rule), so bars have a known size.
pub const TEXT_BUTTON_WIDTH: f32 = 136.0;
/// Corner radius of every surface (glass, solid, docked) and so of a floating
/// window that holds one.
pub const SURFACE_RADIUS: f32 = 20.0;
/// A pill's corner radius: more than any pill is tall, so its ends are half circles.
pub const PILL_RADIUS: f32 = 999.0;
/// Inner padding of a `surface`.
pub const SURFACE_PADDING: f32 = 6.0;

/// Fully rounded glass pill with one line of text (overlay hint, tooltips).
pub fn pill<'a, Message: 'a>(label: impl IntoFragment<'a>) -> Container<'a, Message> {
    let palette = theme::floating();
    container(text(label).size(14).color(palette.text))
        .padding([8, 18])
        .style(|_| glass(999.0))
}

/// Glass panel (20 px radius): action bar, toolbars, popovers, side panels.
pub fn surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content)
        .padding(SURFACE_PADDING)
        .style(|_| glass(20.0))
}

/// A `surface` for a colour-keyed window (the live controller): opaque and
/// without a shadow.
pub fn solid_surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    let palette = theme::floating();
    container(content)
        .padding(SURFACE_PADDING)
        .style(move |_| container::Style {
            background: Some(palette.window.into()),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: SURFACE_RADIUS.into(),
            },
            ..container::Style::default()
        })
}

/// A round accent button with nothing on it (a minimised control).
pub fn accent_dot<'a, Message: Clone + 'a>(size: f32, on_press: Message) -> Button<'a, Message> {
    button(space::horizontal())
        .width(size)
        .height(size)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| button::Style {
            background: Some(accent(status)),
            border: Border::default().rounded(super::shape::radius(PILL_RADIUS)),
            ..button::Style::default()
        })
}

/// The thin frame around a pinned image (a pin window has no title bar).
pub fn pinned<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .width(iced::Fill)
        .height(iced::Fill)
        .padding(1)
        .style(|_| container::Style {
            background: Some(theme::floating().border.into()),
            ..container::Style::default()
        })
}

/// Icon + label + key hint. `primary` = accent gradient (the default action),
/// otherwise quiet (transparent until hovered).
pub fn text_button<'a, Message: Clone + 'a>(
    icon: char,
    label: &'a str,
    key_hint: String,
    primary: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::floating();
    let (fg, hint) = if primary {
        (theme::ON_ACCENT, theme::ON_ACCENT)
    } else {
        (palette.text, palette.muted)
    };
    let content = row![
        self::icon(icon).color(fg),
        text(label).size(14).color(fg),
        space::horizontal(),
        text(key_hint).size(11).color(hint),
    ]
    .spacing(8)
    .height(iced::Fill)
    .align_y(iced::Center);
    button(content)
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .padding([0, 12])
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: quiet_or_accent(primary, status),
            text_color: fg,
            border: Border::default().rounded(14),
            ..button::Style::default()
        })
}

/// A glyph from the icon font.
pub fn icon<'a>(glyph: char) -> Text<'a> {
    text(glyph)
        .font(ICON_FONT)
        .size(16)
        .color(theme::floating().text)
}

/// Running text: answers, messages.
pub fn body<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(14).color(theme::floating().text)
}

/// Secondary small text: notes, model name, progress.
pub fn caption<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(12).color(theme::floating().muted)
}

/// A pill button floating over the frozen frame ("Select text", "More"): the
/// glass of `pill`, `TEXT_BUTTON_WIDTH` wide, `CONTROL` tall.
pub fn float_chip<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Message,
) -> Container<'a, Message> {
    let palette = theme::floating();
    // the pill's own glass, the button on it only lights up under the pointer
    let button = button(text(label).size(14).color(palette.text).center())
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(palette.hover.into()),
                _ => None,
            },
            text_color: palette.text,
            border: Border::default().rounded(super::shape::radius(PILL_RADIUS)),
            ..button::Style::default()
        });
    container(button).style(|_| glass(999.0))
}

/// Button background: accent gradient for `primary`, else clear until hovered.
fn quiet_or_accent(primary: bool, status: button::Status) -> Option<Background> {
    let palette = theme::floating();
    match (primary, status) {
        (true, _) => Some(accent(status)),
        (false, button::Status::Hovered) => Some(palette.hover.into()),
        (false, button::Status::Pressed) => Some(palette.pressed.into()),
        (false, _) => None,
    }
}

/// Shows `tip` in a pill beside `content` on hover. Outside this file use `hint`
/// (it keeps the policy and the test registry), not this.
fn with_tip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl IntoFragment<'a>,
    side: tooltip::Position,
) -> Tooltip<'a, Message> {
    tooltip(content, pill(tip), side).gap(10)
}

/// Why a control shows a visible tooltip at all. A tooltip that only repeats
/// what the control already says is noise, so these are the only three reasons.
#[derive(Debug, Clone, Copy)]
pub enum Why<'a> {
    /// Nothing but a glyph on it: the label is its only name.
    Icon,
    /// The shortcut is not written on the control: "Label · Ctrl+C".
    Keys(&'a str),
    /// Its text is cut short: the tooltip is the whole text.
    Cut(&'a str),
}

/// A control's accessible name (screen readers, automation, the unique-label
/// test) with a visible tooltip for one of the `Why` reasons.
pub fn hint<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    why: Why<'a>,
    side: tooltip::Position,
) -> Element<'a, Message> {
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut content = content.into();
    let shown = match why {
        Why::Icon => label.to_string(),
        Why::Keys(keys) => format!("{label} · {keys}"),
        Why::Cut(full) => full.to_string(),
    };
    #[cfg(test)]
    tests_support::record_shown(&mut content, label, why, &shown);
    with_tip(content, shown, side).into()
}

/// Square 44x44 icon button (toolbar tools, back, undo/redo): quiet, hover,
/// `active` = accent gradient.
pub fn icon_button<'a, Message: Clone + 'a>(
    glyph: char,
    active: bool,
    on_press: Message,
) -> Button<'a, Message> {
    icon_button_maybe(glyph, active, Some(on_press))
}

/// `icon_button`; `None` = dimmed and dead, same size and place.
pub fn icon_button_maybe<'a, Message: Clone + 'a>(
    glyph: char,
    active: bool,
    on_press: Option<Message>,
) -> Button<'a, Message> {
    let fg = match (active, on_press.is_some()) {
        (true, _) => theme::ON_ACCENT,
        (false, true) => theme::floating().text,
        (false, false) => theme::floating().muted.scale_alpha(0.6),
    };
    square_button(
        text(glyph).font(ICON_FONT).size(18).color(fg),
        active,
        on_press,
    )
}

/// A 44x44 handle that starts a drag when the button goes down (an icon
/// button reacts on release, too late for an OS drag): quiet, grab cursor.
pub fn drag_handle<'a, Message: Clone + 'a>(glyph: char, on_down: Message) -> Element<'a, Message> {
    let fg = theme::floating().text;
    let look = square_button(text(glyph).font(ICON_FONT).size(18).color(fg), false, None);
    iced::widget::mouse_area(look)
        .on_press(on_down)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// A small picture as a 44x44 quiet button (Snip's Recent strip).
pub fn thumbnail<'a, Message: Clone + 'a>(
    picture: iced::widget::image::Handle,
    on_press: Message,
) -> Button<'a, Message> {
    let shown = iced::widget::image(picture)
        .width(CONTROL - 8.0)
        .height(CONTROL - 8.0)
        .content_fit(iced::ContentFit::Contain);
    square_button(shown, false, Some(on_press))
}

fn square_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    active: bool,
    on_press: Option<Message>,
) -> Button<'a, Message> {
    let palette = theme::floating();
    button(container(content).center(iced::Fill))
        .width(CONTROL)
        .height(CONTROL)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let background = match (active, status) {
                (true, _) => Some(accent(status)),
                (false, button::Status::Hovered) => Some(palette.hover.into()),
                (false, button::Status::Pressed) => Some(palette.pressed.into()),
                (false, _) => None,
            };
            button::Style {
                background,
                text_color: palette.text,
                border: Border::default().rounded(14),
                ..button::Style::default()
            }
        })
}

/// The accent gradient; hover/press brighten it a little.
fn accent(status: button::Status) -> Background {
    let lift = match status {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.14,
        _ => 0.0,
    };
    let (start, end) = theme::accent();
    let mix = |c: Color| Color {
        r: c.r + (1.0 - c.r) * lift,
        g: c.g + (1.0 - c.g) * lift,
        b: c.b + (1.0 - c.b) * lift,
        a: c.a,
    };
    gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, mix(start))
        .add_stop(1.0, mix(end))
        .into()
}

/// The one glass style: panel colour, 1 px border; the active look's bar
/// style makes it solid or a see-through outline. Never an `iced::Shadow`
/// anywhere: tiny-skia draws a shadow whole on every redraw, outside the part
/// being redrawn too, so it piles up into a black block (and a floating
/// window can't show one: no alpha there).
fn glass(radius: f32) -> container::Style {
    let palette = theme::floating();
    let (background, border, width) = match crate::config::look().bar {
        BarStyle::Outline => (palette.panel, theme::accent().0, 1.5),
        _ => (palette.panel, palette.border, 1.0),
    };
    container::Style {
        background: Some(background.into()),
        border: Border {
            color: border,
            width,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

mod forms {
    use iced::Element;
    use iced::widget::tooltip;

    /// A control's accessible name (iced has no a11y tree): what automation calls it,
    /// and what the test registry checks is unique per screen. It shows NO tooltip,
    /// since the control's own text says it; `hint` is for icon-only, cut or
    /// shortcut-carrying controls. `side` stays so a call can become a `hint` in place.
    pub fn tip<'a, Message: 'a>(
        content: impl Into<Element<'a, Message>>,
        label: &'a str,
        _side: tooltip::Position,
    ) -> Element<'a, Message> {
        #[cfg(test)]
        super::TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        #[cfg(not(test))]
        let _ = label;
        content.into()
    }
}
#[allow(unused_imports)]
pub use forms::*;

pub mod chrome {
    //! The header and frame of our normal windows (Settings, Onboarding, the
    //! permission prompt): the OS title bar is off, so the window draws its own, the
    //! same everywhere. App icon and title, drag anywhere on it, double-click to
    //! maximise, minimise / maximise / close; a thin strip along every edge resizes.
    use std::sync::LazyLock;

    use iced::widget::{Space, button, column, container, image, mouse_area, row, stack, text};
    use iced::window::Direction;
    use iced::{Border, Center, Element, Fill, Length, mouse};

    use super::{ICON_FONT, theme};

    /// The header's height (logical px) and the resize strip along each edge.
    pub const HEADER: f32 = 40.0;
    pub const EDGE: f32 = 6.0;
    /// One caption button's width (the three share it).
    const BUTTON: f32 = 46.0;

    /// What the frame reports.
    #[derive(Debug, Clone, Copy)]
    pub enum Event {
        /// Pressed on the header's free area: a drag, or the second half of a
        /// double-click (the shell tells them apart by time).
        Press,
        Minimise,
        /// The maximise / restore button.
        Maximise,
        Close,
        /// Pressed on an edge or corner: the OS resizes from that side.
        Resize(Direction),
    }

    /// What the window can do: a fixed-size window has no maximise button and no
    /// edges; a maximised one has no edges either.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Mode {
        pub resizable: bool,
        pub maximised: bool,
    }

    /// Our logo, decoded once: the window icon (taskbar, Alt+Tab) and the header's.
    pub struct Logo {
        pub width: u32,
        pub height: u32,
        pub rgba: Vec<u8>,
    }

    pub fn logo() -> &'static Logo {
        static LOGO: LazyLock<Logo> = LazyLock::new(|| {
            let bytes = include_bytes!("../../assets/icons/64x64.png");
            let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
                .read_info()
                .expect("the bundled logo is a PNG");
            let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
            let info = reader
                .next_frame(&mut rgba)
                .expect("the bundled logo decodes");
            rgba.truncate(info.buffer_size());
            Logo {
                width: info.width,
                height: info.height,
                rgba,
            }
        });
        &LOGO
    }

    /// A bundled RGBA PNG as an image (decode it once: keep the handle).
    pub(crate) fn png_handle(bytes: &[u8]) -> image::Handle {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("a bundled PNG");
        let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
        let info = reader.next_frame(&mut rgba).expect("the PNG decodes");
        rgba.truncate(info.buffer_size());
        image::Handle::from_rgba(info.width, info.height, rgba)
    }

    /// A caption button: quiet, a hover tint; `close` tints red.
    fn caption<'a, Message: Clone + 'a>(
        glyph: char,
        on_press: Message,
        close: bool,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        button(container(text(glyph).font(ICON_FONT).size(14)).center(Fill))
            .width(BUTTON)
            .height(HEADER)
            .padding(0)
            .on_press(on_press)
            .style(move |_, status| {
                let (background, text_color) = match (status, close) {
                    (button::Status::Hovered | button::Status::Pressed, true) => {
                        (Some(theme::CLOSE_HOVER.into()), theme::ON_CLOSE_HOVER)
                    }
                    (button::Status::Hovered, false) => (Some(palette.hover.into()), palette.text),
                    (button::Status::Pressed, false) => {
                        (Some(palette.pressed.into()), palette.text)
                    }
                    _ => (None, palette.text),
                };
                button::Style {
                    background,
                    text_color,
                    border: Border::default(),
                    ..button::Style::default()
                }
            })
            .into()
    }

    fn header<'a, Message: Clone + 'a>(
        title: &'a str,
        mode: Mode,
        on: &impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        let grip = row![
            image(crate::ui::kit::logo::logo(18)).width(18).height(18),
            text(title).size(13).color(palette.text)
        ]
        .spacing(10)
        .align_y(Center);
        let mut bar = row![
            mouse_area(
                container(grip)
                    .padding([0, 14])
                    .center_y(HEADER)
                    .width(Fill)
            )
            .on_press(on(Event::Press)),
            caption(super::MINUS, on(Event::Minimise), false),
        ];
        if mode.resizable {
            let glyph = if mode.maximised {
                super::RESTORE
            } else {
                super::STOP // lucide "square"
            };
            bar = bar.push(caption(glyph, on(Event::Maximise), false));
        }
        bar = bar.push(caption(super::CLOSE, on(Event::Close), true));
        container(bar)
            .height(HEADER)
            .style(move |_| container::Style {
                background: Some(palette.window.into()),
                text_color: Some(palette.text),
                ..container::Style::default()
            })
            .into()
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
        let corner =
            |direction, interaction| zone(direction, interaction, EDGE.into(), EDGE.into());
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

    /// `content` under the header, in a window-coloured frame with a 1 px border
    /// (not when maximised), and the resize edges on top.
    pub fn frame<'a, Message: Clone + 'a>(
        title: &'a str,
        mode: Mode,
        content: impl Into<Element<'a, Message>>,
        on: impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        let page = column![
            header(title, mode, &on),
            container(content).width(Fill).height(Fill)
        ];
        let border = if mode.maximised { 0.0 } else { 1.0 };
        let window = container(page)
            .width(Fill)
            .height(Fill)
            .padding(border)
            .style(move |_| container::Style {
                background: Some(palette.window.into()),
                border: Border {
                    color: palette.border,
                    width: border,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            });
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

    impl From<crate::ui::kit::header::Caption> for Event {
        fn from(caption: crate::ui::kit::header::Caption) -> Event {
            use crate::ui::kit::header::Caption;
            match caption {
                Caption::Drag => Event::Press,
                Caption::Minimise => Event::Minimise,
                Caption::Maximise => Event::Maximise,
                Caption::Close => Event::Close,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::ui::widgets::shot;
        use iced::Size;

        /// What one click at `at` reports, by name.
        fn pressing(at: (f32, f32), mode: Mode) -> Vec<String> {
            let page: Element<'_, Event> = text("page").into();
            let view = frame("Settings", mode, page, |e| e);
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
            is(pressing((200.0, 20.0), FREE), &["Press"]);
            is(pressing((284.0, 20.0), FREE), &["Minimise"]);
            is(pressing((330.0, 20.0), FREE), &["Maximise"]);
            is(pressing((380.0, 20.0), FREE), &["Close"]);
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
                is(pressing((200.0, 20.0), mode), &["Press"]);
            }
            // Fixed: close sits where maximise would be, one slot over.
            let fixed = Mode {
                resizable: false,
                maximised: false,
            };
            is(pressing((380.0, 20.0), fixed), &["Close"]);
            is(pressing((330.0, 20.0), fixed), &["Minimise"]);
        }

        #[test]
        fn the_logo_decodes() {
            let l = logo();
            assert_eq!((l.width, l.height), (64, 64));
            assert_eq!(l.rgba.len(), 64 * 64 * 4);
            assert!(l.rgba.chunks(4).any(|p| p[3] > 0), "not blank");
        }
    }
}

#[cfg(test)]
thread_local! {
    pub static TIPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Every visible tooltip built (its text, why, and the texts its control shows).
    pub static SHOWN: std::cell::RefCell<Vec<tests_support::Shown>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The tooltip policy as a test: what a screen's visible tooltips say must add
/// something to what their controls already show.
#[cfg(test)]
pub mod tests_support {
    use super::{SHOWN, TIPS, Why};
    use iced::advanced::layout::{Layout, Limits};
    use iced::advanced::widget::{Id, Operation, Tree};
    use iced::{Element, Font, Pixels, Rectangle, Size};

    pub struct Shown {
        pub label: String,
        pub why: String,
        pub tip: String,
        pub visible: Vec<String>,
    }

    /// The texts `content` shows (labels and typed text, glyphs included).
    fn texts<Message>(content: &mut Element<'_, Message>) -> Vec<String> {
        struct Collect(Vec<String>);
        impl Operation for Collect {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
                self.0.push(text.to_string());
            }
        }
        let renderer = iced::Renderer::new(Font::DEFAULT, Pixels(16.0));
        let mut tree = Tree::new(content.as_widget());
        let limits = Limits::new(Size::ZERO, Size::new(900.0, 700.0));
        let node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut collect = Collect(Vec::new());
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut collect);
        collect.0
    }

    pub fn record_shown<Message>(
        content: &mut Element<'_, Message>,
        label: &str,
        why: Why<'_>,
        tip: &str,
    ) {
        TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        let visible = texts(content);
        SHOWN.with(|s| {
            s.borrow_mut().push(Shown {
                label: label.to_string(),
                why: format!("{why:?}"),
                tip: tip.to_string(),
                visible,
            })
        });
    }

    /// The visible tooltips built since the last check (their texts).
    pub fn shown() -> Vec<String> {
        SHOWN.with(|s| s.borrow().iter().map(|s| s.tip.clone()).collect())
    }

    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// Asserts that every visible tooltip built since the last call says something
    /// its control does not: not its text again, not its shortcut again, and an
    /// icon-only control has no words on it. Clears the record.
    pub fn assert_tips_add_something(screen: &str) {
        let shown = SHOWN.with(|s| std::mem::take(&mut *s.borrow_mut()));
        for s in shown {
            let (tip, label) = (norm(&s.tip), norm(&s.label));
            for v in s.visible.iter().map(|v| norm(v)).filter(|v| !v.is_empty()) {
                assert!(
                    v != tip && v != label,
                    "{screen}: the tooltip {:?} repeats {:?} on its control",
                    s.tip,
                    s.visible
                );
                if s.why.starts_with("Keys") {
                    assert!(
                        !tip.ends_with(&v),
                        "{screen}: {:?} already shows its shortcut",
                        s.tip
                    );
                }
            }
            if s.why == "Icon" {
                let words = s
                    .visible
                    .iter()
                    .any(|v| v.chars().any(char::is_alphanumeric));
                assert!(!words, "{screen}: {:?} has words, so no tooltip", s.tip);
            }
        }
    }
}

/// Headless renders for tests (the kit's test harness).
#[cfg(test)]
pub use super::kit::testing::shot;

#[cfg(test)]
mod tests {
    /// No `iced::Shadow` with a blur anywhere (see `glass`): tiny-skia draws
    /// it outside the part being redrawn and it piles up into black blocks.
    #[test]
    fn no_drawn_shadows() {
        fn scan(dir: &std::path::Path, found: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    scan(&p, found);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let n = src.matches(concat!("blur", "_radius:")).count();
                    if n > 0 {
                        found.push(format!("{} ({n})", p.display()));
                    }
                }
            }
        }
        let mut found = Vec::new();
        scan(std::path::Path::new("src"), &mut found);
        assert!(found.is_empty(), "drawn shadows in {found:?}");
    }

    /// The ink of `sample` in `family` at `size` px laid out in one `LINE` line:
    /// (top, bottom) in px from the line's top.
    fn ink(sample: &str, family: &str, size: f32) -> (f32, f32) {
        use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
        let mut db = cosmic_text::fontdb::Database::new();
        for f in super::FONTS {
            db.load_font_data(f.to_vec());
        }
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut buf = Buffer::new(&mut fs, Metrics::new(size, size * super::LINE));
        let attrs = Attrs::new().family(Family::Name(family));
        buf.set_text(&mut fs, sample, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(&mut fs, false);
        let mut cache = SwashCache::new();
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for run in buf.layout_runs() {
            for glyph in run.glyphs {
                let p = glyph.physical((0.0, 0.0), 1.0);
                let Some(img) = cache.get_image(&mut fs, p.cache_key) else {
                    continue;
                };
                if img.placement.height == 0 {
                    continue;
                }
                let y = run.line_y + p.y as f32 - img.placement.top as f32;
                top = top.min(y);
                bottom = bottom.max(y + img.placement.height as f32);
            }
        }
        (top, bottom)
    }

    /// A field's line holds the tallest Latin and Arabic text (accents, Arabic
    /// marks above and below): a text field clips to its line (#53).
    #[test]
    fn a_line_holds_latin_and_arabic_marks() {
        let line = 14.0 * super::LINE;
        for (sample, family) in [
            ("ÅÉÎÕÛ jgpqy ÇŞ", "Inter"),
            ("إِنَّ الْعِلْمَ نُورٌ ضِّ", "Noto Sans Arabic"),
        ] {
            let (top, bottom) = ink(sample, family, 14.0);
            assert!(
                top >= 0.0 && bottom <= line,
                "{family}: ink {top}..{bottom} outside the {line} px line"
            );
        }
    }

    #[test]
    fn glyphs_are_in_the_subset_font() {
        let script = include_str!("../../assets/lucide/subset.sh");
        for glyph in [super::STOP, super::CLOSE, super::MINUS, super::RESTORE] {
            let code = format!("U+{:04X}", glyph as u32);
            assert!(script.contains(&code), "add {code} to subset.sh");
        }
    }
}
