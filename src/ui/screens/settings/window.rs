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
use crate::app_language::{S, count, tr};
use crate::t;
use crate::ui::kit::button::{Kind, button, cancel, label};
use crate::ui::kit::chip::{Tone, status};
use crate::ui::kit::divider::divider;
use crate::ui::kit::header::regular;
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::sidebar::{Item, sidebar};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, size, space};

/// The window's 1 px rim (none when maximised: the screen's edge is the edge).
const RIM: f32 = 1.0;
/// Under the page's title (`mb-1.5`).
const TITLE_GAP: f32 = 6.0;
/// The longest the header pill's line gets (characters), before "…".
const PILL_MAX: usize = 48;

/// The page's content box (the tests find its width by this id).
pub(super) const PAGE_ID: &str = "settings-page";

/// The sidebar's footer: this version and this OS.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const OS: &str = if cfg!(windows) { "Windows" } else { "Linux" };

pub fn view(state: &State, maximised: bool) -> Element<'_, Message> {
    // the whole window in the app's language and direction
    let _dir = crate::ui::kit::dir::scope(crate::app_language::rtl());
    let logo = crate::ui::kit::logo::logo(16);
    let header = regular(
        logo,
        t!(win_title),
        pill(state),
        Some(maximised),
        Message::Caption,
    );
    let mut main = column![crate::dir_row![nav(state), page(state)].height(Fill)];
    if state.asking_close {
        main = main.push(close_question());
    }
    let rim = if maximised { 0.0 } else { RIM };
    // Tab / Shift+Tab walk the sidebar, then the page (`kit::focus`)
    crate::ui::kit::focus::scope(
        surface(Material::Window, 0.0, column![header, divider(), main])
            .padding(rim)
            .width(Fill)
            .height(Fill),
    )
}

/// The header pill, only when something needs the user (owner, 2026-10-29:
/// changes save by themselves, so "Auto-save active" / "Saved" say nothing).
fn pill(state: &State) -> Element<'_, Message> {
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
        return Space::new().into();
    };
    status(cut(&line), tone)
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
        // the way to About's "Something not working?" card, from every page
        Some((t!(ab_report), Message::Section(Section::About))),
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
    let title = crate::ui::kit::dir::row(title).align_y(Center);
    let body = column![title, Space::new().height(TITLE_GAP),]
        .push(previews::view(state))
        .push((page.view)(state));
    // at most `PAGE_MAX` wide, at the start edge (a maximised window)
    let body = (container(body).id(PAGE_ID))
        .width(Fill)
        .max_width(size::PAGE_MAX);
    let body = container(body)
        .padding(space::PAGE)
        .width(Fill)
        .align_x(crate::ui::kit::dir::start());
    scroll(body).width(Fill).height(Fill).into()
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
