//! The Settings window's shell (`settings-window.js`): our own header (logo,
//! title, the auto-save pill, the caption buttons), the sidebar of `PAGES`
//! with its gliding highlight, and the page, rising in as it changes.
use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill, Padding};

use super::pages;
use super::state::{Message, State};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::chip::{Tone, status};
use crate::ui::kit::header::regular;
use crate::ui::kit::motion::PAGE_RISE;
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::sidebar::{Item, sidebar};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{sub, text};
use crate::ui::kit::tokens::{Role, space};

/// The window's 1 px rim (none when maximised: the screen's edge is the edge).
const RIM: f32 = 1.0;
/// Under the page's title (`mb-1.5`).
const TITLE_GAP: f32 = 6.0;
/// The longest the header pill's line gets (characters), before "…".
const PILL_MAX: usize = 48;

/// The sidebar's footer: this version and this OS.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const OS: &str = if cfg!(windows) { "Windows" } else { "Linux" };

pub fn view(state: &State, maximised: bool) -> Element<'_, Message> {
    let logo = crate::ui::widgets::chrome::logo_handle();
    let header = regular(
        logo,
        "OpenCircle Settings",
        pill(state),
        maximised,
        Message::Caption,
    );
    let mut main = column![row![nav(state), page(state)].height(Fill)];
    if state.asking_close {
        main = main.push(close_question());
    }
    let rim = if maximised { 0.0 } else { RIM };
    surface(Material::Window, 0.0, column![header, main])
        .padding(rim)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The auto-save pill: what is wrong, else what the last change did.
fn pill(state: &State) -> Element<'_, Message> {
    let n = state.problems.len();
    let (line, tone) = if n > 0 {
        let what = if n == 1 { "problem" } else { "problems" };
        (format!("{n} {what} to fix before saving"), Tone::Error)
    } else if let Some(notice) = &state.notice {
        (notice.clone(), Tone::Error)
    } else if state.asking_close {
        ("Save your changes before closing?".into(), Tone::Error)
    } else if state.writing {
        ("Saving…".into(), Tone::Busy)
    } else if state.saved_at.is_some() {
        ("Saved".into(), Tone::Ok)
    } else {
        ("Auto-save active".into(), Tone::Ok)
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
            label: p.title,
            badge: p.badge,
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
        [VERSION, OS],
    )
}

fn page(state: &State) -> Element<'_, Message> {
    let page = pages::of(state.section);
    let mut title =
        row![text(Role::PAGE_TITLE, page.title), Space::new().width(Fill)].align_y(Center);
    if let Some(note) = page.note {
        title = title.push(sub(Role::ROW_DESC, note));
    }
    let rise = state
        .moving()
        .map_or(0.0, |(.., rise)| (1.0 - rise) * PAGE_RISE);
    let body = column![
        Space::new().height(rise),
        title,
        Space::new().height(TITLE_GAP),
        (page.view)(state),
    ];
    scroll(container(body).padding(space::PAGE).width(Fill))
        .width(Fill)
        .height(Fill)
        .into()
}

/// Closing with changes that can't be saved yet: Save / Discard / Keep editing.
fn close_question<'a>() -> Element<'a, Message> {
    let b = |s, m| button(label(s), Kind::Elevated, Some(m));
    container(
        row![
            Space::new().width(Fill),
            b("Discard", Message::CloseDiscard),
            b("Keep editing", Message::CloseKeep),
            b("Save", Message::CloseSave),
        ]
        .spacing(space::GAP)
        .align_y(Center),
    )
    .padding(Padding::from(space::PAGE))
    .into()
}
