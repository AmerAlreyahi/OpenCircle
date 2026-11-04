//! A list of options and its opener (the design's `.oc-dropdown-item` rows
//! and the live controller's language buttons): `opener` shows the current
//! choice with a ▾ (`accent` = the target: `accent_soft` with `accent` text),
//! `option` is one row of the open list: 12 px / 500 `text`, 6.5x10 px in,
//! radius 8, `hover` under the cursor, `active` (the current one) raised on
//! `elevated` with its edge; a `badge` at its end. States change colour only.
//! A menu (`menu`, `item`, `separator`: the tray menu) is the same kind of
//! list: plain rows with a shortcut hint.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard::key::Named;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Border, Center, Element, Fill, Padding};

use super::button::faded;
use super::chip::badge as tag;
use super::divider::divider;
use super::icon::{Icon, icon};
use super::surface::{Material, surface};
use super::text::font;
use super::themes::c;
use super::tokens::{Role, radius, size, space};

/// A row's corners (`rounded-lg`) and its room.
const ROW_RADIUS: f32 = 8.0;
const ROW_PAD: [f32; 2] = [6.5, 10.0];
/// The opener's room (`px-2.5`).
const OPENER_PAD: f32 = 10.0;

/// The opener of a list (the translate pill's language chips): `label` and
/// a chevron, `width` x `height`, radius 9, 12 px; `accent` = the target's
/// look (600, `accent` on `accent_soft`), else 500 `text` on `hover`. `open`:
/// its list shows (an up chevron, a 2 px focus ring, a stronger fill). `rtl`:
/// mirrored (the label at the right, the chevron at the left).
pub fn opener<'a, Message: Clone + 'a>(
    label: String,
    accent: bool,
    open: bool,
    rtl: bool,
    (width, height): (f32, f32),
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let role = match accent {
        true => Role {
            weight: iced::font::Weight::Semibold,
            ..Role::CONTROL
        },
        false => Role::CONTROL,
    };
    let name = text(label).size(role.size).font(font(role));
    let name = match rtl {
        true => container(name).align_right(Fill),
        false => container(name).width(Fill),
    };
    let chevron = icon(
        if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        size::ICON_SMALL,
    );
    let content = match rtl {
        true => row![chevron, name],
        false => row![name, chevron],
    }
    .spacing(space::GAP)
    .align_y(Center);
    button(container(content).center_y(Fill))
        .width(width)
        .height(height)
        .padding([0.0, OPENER_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = open || matches!(status, Status::Hovered | Status::Pressed);
            let (fill, colour) = match (accent, hot) {
                (true, false) => (c().accent_soft, c().accent),
                (true, true) => (faded(c().accent, 0.26), c().accent),
                (false, false) => (c().hover, c().text),
                (false, true) => (c().elevated, c().text),
            };
            let ring = match open {
                true => Border::default()
                    .rounded(radius::CONTROL)
                    .width(FOCUS_RING)
                    .color(faded(c().accent, 0.32)),
                false => Border::default().rounded(radius::CONTROL),
            };
            button::Style {
                background: Some(fill.into()),
                text_color: colour,
                border: ring,
                ..button::Style::default()
            }
        })
        .into()
}

/// An open list's frame (the dropdown's menu, the language lists): the
/// `dropdown` material, a `rim` edge, radius 14.
pub fn frame<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Container<'a, Message> {
    container(content).style(|_| container::Style {
        background: Some(c().dropdown.into()),
        border: Border::default()
            .rounded(radius::DROPDOWN)
            .width(1)
            .color(c().rim),
        ..container::Style::default()
    })
}

/// How a `choice` row shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    /// The current choice: raised on `elevated`, an accent check at its end.
    pub current: bool,
    /// The keyboard's row: `hover` with a focus ring.
    pub ringed: bool,
    /// Under the pointer.
    pub hovered: bool,
    /// Needs AI: the quiet `needs_ai` ring at its end.
    pub needs_ai: bool,
}

/// A row of a list with a search (the language lists): `label` (12 px /
/// 500, the `bold` byte range of it in 700: what was typed), `width` x
/// `CHOICE_H`, radius 8, 10 px in; `look` says the rest. `rtl`: mirrored.
pub fn choice<'a, Message: Clone + 'a>(
    label: &str,
    bold: Option<(usize, usize)>,
    look: Look,
    width: f32,
    rtl: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let role = Role::CONTROL;
    let strong = Role {
        weight: iced::font::Weight::Bold,
        ..role
    };
    let name: Element<'a, Message> = match bold {
        Some((a, b)) => {
            // three texts: one shaped run takes one weight per word
            let part = |s: &str, r: Role| {
                text(s.to_string())
                    .size(r.size)
                    .font(font(r))
                    .color(c().text)
            };
            row![
                part(&label[..a], role),
                part(&label[a..b], strong),
                part(&label[b..], role)
            ]
            .into()
        }
        None => text(label.to_string())
            .size(role.size)
            .font(font(role))
            .color(c().text)
            .into(),
    };
    let name: Element<'a, Message> = match rtl {
        true => container(name).align_right(Fill).into(),
        false => container(name).width(Fill).into(),
    };
    let mut ends: Vec<Element<'a, Message>> = Vec::new();
    if look.needs_ai {
        ends.push(needs_ai(look.hovered || look.ringed));
    }
    if look.current {
        ends.push(icon(Icon::Check, size::ICON_SMALL).color(c().accent).into());
    }
    let mut parts = vec![name];
    parts.extend(ends);
    if rtl {
        parts.reverse();
    }
    let content = row(parts).spacing(space::GAP + 2.0).align_y(Center);
    button(container(content).center_y(Fill))
        .width(width)
        .height(CHOICE_H)
        .padding([0.0, OPENER_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = look.hovered || matches!(status, button::Status::Hovered);
            let (fill, edge) = match () {
                _ if look.current => (Some(c().elevated), c().elevated_border),
                _ if look.ringed => (Some(c().hover), faded(c().accent, 0.32)),
                _ if hot => (Some(c().hover), iced::Color::TRANSPARENT),
                _ => (None, iced::Color::TRANSPARENT),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: c().text,
                border: Border::default().rounded(ROW_RADIUS).width(1).color(edge),
                ..button::Style::default()
            }
        })
        .into()
}

/// A `choice` row's height.
pub const CHOICE_H: f32 = 28.0;
/// The open opener's ring.
const FOCUS_RING: f32 = 2.0;
/// The needs-AI mark: a 12 px ring (the OpenCircle logo's circle).
const MARK: f32 = 12.0;
const MARK_EDGE: f32 = 1.5;

/// The quiet needs-AI mark: a 12 px `text_muted` ring (`text_sub` when
/// `loud`: its row is hovered or ringed). Never a badge or an AI glyph.
pub fn needs_ai<'a, Message: 'a>(loud: bool) -> Element<'a, Message> {
    container(Space::new())
        .width(MARK)
        .height(MARK)
        .style(move |_| container::Style {
            border: Border::default()
                .rounded(radius::FULL)
                .width(MARK_EDGE)
                .color(if loud { c().text_sub } else { c().text_muted }),
            ..container::Style::default()
        })
        .into()
}

/// A list's group heading ("Recent", "All languages"): 10.5 px / 600
/// `text_muted`, 10 px in, at the bottom of its `height`. `rtl`: at the right.
pub fn group<'a, Message: 'a>(label: &'a str, height: f32, rtl: bool) -> Element<'a, Message> {
    let role = Role::SECTION;
    let line = text(label)
        .size(role.size)
        .font(font(role))
        .color(c().text_muted);
    let line = container(line).padding(Padding {
        left: OPENER_PAD,
        right: OPENER_PAD,
        bottom: 4.0,
        top: 0.0,
    });
    let line = match rtl {
        true => line.align_right(Fill),
        false => line.width(Fill),
    };
    line.align_bottom(height).into()
}

/// A menu (the design's tray menu): `MENU_WIDTH` wide, its `items` and
/// `separator`s on the widget surface with `radius::MENU` corners, 4 px in
/// (`p-1`, inside the 1 px edge). As tall as `menu_height` says.
pub fn menu<'a, Message: 'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Widget, radius::MENU, column(items))
        .padding(MENU_PAD)
        .width(MENU_WIDTH)
        .into()
}

/// The menu's width, a row's height, the room around the rows (edge + `p-1`)
/// and a separator's height (1 px, `my-1`).
pub const MENU_WIDTH: f32 = 240.0;
pub const ITEM_H: f32 = 32.0;
const MENU_PAD: f32 = 5.0;
const SEPARATOR_PAD: [f32; 2] = [4.0, 8.0];
const SEPARATOR_H: f32 = 9.0;
/// The room before the check mark (`ml-1`), before the hint (`pl-2`) and in a
/// row (`px-2`); the check's size (`w-3`).
const CHECK_GAP: f32 = 4.0;
const HINT_GAP: f32 = 8.0;
const ITEM_PAD: f32 = 8.0;
const CHECK: f32 = 12.0;

/// A menu of `items` rows and `separators` lines: its height.
pub fn menu_height(items: usize, separators: usize) -> f32 {
    2.0 * MENU_PAD + items as f32 * ITEM_H + separators as f32 * SEPARATOR_H
}

/// A menu's hairline between groups (`h-px my-1 mx-2`).
pub fn separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(divider()).padding(SEPARATOR_PAD).into()
}

/// A menu row: `label` (13 px `text`), a muted ✓ after it when `checked`
/// (a toggle that is on), the muted `hint` (its shortcut) at the end; the
/// whole menu wide, `ITEM_H` tall, `hover` under the cursor or when
/// `focused` (the keyboard's row). Quit is no different (not red).
pub fn item<'a, Message: Clone + 'a>(
    label: &'a str,
    checked: bool,
    hint: Option<String>,
    focused: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let (role, small) = (Role::MENU, Role::MENU_HINT);
    let check = checked.then(|| {
        container(icon(Icon::Check, CHECK).color(c().text_muted)).padding(Padding {
            left: CHECK_GAP,
            ..Padding::ZERO
        })
    });
    let hint = hint.map(|h| {
        container(
            text(h)
                .size(small.size)
                .font(font(small))
                .color(c().text_muted),
        )
        .padding(Padding {
            left: HINT_GAP,
            ..Padding::ZERO
        })
    });
    let content = row![text(label).size(role.size).font(font(role))]
        .push(check)
        .push(Space::new().width(Fill))
        .push(hint)
        .align_y(Center);
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(ITEM_H)
        .padding([0.0, ITEM_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = focused || matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: hot.then(|| c().hover.into()),
                text_color: c().text,
                border: Border::default().rounded(radius::MENU_ITEM),
                ..button::Style::default()
            }
        })
        .into()
}

/// A menu row that runs something (Snip's More): its `glyph` (16 px,
/// `text_sub`), `label` (13 px `text`), its `key` (mono, muted) at the end;
/// `hover` under the cursor or when `focused`; no `on_press` = dimmed.
pub fn action<'a, Message: Clone + 'a>(
    glyph: char,
    label: &'a str,
    key: Option<&'a str>,
    focused: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use button::Status;
    let (role, small) = (Role::MENU, Role::KEYCAP);
    let live = on_press.is_some();
    let ink = move |c: iced::Color| if live { c } else { faded(c, 0.45) };
    let content = row![
        text(glyph)
            .font(super::icon::FONT)
            .size(super::tokens::size::ICON)
            .color(ink(c().text_sub)),
        text(label)
            .size(role.size)
            .font(font(role))
            .color(ink(c().text)),
        Space::new().width(Fill),
    ]
    .push(key.map(|k| {
        text(k)
            .size(small.size)
            .font(font(small))
            .color(ink(c().text_muted))
    }))
    .spacing(ITEM_PAD)
    .align_y(Center);
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(ITEM_H)
        .padding([0.0, ITEM_PAD])
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = focused || (live && matches!(status, Status::Hovered | Status::Pressed));
            button::Style {
                background: hot.then(|| c().hover.into()),
                text_color: c().text,
                border: Border::default().rounded(radius::MENU_ITEM),
                ..button::Style::default()
            }
        })
        .into()
}

/// One option: `label`, a `badge` at its end, `width` x `height`.
pub fn option<'a, Message: Clone + 'a>(
    label: String,
    badge: Option<&'a str>,
    active: bool,
    (width, height): (f32, f32),
    on_press: Message,
) -> Element<'a, Message> {
    use button::Status;
    let role = Role::CONTROL;
    let content = row![container(text(label).size(role.size).font(font(role))).width(Fill)]
        .push(badge.map(tag))
        .spacing(space::GAP)
        .align_y(Center);
    button(container(content).center_y(Fill))
        .width(width)
        .height(height)
        .padding(ROW_PAD)
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, Status::Hovered | Status::Pressed);
            let (fill, edge) = match () {
                _ if active => (Some(c().elevated), c().elevated_border),
                _ if hot => (Some(c().hover), iced::Color::TRANSPARENT),
                _ => (None, iced::Color::TRANSPARENT),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: c().text,
                border: Border::default().rounded(ROW_RADIUS).width(1).color(edge),
                ..button::Style::default()
            }
        })
        .into()
}

/// `content` (a list with a search field) that takes the keys a list walks
/// with before its field sees them: `on_key` maps a named key (with Shift)
/// to a message; those keys are captured, every other event goes on.
pub fn keyed<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_key: impl Fn(Named, bool) -> Option<Message> + 'a,
) -> Element<'a, Message> {
    Element::new(Keyed {
        content: content.into(),
        on_key: Box::new(on_key),
    })
}

type OnKey<'a, Message> = Box<dyn Fn(Named, bool) -> Option<Message> + 'a>;

struct Keyed<'a, Message> {
    content: Element<'a, Message>,
    on_key: OnKey<'a, Message>,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Keyed<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> iced::Size<iced::Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> iced::Size<iced::Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        (self.content.as_widget()).draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        (self.content.as_widget_mut()).operate(tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        use iced::keyboard::{Event, Key};
        if let iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(named),
            modifiers,
            ..
        }) = event
            && let Some(message) = (self.on_key)(*named, modifiers.shift())
        {
            shell.publish(message);
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        (self.content.as_widget()).mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &iced::Rectangle,
        translation: iced::Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        (self.content.as_widget_mut()).overlay(tree, layout, renderer, viewport, translation)
    }
}
