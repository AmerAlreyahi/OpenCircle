//! The Settings window's shell (`settings-window.js`): our own header (logo,
//! title, the pill when something needs the user, the caption buttons) across the window, the
//! one hairline under it (the sidebar's edge, also `kit::divider`, starts
//! there; owner, 2026-10-28), the sidebar of `PAGES`
//! with its gliding highlight, and the page. The page switches at once: the
//! design's rise (the whole page moving 14 px over 190 ms) redrew the whole
//! window every frame on the CPU renderer (laggy on a 4K screen); it returns
//! with the GPU renderer.
use crate::shared::kit::icon::Icon;
use iced::widget::{Space, column, container, mouse_area, row};
use iced::{Center, Element, Fill, Padding};

use super::state::{Message, State};
use super::{pages, previews};
use crate::shared::app_language::{S, count, fmt, tr};
use crate::shared::kit::button::{Kind, button, cancel, label};
use crate::shared::kit::chip::{Tone, status, tag};
use crate::shared::kit::floating_card::floating_card;
use crate::shared::kit::header::regular_with_chip;
use crate::shared::kit::modal;
use crate::shared::kit::nav_card::{Entry, Foot, nav_card};
use crate::shared::kit::page_head::page_head;
use crate::shared::kit::scroll::scroll;
use crate::shared::kit::search_card::{Hit, search_card};
use crate::shared::kit::text::sub;
use crate::shared::kit::tokens::settings::{self, CONTENT_MAX, CONTENT_PAD, GUTTER, SEARCH_W};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::t;

/// The window's 1 px rim (none when maximised: the screen's edge is the edge).
const RIM: f32 = 1.0;
/// The longest the header pill's line gets (characters), before "…".
const PILL_MAX: usize = 22;
/// About how wide a header pill is per character, and its dot and padding (the search keeps clear of it).
const PILL_CHAR: f32 = 7.0;
const PILL_PAD: f32 = 28.0;
/// The caption buttons and the gap before them.
const BUTTONS: f32 = 3.0 * 46.0 + 8.0;

/// The page's content box (the tests find its width by this id).
pub(super) const PAGE_ID: &str = "settings-page";

/// The title bar's chip: this version.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub fn view(state: &State, maximised: bool) -> Element<'_, Message> {
    // the whole window in the app's language and direction (the title bar below is never mirrored)
    let _dir = crate::shared::kit::dir::scope(crate::shared::app_language::rtl());
    let logo = crate::shared::kit::logo::logo(16);
    let header = regular_with_chip(
        logo,
        t!(win_title),
        tag(VERSION),
        pill(state),
        Some(maximised),
        Message::Caption,
    );
    // the search sits EXACTLY on the window's middle (W107): a layer over the whole header, kept clear
    // of the title and of the buttons by the same margin on both sides, narrower when the window is
    let header = crate::shared::kit::header::with_centre(
        header,
        search_field(state),
        SEARCH_W,
        search_margin(state),
    );
    // two floating cards on the calm pane: the sidebar and the page
    let cards = crate::dir_row![nav(state), page(state)]
        .spacing(GUTTER)
        .height(Fill);
    let body: Element<'_, Message> = container(cards)
        .padding(Padding::new(GUTTER).top(0.0))
        .height(Fill)
        .into();
    // F13: the card hangs from the field, over the page
    let body = match search_hits(state) {
        Some(hits) => iced::widget::stack![body, hits].into(),
        None => body,
    };
    let mut main = column![body];
    if state.asking_close {
        main = main.push(close_question());
    }
    let rim = if maximised { 0.0 } else { RIM };
    // Tab / Shift+Tab walk the sidebar, then the page (`kit::focus`)
    let window = crate::shared::kit::focus::scope(
        crate::shared::kit::floating_card::pane(column![header, main])
            .padding(rim)
            .width(Fill)
            .height(Fill),
    );
    match state.reporting {
        true => modal::over(
            window,
            report_dialog(),
            settings::REPORT_W,
            Message::Dismiss,
        ),
        false => window,
    }
}

/// The header's search (Ctrl+K, or Ctrl+F, focuses it), centred on the window: a click or typing opens
/// it; no keys drawn in it (the tooltip names them).
fn search_field(state: &State) -> Element<'_, Message> {
    let query = state.search.as_ref().map_or("", |s| s.query.as_str());
    let field = crate::shared::kit::field::search(t!(win_search_ph), query)
        .id(super::search::FIELD)
        .on_input(Message::SearchInput)
        .on_submit(Message::SearchGo);
    let field = mouse_area(field).on_press(Message::SearchOpen);
    hint(
        field,
        t!(win_search_ph),
        Why::Keys("Ctrl+K"),
        iced::widget::tooltip::Position::Bottom,
    )
}

/// The card under the field: the typed query's hits, or (open, nothing typed) "Most used" with the
/// current values; the chosen row marked, a hint line at its foot.
fn search_hits(state: &State) -> Option<Element<'_, Message>> {
    let search = state.search.as_ref()?;
    let typed = !search.query.trim().is_empty();
    let glyph_of = |s| pages::of(s).glyph;
    let (head, rows, total): (_, Vec<Hit<Message>>, usize) = match typed {
        false => (
            Some((t!(win_search_most), t!(win_search_type))),
            super::search::most_used(state)
                .into_iter()
                .enumerate()
                .map(|(i, q)| Hit {
                    glyph: q.glyph,
                    title: q.title,
                    marks: Vec::new(),
                    crumb: q.crumb,
                    value: q.value,
                    selected: i == search.at,
                    on_press: Message::QuickPick(i),
                })
                .collect(),
            0,
        ),
        true => {
            let all = search.all_hits().len();
            (
                None,
                search
                    .hits()
                    .iter()
                    .enumerate()
                    .map(|(i, e)| Hit {
                        glyph: glyph_of(e.section),
                        marks: search.marks(&e.title),
                        title: e.title.clone(),
                        crumb: match e.is_page {
                            true => String::new(),
                            false => e.page.to_string(),
                        },
                        value: None,
                        selected: i == search.at,
                        on_press: Message::SearchPick(i),
                    })
                    .collect(),
                all,
            )
        }
    };
    let count_text = match typed {
        true => fmt(S::win_search_count, &[&total]),
        false => String::new(),
    };
    let card = search_card(
        head,
        rows,
        t!(win_search_none),
        (t!(win_search_keys), count_text),
    );
    // under the field, as wide as it (it narrows with the window as the field does)
    let layer = container(container(card).max_width(SEARCH_W).width(Fill))
        .padding(Padding::from([0.0, search_margin(state)]).top(0.0))
        .width(Fill)
        .center_x(Fill);
    Some(layer.into())
}

/// "Report a problem" (owner, 2026-11-18): copy the diagnostics (the accent: the report
/// needs them), then report; nothing is sent.
fn report_dialog<'a>() -> Element<'a, Message> {
    modal::pad(vec![
        // the flag, then the title and the x (never mirrored, like the header)
        row![
            crate::shared::kit::icon::glyph(Icon::Flag.glyph(), settings::FLAG)
                .color(crate::shared::kit::themes::c().accent),
            modal::header(t!(ab_trouble), None, Message::Dismiss),
        ]
        .spacing(space::GAP)
        .align_y(Center)
        .into(),
        sub(Role::ROW_DESC, t!(ab_trouble_desc)).into(),
        // the buttons sit in the middle of the popup (owner, 2026-11-25)
        crate::dir_row![
            Space::new().width(Fill),
            crate::shared::kit::button::regular(t!(ab_diag), true, Some(Message::CopyDiagnostics)),
            crate::shared::kit::button::regular(t!(ab_report), false, Some(Message::ReportProblem)),
            Space::new().width(Fill),
        ]
        .spacing(space::GAP)
        .align_y(Center)
        .into(),
    ])
}

/// The header pill, only when something needs the user (owner, 2026-10-29:
/// changes save by themselves, so "Auto-save active" / "Saved" say nothing).
fn pill_line(state: &State) -> Option<(String, Tone)> {
    let n = state.problems.len();
    let (line, tone) = if n > 0 {
        let forms = [
            S::win_pill_one,
            S::win_pill_two,
            S::win_pill_few,
            S::win_pill_many,
        ];
        (count(n, forms), Tone::Error)
    } else if let Some(notice) = &state.notice {
        (notice.clone(), Tone::Error)
    } else if state.asking_close {
        (t!(win_ask_close).into(), Tone::Error)
    } else {
        return None;
    };
    Some((cut(&line), tone))
}

fn pill(state: &State) -> Element<'_, Message> {
    match pill_line(state) {
        Some((line, tone)) => status(line, tone),
        None => Space::new().into(),
    }
}

/// What the search keeps clear on each side of the window's middle: the title (left) and the buttons
/// with a shown pill (right), the same on both sides so it stays centred; the pill wins (the field
/// narrows, down to its icon and keys).
fn search_margin(state: &State) -> f32 {
    let pill = pill_line(state).map_or(0.0, |(line, _)| {
        line.chars().count() as f32 * PILL_CHAR + PILL_PAD
    });
    crate::shared::kit::header::CENTRE_MARGIN.max(BUTTONS + pill)
}

fn cut(line: &str) -> String {
    match line.chars().count() > PILL_MAX {
        true => line.chars().take(PILL_MAX - 1).chain(['…']).collect(),
        false => line.to_string(),
    }
}

fn nav(state: &State) -> Element<'_, Message> {
    let shown: Vec<_> = pages::visible(&state.draft.labs).collect();
    let items: Vec<Entry<'static>> = (shown.iter())
        .map(|p| Entry {
            label: tr(p.title),
            glyph: p.glyph,
            tail: p.badge.map(tr),
            group: p.group,
        })
        .collect();
    // (a page with no row of its own highlights the row its content lives in)
    let home = |id| match id {
        super::state::Section::TextReading => super::state::Section::Look,
        super::state::Section::History => super::state::Section::Privacy,
        id => id,
    };
    let active = shown
        .iter()
        .position(|p| p.id == home(state.section))
        .unwrap_or(0);
    let ids: Vec<_> = shown.iter().map(|p| p.id).collect();
    nav_card(
        &items,
        active,
        state.nav_collapsed,
        move |i| Message::Section(ids[i]),
        Foot {
            label: t!(ab_report),
            glyph: Icon::Flag.glyph(),
            keys: &[],
            on_press: Message::ReportOpen,
        },
        Foot {
            label: t!(win_collapse),
            glyph: Icon::PanelLeft.glyph(),
            keys: &["Ctrl", "B"],
            on_press: Message::ToggleNav,
        },
    )
}

fn page(state: &State) -> Element<'_, Message> {
    let wide = state.win_w >= settings::WIDE;
    let page = pages::of(state.section);
    // A3+: "See how it works" opens the clip under the head (a wide window shows it in a column)
    let see = previews::opener(state)
        .filter(|_| !wide)
        .map(|(key, open)| {
            let go = Some(Message::Preview(key, !open));
            button(label(t!(c_see_how)), Kind::Quiet, go)
        });
    let head = page_head(page.glyph, tr(page.title), tr(page.line), see);
    let editing =
        state.section == super::state::Section::Translate && state.glossaries.is_editing();
    // (the glossary editor is a sub-page: no preview over it)
    let clip = previews::view(state, wide).filter(|_| !editing);
    let (above, beside) = match wide {
        true => (None, clip),
        false => (clip, None),
    };
    let body = column![head]
        .push(above)
        .push(crate::shared::kit::setting_row::highlighting(
            state.marked.as_deref(),
            || match page.sectioned {
                true => (page.view)(state),
                false => crate::shared::kit::section::framed((page.view)(state)),
            },
        ))
        .spacing(settings::SECTION_GAP);
    // at most `CONTENT_MAX` wide, in the middle of the card
    let body = container(container(body).id(PAGE_ID).width(Fill))
        .padding(Padding::from([CONTENT_PAD[0], CONTENT_PAD[1]]))
        .max_width(CONTENT_MAX)
        .width(Fill);
    let body = container(body).center_x(Fill).width(Fill);
    let mut inside = row![
        scroll(body)
            .id(super::search::PAGE)
            .width(Fill)
            .height(Fill)
    ];
    if let Some(clip) = beside {
        inside = inside.push(
            container(clip)
                .width(settings::CLIP_COLUMN)
                .padding(Padding::from([CONTENT_PAD[0], settings::CLIP_PAD_X])),
        );
    }
    floating_card(inside)
        .width(Fill)
        .height(Fill)
        .clip(true)
        .into()
}

/// Closing with changes that can't be saved yet: Save / Discard / Keep editing.
fn close_question<'a>() -> Element<'a, Message> {
    let b = |s, m| button(label(s), Kind::Elevated, Some(m));
    container(
        crate::dir_row![
            Space::new().width(Fill),
            b(t!(win_discard), Message::CloseDiscard),
            cancel(
                label(t!(win_keep_editing)),
                Kind::Elevated,
                Message::CloseKeep
            ),
            b(t!(win_save), Message::CloseSave),
        ]
        .spacing(space::GAP)
        .align_y(Center),
    )
    .padding(Padding::from(space::PAGE))
    .into()
}
