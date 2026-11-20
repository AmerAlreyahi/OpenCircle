//! The Steps sidebar (STUDIO v3.1): the only side panel. Its header names it and
//! holds its toggle; then Add pictures / Add this picture / Paste as step, the
//! numbered steps (click = edit that step, drag = reorder, x = delete) and, at
//! the foot, Copy all and Export. The Steps strip's own steps, one store.
use std::sync::Arc;

use iced::Element;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, mouse_area};

use super::{Msg, Studio};
use crate::features::snip::annotate::Message;
use crate::features::snip::export;
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::features::snip::screens::steps::Step;
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::text::muted;
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{dir, dropdown, side_panel, thumb};
use crate::t;

/// The sidebar's side padding.
pub(super) const PAD: f32 = 12.0;

/// The toggle's glyph: it points at the side the sidebar is on.
pub(super) fn toggle_glyph(at_start: bool) -> char {
    match at_start != dir::rtl() {
        true => super::view::PANEL_LEFT,
        false => super::view::PANEL_RIGHT,
    }
}

/// The whole sidebar. `steps`: the Steps strip's steps.
pub fn view<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let wide = super::SIDEBAR - 2.0 * PAD;
    let add = button::labelled(
        super::view::PLUS,
        t!(st_add_pictures),
        wide,
        Kind::Elevated,
        false,
        Some(Msg::PickFiles),
    );
    let this = button::labelled(
        toolbar::COPY,
        t!(st_add_steps),
        wide,
        Kind::Elevated,
        false,
        s.editor.is_some().then_some(Msg::Editor(Message::AddStep)),
    );
    let paste = button::labelled(
        super::view::PASTE,
        t!(st_paste_step),
        wide,
        Kind::Elevated,
        false,
        Some(Msg::PasteStep),
    );
    let tools = container(column![add, this, paste].spacing(space::GAP)).padding([space::GAP, PAD]);
    let body = column![tools, steps_list(s, steps)];
    let panel = side_panel::panel(super::SIDEBAR, t!(st_steps), body.into());
    // (a file dropped while the pointer is here is a step)
    mouse_area(panel)
        .on_enter(Msg::OverSidebar(true))
        .on_exit(Msg::OverSidebar(false))
        .into()
}

/// The Steps strip's steps, four to a row: click = open on the canvas, drag =
/// reorder, x = delete; Copy all and Export at the foot.
fn steps_list<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    if steps.is_empty() {
        let none = muted(Role::ROW_DESC, t!(st_steps_none));
        return container(none).padding([0.0, PAD]).into();
    }
    let n = steps.len();
    let order = s.pick.order(n);
    let cells: Vec<Element<'a, Msg>> = order
        .into_iter()
        .enumerate()
        .map(|(place, i)| {
            let look = thumb::Look {
                hover: s.step_hover == Some(i) && !s.pick.dragging(),
                lifted: s.pick.lifted() == Some(i),
                focused: false,
                rtl: dir::rtl(),
            };
            let shot = thumb::mini(steps[i].thumb(), place + 1, look, Some(Msg::StepDelete(i)));
            let shot = mouse_area(shot)
                .on_press(Msg::StepPress(i))
                .on_enter(Msg::StepHover(i, true))
                .on_exit(Msg::StepHover(i, false));
            hint(shot, t!(st_step_tip), Why::Icon, Position::Right)
        })
        .collect();
    let mut cells = cells;
    let rows = cells.chunks_mut(super::STEP_COLS).map(|chunk| {
        let line = chunk
            .iter_mut()
            .map(|c| std::mem::replace(c, Space::new().into()));
        Element::from(dir::row(line.collect::<Vec<_>>()).spacing(super::STEP_GAP))
    });
    let list = mouse_area(column(rows.collect::<Vec<_>>()).spacing(super::STEP_GAP))
        .on_move(move |at| Msg::StepMove(at, n))
        .on_release(Msg::StepRelease);
    let half = (super::SIDEBAR - 2.0 * PAD - space::GAP) / 2.0;
    let copy = button::labelled(
        toolbar::COPY,
        t!(sn_step_copy_all),
        half,
        Kind::Accent,
        false,
        Some(Msg::CopyAllSteps),
    );
    let rows = [
        (export::Steps::Markdown, t!(st_export_md)),
        (export::Steps::Html, t!(st_export_html)),
        (export::Steps::Images, t!(st_export_images)),
    ];
    // the walkthrough first; one step is no walkthrough
    let walk = (n > 1).then(|| {
        (
            t!(st_export_walkthrough).to_string(),
            false,
            Msg::ExportWalkthrough,
        )
    });
    let rows = (walk.into_iter())
        .chain(
            rows.into_iter()
                .map(|(how, words)| (words.to_string(), false, Msg::ExportSteps(how))),
        )
        .collect();
    let export = dropdown::labelled_menu(t!(st_export), rows, half);
    let foot = crate::dir_row![copy, export].spacing(space::GAP);
    column![
        container(list).padding([0.0, PAD]),
        container(foot).padding([space::GAP, PAD])
    ]
    .into()
}
