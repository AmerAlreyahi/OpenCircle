//! `.oc-card`: radius 12, `card_idle` with a divider edge. `card` is the info
//! card (`!cursor-default !p-4`, 16 px in); `theme_card` and `look_card` are
//! the picking cards (9x10 px in): under the cursor `hover` with a `rim` edge,
//! chosen `card_selected` with an `elevated_border` edge (light themes ring it
//! in `card_ring`). States change colour only (no lift: the CPU renderer
//! redraws the card alone). `grid` lays cards out in equal columns.
use super::text::{sub, text};
use super::themes::{Theme, c};
use super::tokens::{Role, radius, space};
use iced::widget::{Space, button, column, container, row, stack};
use iced::{Border, Center, Color, Element, Fill};

const PAD: f32 = 16.0;
/// A theme card's dots (`w-2.5`), each 4 px over the one before (`-space-x-1`).
const DOT: f32 = 10.0;
const DOT_STEP: f32 = DOT - 4.0;
/// Between a theme card's lines (`gap-1`).
const LINE_GAP: f32 = 4.0;

pub fn card<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    shell(content, [PAD, PAD], |t| (t.card_idle, t.divider))
}

/// The one card surface: radius 12, a 1 px edge, `fill_edge` picks the fill and the edge.
fn shell<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    padding: [f32; 2],
    fill_edge: impl Fn(&super::themes::Colours) -> (Color, Color) + 'a,
) -> Element<'a, Message> {
    container(content)
        .padding(padding)
        .width(Fill)
        .style(move |_| {
            let (fill, edge) = fill_edge(c());
            container::Style {
                background: Some(fill.into()),
                border: Border::default().rounded(radius::CARD).width(1).color(edge),
                ..container::Style::default()
            }
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
    super::focus::press(
        button(content)
            .width(Fill)
            .padding(space::CARD)
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
            }),
        radius::CARD,
        on_press,
    )
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

/// `ThemeCard`: the `name` (`CARD_TITLE`) and the theme's three dots, its
/// `desc` (`CARD_DESC`, `text_sub`) under them (the words in the app's
/// language: `theme.name` / `theme.desc` in English).
pub fn theme_card<'a, Message: Clone + 'a>(
    theme: &'static Theme,
    (name, desc): (&'a str, &'a str),
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
    let top = crate::dir_row![
        container(text(Role::CARD_TITLE, name))
            .width(Fill)
            .align_x(super::dir::start()),
        dots
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let lines = column![top, sub(Role::CARD_DESC, desc)]
        .spacing(LINE_GAP)
        .align_x(super::dir::start());
    pick(lines, selected, Some(on_press))
}

/// `ActiveLookCard`: a strip of the look's colours (left to right), the name
/// (`CARD_TITLE`) with its badge, the description (`CARD_DESC`, `text_sub`)
/// under them. `on_press: None` = not offered (its name muted).
/// `cards` in rows of `columns`, each card an equal share, `GAP` apart (the
/// design's `grid gap-1.5`); a short last row keeps the column width.
pub fn grid<'a, Message: 'a>(
    columns: usize,
    cards: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    grid_spaced(columns, space::GAP, cards)
}

/// Like `grid`, with an explicit gap between rows and columns (`settings::CARD_GAP` in Settings).
pub fn grid_spaced<'a, Message: 'a>(
    columns: usize,
    gap: f32,
    cards: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut cards = cards.into_iter().peekable();
    let mut rows = column![].spacing(gap);
    while cards.peek().is_some() {
        let mut line: Vec<Element<'a, Message>> = cards.by_ref().take(columns).collect();
        while line.len() < columns {
            line.push(Space::new().width(Fill).into());
        }
        rows = rows.push(super::dir::row(line).spacing(gap));
    }
    rows.into()
}

/// A saved chat in History (`renderAiHistoryListHtml`), a picking card
/// `width` wide: the star (a favourite toggle: `accent` when on, faint when
/// off), the 12 px / 600 title cut to its room, under them the 10.5 px meta
/// in `text_sub` ("18m ago · 680×190 px", the size in mono), and at the right
/// "Del" (10 px `text_muted`, `DANGER` on a rose tint under the cursor).
/// `selected`: the chat that is open.
pub struct Chat<'a, Message> {
    pub title: &'a str,
    pub when: String,
    pub crop: Option<String>,
    pub starred: bool,
    pub selected: bool,
    pub width: f32,
    pub open: Message,
    pub star: Message,
    pub delete: Message,
    /// The delete button's word ("Del").
    pub delete_label: &'a str,
}

pub fn chat<'a, Message: Clone + 'a>(chat: Chat<'a, Message>) -> Element<'a, Message> {
    use super::button::faded;
    use super::icon::{Icon, icon};
    use super::tokens::fixed;
    use iced::font::Weight;
    use iced::widget::text as plain;
    const TITLE: f32 = 12.0;
    const META: f32 = 10.5;
    const STAR: f32 = 12.0;
    let quiet = |hot: bool, on: bool| button::Style {
        text_color: match (on, hot) {
            (true, _) => c().accent,
            (false, true) => c().text,
            (false, false) => faded(c().text_muted, 0.6),
        },
        ..button::Style::default()
    };
    let starred = chat.starred;
    let star = button(icon(Icon::Star, STAR))
        .padding(0)
        .style(move |_, s| {
            quiet(
                matches!(s, button::Status::Hovered | button::Status::Pressed),
                starred,
            )
        });
    let star = super::focus::press(star, 4.0, Some(chat.star));
    let del = button(
        plain(chat.delete_label)
            .size(10)
            .font(super::text::weight(Weight::Normal)),
    )
    .padding([2, 6])
    .style(|_, s| {
        let hot = matches!(s, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hot.then(|| faded(fixed::DANGER, 0.2).into()),
            text_color: if hot { fixed::DANGER } else { c().text_muted },
            border: Border::default().rounded(4),
            ..button::Style::default()
        }
    });
    let del = super::focus::press(del, radius::CONTROL, Some(chat.delete));
    let font = super::text::weight(Weight::Semibold);
    // the card's padding and edges, the star and its gap, Del and its gap
    let room = chat.width - 2.0 * (space::CARD[1] + 1.0) - (STAR + space::GAP) - 36.0;
    let title = super::text::cut(chat.title, TITLE, font, room.max(0.0));
    let title = crate::dir_row![star, plain(title).size(TITLE).font(font)]
        .spacing(space::GAP)
        .align_y(Center);
    let meta_font = super::text::weight(Weight::Normal);
    let meta = crate::dir_row![plain(chat.when).size(META).font(meta_font)].spacing(8);
    let meta = match chat.crop {
        Some(crop) => meta.push(
            crate::dir_row![
                plain("·").size(META).font(meta_font),
                plain(crop)
                    .size(META)
                    .font(super::text::mono(Weight::Normal))
            ]
            .spacing(8),
        ),
        None => meta,
    };
    let meta = container(meta).style(|_| container::Style {
        text_color: Some(c().text_sub),
        ..container::Style::default()
    });
    let lines = column![title, meta]
        .spacing(2)
        .width(Fill)
        .align_x(super::dir::start());
    let content = crate::dir_row![lines, del].spacing(8).align_y(Center);
    container(pick(content, chat.selected, Some(chat.open)))
        .width(chat.width)
        .into()
}

/// A small rounded icon tile on `hover` with a subtle `divider` rim (`extra.css` `.mic`, `.gic`).
pub fn icon_tile<'a, Message: 'a>(
    glyph: char,
    icon_size: f32,
    box_size: f32,
    corner: f32,
) -> Element<'a, Message> {
    container(super::icon::glyph(glyph, icon_size).color(c().text_sub))
        .center(box_size)
        .style(move |_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default()
                .rounded(corner)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// A one-line quiet note row with a leading icon tile (`extra.css` `.gnote`).
pub fn quiet_note<'a, Message: 'a>(glyph: char, line: &'a str) -> Element<'a, Message> {
    let tile = icon_tile(
        glyph,
        super::tokens::size::ICON_SMALL,
        super::tokens::settings::NOTE_TILE,
        radius::MENU,
    );
    container(
        super::dir::row([
            tile,
            super::text::paragraph(sub(Role::CHOICE_DESC, line)).into(),
        ])
        .spacing(super::tokens::settings::CARD_GAP)
        .align_y(Center),
    )
    .width(Fill)
    .padding(
        iced::Padding::ZERO
            .top(super::tokens::settings::ROW_PAD_Y)
            .bottom(super::tokens::settings::CHOICE_PAD),
    )
    .into()
}

/// The 4 states of the OpenRouter sign-in connection card (Option A).
pub enum ConnectionState<'a, Message> {
    NotConnected {
        title: &'a str,
        desc: &'a str,
        cta: &'a str,
        on_sign_in: Option<Message>,
        error: Option<&'a str>,
    },
    Waiting {
        title: &'a str,
        desc: &'a str,
        status: &'a str,
        countdown: String,
        turn: f32,
        still: bool,
        cancel: &'a str,
        on_cancel: Message,
        reopen: &'a str,
        on_reopen: Message,
    },
    Connected {
        title: &'a str,
        saved: &'a str,
        desc: &'a str,
        error: Option<&'a str>,
        test: &'a str,
        on_test: Message,
        disconnect: &'a str,
        on_disconnect: Message,
    },
    Failed {
        title: &'a str,
        desc: &'a str,
        detail: &'a str,
        retry: &'a str,
        on_retry: Message,
        own_link: &'a str,
        on_own: Message,
    },
}

/// Lucide key glyph drawn with crisp strokes inside the 48x48 neutral tile.
struct KeyMark {
    colour: Color,
}

impl<Message> iced::widget::canvas::Program<Message> for KeyMark {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::Point;
        use iced::widget::canvas::Frame;
        use iced::widget::canvas::LineCap;
        use iced::widget::canvas::LineJoin;
        use iced::widget::canvas::Path;
        use iced::widget::canvas::Stroke;
        let mut frame = Frame::new(renderer, bounds.size());
        let s = bounds.width.min(bounds.height) / 24.0;
        let ox = (bounds.width - 24.0 * s) * 0.5;
        let oy = (bounds.height - 24.0 * s) * 0.5;
        let pt = |x: f32, y: f32| Point::new(ox + x * s, oy + y * s);
        let stroke = Stroke::default()
            .with_color(self.colour)
            .with_width((2.0 * s).max(1.5))
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);
        frame.stroke(&Path::circle(pt(7.5, 15.5), 5.5 * s), stroke);
        let shaft = Path::new(|b| {
            b.move_to(pt(21.0, 2.0));
            b.line_to(pt(11.4, 11.6));
            b.move_to(pt(15.5, 7.5));
            b.line_to(pt(18.5, 10.5));
            b.line_to(pt(22.0, 7.0));
            b.line_to(pt(19.0, 4.0));
        });
        frame.stroke(&shaft, stroke);
        vec![frame.into_geometry()]
    }
}

/// OpenRouter sign-in connection card (Option A, 4 states).
pub fn connection_card<'a, Message: Clone + 'a>(
    state: ConnectionState<'a, Message>,
) -> Element<'a, Message> {
    use super::button::Kind;
    use super::button::button;
    use super::button::compact_accent;
    use super::button::compact_elevated;
    use super::button::danger_pill;
    use super::button::faded;
    use super::button::label;
    use super::colour::mix;
    use super::icon::Icon;
    use super::icon::glyph;
    use super::logo;
    use super::spinner;
    use super::text::error;
    use super::text::muted;
    use super::text::paragraph;
    use super::themes::current;
    use super::tokens::fixed;
    use super::tokens::settings;
    use iced::Padding;

    let neutral_tile = || -> Element<'a, Message> {
        let col = c().text;
        container(
            iced::widget::canvas(KeyMark { colour: col })
                .width(settings::CONCARD_ICON)
                .height(settings::CONCARD_ICON),
        )
        .center(settings::CONCARD_TILE)
        .style(|_| container::Style {
            background: Some(c().elevated.into()),
            border: Border::default()
                .rounded(settings::CONCARD_TILE_RADIUS)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
    };
    let tone_tile = |ic: Icon, tone: Color, bg_a: f32, rim_a: f32| -> Element<'a, Message> {
        container(glyph(ic.glyph(), settings::CONCARD_ICON).color(tone))
            .center(settings::CONCARD_TILE)
            .style(move |_| container::Style {
                background: Some(faded(tone, bg_a).into()),
                border: Border::default()
                    .rounded(settings::CONCARD_TILE_RADIUS)
                    .width(1)
                    .color(faded(tone, rim_a)),
                ..container::Style::default()
            })
            .into()
    };
    let quiet_link = |txt: &'a str, on_press: Message| -> Element<'a, Message> {
        button(sub(Role::CONTROL, txt), Kind::Quiet, Some(on_press))
    };

    let (bg, edge, pad, body): (Color, Color, [f32; 2], Element<'a, Message>) = match state {
        ConnectionState::NotConnected {
            title,
            desc,
            cta,
            on_sign_in,
            error: err_line,
        } => {
            let btn = compact_accent(Icon::External.glyph(), cta, on_sign_in);
            let btn = match err_line {
                Some(why) => super::tooltip::tip(btn, why, iced::widget::tooltip::Position::Top),
                None => btn,
            };
            let mut main = column![
                text(Role::CONCARD_HEAD, title),
                container(paragraph(sub(Role::ROW_DESC, desc)))
                    .max_width(settings::CONCARD_DESC_MAX),
                container(btn).padding(Padding::ZERO.top(settings::CONCARD_ACTION_TOP)),
            ]
            .spacing(settings::CARD_GAP)
            .width(Fill)
            .align_x(super::dir::start());
            if let Some(why) = err_line {
                main = main.push(
                    container(paragraph(error(Role::ROW_DESC, why)))
                        .max_width(settings::CONCARD_DESC_MAX),
                );
            }
            let row = super::dir::row([neutral_tile(), main.into()]).spacing(settings::CONCARD_GAP);
            let bg = if current().light {
                c().bg
            } else {
                fixed::DEMO_DARK_BG
            };
            (bg, c().divider, settings::CONCARD_PAD, row.into())
        }
        ConnectionState::Waiting {
            title,
            desc,
            status,
            countdown,
            turn,
            still,
            cancel,
            on_cancel,
            reopen,
            on_reopen,
        } => {
            let spin = spinner::spinner(
                logo::logo(settings::CONCARD_SPINNER as u32),
                settings::CONCARD_SPINNER,
                turn,
                still,
            );
            let count = muted(Role::CONCARD_COUNT, countdown);
            let wait_box = container(
                super::dir::row([
                    spin,
                    sub(Role::ROW_DESC, status).into(),
                    Space::new().width(Fill).into(),
                    count.into(),
                ])
                .spacing(settings::CARD_GAP)
                .align_y(Center),
            )
            .max_width(settings::CONCARD_WAIT_MAX)
            .width(Fill)
            .padding(settings::CONCARD_WAIT_PAD)
            .style(|_| container::Style {
                background: Some(c().widget.into()),
                border: Border::default()
                    .rounded(radius::CARD)
                    .width(1)
                    .color(c().divider),
                ..container::Style::default()
            });
            let actions = super::dir::row([
                compact_elevated(cancel, Some(on_cancel)),
                quiet_link(reopen, on_reopen),
            ])
            .spacing(settings::CONCARD_GAP)
            .align_y(Center);
            let main = column![
                text(Role::CONCARD_HEAD, title),
                container(paragraph(sub(Role::ROW_DESC, desc)))
                    .max_width(settings::CONCARD_DESC_MAX),
                wait_box,
                container(actions).padding(Padding::ZERO.top(settings::CONCARD_ACTION_TOP)),
            ]
            .spacing(settings::CARD_GAP)
            .width(Fill)
            .align_x(super::dir::start());
            let row = super::dir::row([neutral_tile(), main.into()]).spacing(settings::CONCARD_GAP);
            let bg = if current().light {
                c().bg
            } else {
                fixed::DEMO_DARK_BG
            };
            (bg, c().divider, settings::CONCARD_PAD, row.into())
        }
        ConnectionState::Connected {
            title,
            saved,
            desc,
            error: err_line,
            test,
            on_test,
            disconnect,
            on_disconnect,
        } => {
            let head = super::dir::row([
                text(Role::CONCARD_OK_HEAD, title).into(),
                muted(Role::ROW_TITLE, "·").into(),
                text(Role::CONCARD_OK_HEAD, saved)
                    .color(fixed::SUCCESS)
                    .into(),
            ])
            .spacing(space::GAP)
            .align_y(Center);
            let mut info = column![head, sub(Role::ROW_DESC, desc)]
                .spacing(LINE_GAP)
                .width(Fill)
                .align_x(super::dir::start());
            if let Some(why) = err_line {
                info = info.push(error(Role::ROW_DESC, why));
            }
            let actions = super::dir::row([
                button(label(test), Kind::Quiet, Some(on_test)),
                danger_pill(disconnect, Some(on_disconnect)),
            ])
            .spacing(space::ROW_Y)
            .align_y(Center);
            let row = super::dir::row([
                tone_tile(
                    Icon::Check,
                    fixed::SUCCESS,
                    settings::CONCARD_OK_TILE_A,
                    settings::CONCARD_OK_TILE_RIM_A,
                ),
                info.into(),
                actions.into(),
            ])
            .spacing(settings::CONCARD_GAP)
            .align_y(Center);
            (
                mix(c().widget, fixed::SUCCESS, settings::CONCARD_OK_TINT),
                faded(fixed::SUCCESS, settings::CONCARD_OK_BORDER_A),
                settings::CONCARD_OK_PAD,
                row.into(),
            )
        }
        ConnectionState::Failed {
            title,
            desc,
            detail,
            retry,
            on_retry,
            own_link,
            on_own,
        } => {
            let actions = super::dir::row([
                compact_accent(Icon::Refresh.glyph(), retry, Some(on_retry)),
                quiet_link(own_link, on_own),
            ])
            .spacing(settings::CONCARD_GAP)
            .align_y(Center);
            let main = column![
                text(Role::CONCARD_HEAD, title),
                container(paragraph(sub(Role::ROW_DESC, desc)))
                    .max_width(settings::CONCARD_DESC_MAX),
                container(paragraph(muted(Role::META, detail)))
                    .max_width(settings::CONCARD_DESC_MAX),
                container(actions).padding(Padding::ZERO.top(settings::CONCARD_ACTION_TOP)),
            ]
            .spacing(settings::CARD_GAP)
            .width(Fill)
            .align_x(super::dir::start());
            let row = super::dir::row([
                tone_tile(
                    Icon::Alert,
                    fixed::DANGER,
                    settings::CONCARD_ERR_TILE_A,
                    settings::CONCARD_ERR_TILE_RIM_A,
                ),
                main.into(),
            ])
            .spacing(settings::CONCARD_GAP);
            (
                mix(c().widget, fixed::DANGER, settings::CONCARD_ERR_TINT),
                faded(fixed::DANGER, settings::CONCARD_ERR_BORDER_A),
                settings::CONCARD_PAD,
                row.into(),
            )
        }
    };

    container(container(body).width(Fill).padding(pad).style(move |_| {
        container::Style {
            background: Some(bg.into()),
            border: Border::default()
                .rounded(settings::CONCARD_RADIUS)
                .width(1)
                .color(edge),
            ..container::Style::default()
        }
    }))
    .width(Fill)
    .padding(Padding::from([settings::ROW_PAD_Y, 0.0]))
    .into()
}
