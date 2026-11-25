//! The Settings window's shell (`settings-window.js`): our own header (logo,
//! title, the pill when something needs the user, the caption buttons) across the window, the
//! one hairline under it (the sidebar's edge, also `kit::divider`, starts
//! there; owner, 2026-10-28), the sidebar of `PAGES`
//! with its gliding highlight, and the page. The page switches at once: the
//! design's rise (the whole page moving 14 px over 190 ms) redrew the whole
//! window every frame on the CPU renderer (laggy on a 4K screen); it returns
//! with the GPU renderer.
use iced::widget::{Space, column, container};
use iced::{Center, Element, Fill, Padding};

use super::state::{Message, Section, State};
use super::{pages, previews};
use crate::shared::app_language::{S, count, tr};
use crate::shared::kit::button::{Kind, button, cancel, label};
use crate::shared::kit::chip::{Tone, status};
use crate::shared::kit::divider::divider;
use crate::shared::kit::header::regular;
use crate::shared::kit::modal;
use crate::shared::kit::scroll::scroll;
use crate::shared::kit::sidebar::{Item, sidebar};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{sub, text};
use crate::shared::kit::tokens::{Role, size, space};
use crate::t;

/// The window's 1 px rim (none when maximised: the screen's edge is the edge).
const RIM: f32 = 1.0;
/// Under the page's title (`mb-1.5`).
const TITLE_GAP: f32 = 6.0;
/// The longest the header pill's line gets (characters), before "…".
const PILL_MAX: usize = 22;
/// About how wide a header pill is per character, and its dot and padding (the search keeps clear of it).
const PILL_CHAR: f32 = 7.0;
const PILL_PAD: f32 = 28.0;
/// The caption buttons and the gap before them.
const BUTTONS: f32 = 3.0 * 46.0 + 8.0;

/// The page's content box (the tests find its width by this id).
pub(super) const PAGE_ID: &str = "settings-page";

/// The sidebar's footer: this version and this OS.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const OS: &str = if cfg!(windows) { "Windows" } else { "Linux" };

pub fn view(state: &State, maximised: bool) -> Element<'_, Message> {
    // the whole window in the app's language and direction
    let _dir = crate::shared::kit::dir::scope(crate::shared::app_language::rtl());
    let logo = crate::shared::kit::logo::logo(16);
    let header = regular(
        logo,
        t!(win_title),
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
    let body: Element<'_, Message> = crate::dir_row![nav(state), page(state)].height(Fill).into();
    // F13: the hits hang from the field, over the page
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
        surface(Material::Window, 0.0, column![header, divider(), main])
            .padding(rim)
            .width(Fill)
            .height(Fill),
    );
    match state.reporting {
        true => modal::over(window, report_dialog(), REPORT_W, Message::Dismiss),
        false => window,
    }
}

/// The search field's widest (F13).
const SEARCH_W: f32 = 240.0;
/// The hits' list width, and the gap between its rows.
const HITS_W: f32 = 420.0;
const HITS_GAP: f32 = 2.0;

/// The header's search (Ctrl+K, or Ctrl+F, focuses it), centred on the window: typing opens the search; the keys
/// are shown at its end while it is empty.
fn search_field(state: &State) -> Element<'_, Message> {
    let query = state.search.as_ref().map_or("", |s| s.query.as_str());
    let field = crate::shared::kit::field::search(t!(win_search_ph), query)
        .id(super::search::FIELD)
        .on_input(Message::SearchInput)
        .on_submit(Message::SearchGo);
    crate::shared::kit::keycap::at_end(field, &["Ctrl", "K"], query.is_empty())
}

/// The list of hits under the header (none while nothing is typed): "row title .
/// page", the arrows' row marked, a hint line at its foot.
fn search_hits(state: &State) -> Option<Element<'_, Message>> {
    use crate::shared::kit::option_list::item;
    let search = state
        .search
        .as_ref()
        .filter(|s| !s.query.trim().is_empty())?;
    let hits = search.hits();
    let mut list = column![].spacing(HITS_GAP);
    if hits.is_empty() {
        list = list.push(sub(Role::ROW_DESC, t!(win_search_none)));
    }
    for (i, e) in hits.iter().enumerate() {
        let hint = (!e.is_page).then(|| e.page.to_string());
        list = list.push(item(
            &e.title,
            false,
            hint,
            i == search.at,
            Message::SearchPick(i),
        ));
    }
    list = list
        .push(divider())
        .push(sub(Role::ROW_DESC, t!(win_search_keys)));
    let panel = surface(
        Material::Widget,
        crate::shared::kit::tokens::radius::CARD,
        list,
    )
    .padding(space::GAP)
    .width(HITS_W);
    Some(
        container(iced::widget::opaque(panel))
            .padding(Padding::ZERO.top(space::GAP))
            .width(Fill)
            .center_x(Fill)
            .into(),
    )
}

/// The dialog's width.
const REPORT_W: f32 = 420.0;

/// "Report a problem" (owner, 2026-11-18): copy the diagnostics (the accent: the report
/// needs them), then report; nothing is sent.
fn report_dialog<'a>() -> Element<'a, Message> {
    modal::pad(vec![
        modal::header(t!(ab_trouble), None, Message::Dismiss),
        sub(Role::ROW_DESC, t!(ab_trouble_desc)).into(),
        crate::dir_row![
            Space::new().width(Fill),
            crate::shared::kit::button::regular(t!(ab_diag), true, Some(Message::CopyDiagnostics)),
            crate::shared::kit::button::regular(t!(ab_report), false, Some(Message::ReportProblem)),
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
    let items: Vec<Item<'static>> = (shown.iter())
        .map(|p| Item {
            label: tr(p.title),
            badge: p.badge.map(tr),
            icon: None,
        })
        .collect();
    let at = |id| shown.iter().position(|p| p.id == id);
    let active = at(state.section).unwrap_or(0);
    let (from, glide) = match state.moving() {
        Some((from, glide, _)) => (at(from).unwrap_or(active), glide),
        None => (active, 1.0),
    };
    let ids: Vec<_> = shown.iter().map(|p| p.id).collect();
    sidebar(
        &items,
        active,
        from,
        glide,
        move |i| Message::Section(ids[i]),
        // (in RTL the footer mirrors: the version at the right)
        [VERSION, OS],
        // the Report a problem dialog, from every page
        Some((t!(ab_report), Message::ReportOpen)),
    )
}

fn page(state: &State) -> Element<'_, Message> {
    let page = pages::of(state.section);
    let mut title = vec![
        text(Role::PAGE_TITLE, tr(page.title)).into(),
        Space::new().width(Fill).into(),
    ];
    if let Some(note) = page.note {
        title.push(sub(Role::ROW_DESC, tr(note)).into());
    }
    let title = crate::shared::kit::dir::row(title).align_y(Center);
    let body =
        column![title, Space::new().height(TITLE_GAP),]
            // (the glossary editor is a sub-page: no preview over it)
            .push(previews::view(state).filter(|_| {
                !(state.section == Section::Translate && state.glossaries.is_editing())
            }))
            .push(crate::shared::kit::setting_row::highlighting(
                state.marked.as_deref(),
                || (page.view)(state),
            ));
    // at most `PAGE_MAX` wide, at the start edge (a maximised window)
    let body = (container(body).id(PAGE_ID))
        .width(Fill)
        .max_width(size::PAGE_MAX);
    let body = container(body)
        .padding(space::PAGE)
        .width(Fill)
        .align_x(crate::shared::kit::dir::start());
    scroll(body)
        .id(super::search::PAGE)
        .width(Fill)
        .height(Fill)
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
