//! `.oc-card`: radius 12, `card_idle` with a divider edge. `card` is the info
//! card (`!cursor-default !p-4`, 16 px in); `theme_card` and `look_card` are
//! the picking cards (9x10 px in): under the cursor `hover` with a `rim` edge,
//! chosen `card_selected` with an `elevated_border` edge (light themes ring it
//! in `card_ring`). States change colour only (no lift: the CPU renderer
//! redraws the card alone). `grid` lays cards out in equal columns.
use iced::gradient::Linear;
use iced::widget::{Space, button, column, container, row, stack};
use iced::{Border, Center, Color, Element, Fill, Gradient, Radians};

use super::chip::badge;
use super::text::{sub, text};
use super::themes::{Theme, c};
use super::tokens::{Role, radius, space};

const PAD: f32 = 16.0;
/// A theme card's dots (`w-2.5`), each 4 px over the one before (`-space-x-1`).
const DOT: f32 = 10.0;
const DOT_STEP: f32 = DOT - 4.0;
/// A look card's colour strip (`h-2`).
const STRIP: f32 = 8.0;
/// Between a theme card's lines (`gap-1`; a look card's are `GAP`).
const LINE_GAP: f32 = 4.0;

pub fn card<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// A picking card round `content`; no `on_press` = it can't be picked (drawn
/// at rest).
fn pick<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    selected: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use button::Status;
    button(content)
        .width(Fill)
        .padding(space::CARD)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let t = c();
            let (fill, edge) = match (selected, status) {
                (true, _) => (t.card_selected, t.card_ring.unwrap_or(t.elevated_border)),
                (false, Status::Hovered | Status::Pressed) => (t.hover, t.rim),
                (false, _) => (t.card_idle, t.divider),
            };
            button::Style {
                background: Some(fill.into()),
                text_color: t.text,
                border: Border::default().rounded(radius::CARD).width(1).color(edge),
                ..button::Style::default()
            }
        })
        .into()
}

/// A round dot of `colour` with a faint light edge (`border-white/25`).
fn dot<'a, Message: 'a>(colour: Color) -> Element<'a, Message> {
    container(Space::new().width(DOT).height(DOT))
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.25)),
            ..container::Style::default()
        })
        .into()
}

/// `ThemeCard`: the name (`CARD_TITLE`) and the theme's three dots, its
/// description (`CARD_DESC`, `text_sub`) under them.
pub fn theme_card<'a, Message: Clone + 'a>(
    theme: &'static Theme,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    // each dot over the one before it, as the design stacks them
    let dots = stack(
        theme
            .swatch
            .iter()
            .enumerate()
            .map(|(i, &s)| row![Space::new().width(DOT_STEP * i as f32), dot(s)].into()),
    )
    // (a stack is as big as its first layer: the three dots' span)
    .width(DOT_STEP * 2.0 + DOT)
    .height(DOT);
    let top = row![
        container(text(Role::CARD_TITLE, theme.name)).width(Fill),
        dots
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let lines = column![top, sub(Role::CARD_DESC, theme.desc)].spacing(LINE_GAP);
    pick(lines, selected, Some(on_press))
}

/// `ActiveLookCard`: a strip of the look's colours (left to right), the name
/// (`CARD_TITLE`) with its badge, the description (`CARD_DESC`, `text_sub`)
/// under them. `on_press: None` = not offered (its name muted).
pub fn look_card<'a, Message: Clone + 'a>(
    name: &'a str,
    badge_label: &'a str,
    desc: &'a str,
    colours: &[Color],
    selected: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let last = colours.len().max(2) - 1;
    let gradient: Gradient = (colours.iter().enumerate())
        .fold(Linear::new(Radians::PI / 2.0), |l, (i, &colour)| {
            l.add_stop(i as f32 / last as f32, colour)
        })
        .into();
    let strip =
        container(Space::new().width(Fill).height(STRIP)).style(move |_| container::Style {
            background: Some(gradient.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        });
    let mut title = text(Role::CARD_TITLE, name);
    if on_press.is_none() {
        title = title.color(c().text_muted);
    }
    let top = row![container(title).width(Fill), badge(badge_label)]
        .spacing(LINE_GAP)
        .align_y(Center);
    let lines =
        column![strip, top, sub(Role::CARD_DESC, desc).line_height(1.375)].spacing(space::GAP);
    pick(lines, selected, on_press)
}

/// `cards` in rows of `columns`, each card an equal share, `GAP` apart (the
/// design's `grid gap-1.5`); a short last row keeps the column width.
pub fn grid<'a, Message: 'a>(
    columns: usize,
    cards: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut cards = cards.into_iter().peekable();
    let mut rows = column![].spacing(space::GAP);
    while cards.peek().is_some() {
        let mut line: Vec<Element<'a, Message>> = cards.by_ref().take(columns).collect();
        while line.len() < columns {
            line.push(Space::new().width(Fill).into());
        }
        rows = rows.push(row(line).spacing(space::GAP));
    }
    rows.into()
}
