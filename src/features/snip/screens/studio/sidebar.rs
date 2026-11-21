//! The Steps sidebar (STUDIO v3.1): the only side panel. Its header names it and
//! holds its toggle; then Add pictures / Add this picture / Paste as step, the
//! numbered steps (click = edit that step, drag = reorder, x = delete) and, at
//! the foot, Copy all and Export. The Steps strip's own steps, one store.
use std::sync::Arc;

use iced::Element;
use iced::widget::{Space, column, container, mouse_area};

use super::{Msg, Page, Studio};
use crate::features::snip::annotate::Message;
use crate::features::snip::export;
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::features::snip::screens::steps::Step;
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::text::muted;
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{dir, side_panel, thumb};
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
    let list = column![tools, steps_list(s, steps)];
    // Export is a second page that slides in (back arrow, Esc); with no step there is none
    let body: Element<'a, Msg> = match (s.page, s.slide, steps.is_empty()) {
        (_, _, true) | (Page::Steps, None, _) => list.into(),
        (Page::Export, None, _) => export_page(s, steps),
        (_, Some(slide), _) => {
            let t = match slide.into {
                true => s.slide_at(),
                false => 1.0 - s.slide_at(),
            };
            crate::shared::kit::slide_pages::pages(
                list.into(),
                export_page(s, steps),
                super::SIDEBAR,
                t,
                dir::rtl(),
            )
        }
    };
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
    let tip_at = super::view::inward(!super::view::rail_on_left(s.sidebar_at_start(), dir::rtl()));
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
            hint(shot, t!(st_step_tip), Why::Icon, tip_at)
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
    let export = button::labelled(
        crate::shared::kit::icon::Icon::Forward.directed().glyph(),
        t!(st_export),
        half,
        Kind::Elevated,
        false,
        Some(Msg::SidebarPage(Page::Export)),
    );
    let foot = crate::dir_row![copy, export].spacing(space::GAP);
    column![
        container(list).padding([0.0, PAD]),
        container(foot).padding([space::GAP, PAD])
    ]
    .into()
}

/// The Export page: a back arrow and its title, then one row per way out. The walkthrough
/// has its own roomy slide-over (it does not fit this width).
fn export_page<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let _ = s;
    let back = crate::shared::kit::icon_button::square(
        crate::shared::kit::icon::Icon::Back.directed().glyph(),
        crate::shared::kit::icon_button::State::default(),
        Some(Msg::SidebarPage(Page::Steps)),
    );
    let back = hint(
        back,
        t!(st_export_back),
        Why::Keys("Esc"),
        super::view::inward(false),
    );
    let head = container(
        dir::row([
            back,
            crate::shared::kit::text::text(Role::ROW_TITLE, t!(st_export)).into(),
        ])
        .spacing(space::GAP)
        .width(iced::Fill)
        .align_y(iced::Center),
    )
    .padding([space::GAP, PAD]);
    let rows = [
        (export::Steps::Markdown, t!(st_export_md)),
        (export::Steps::Html, t!(st_export_html)),
        (export::Steps::Images, t!(st_export_images)),
    ];
    let walk = (steps.len() > 1).then(|| {
        crate::shared::kit::option_list::item(
            t!(st_export_walkthrough),
            false,
            None,
            false,
            Msg::ExportWalkthrough,
        )
    });
    let list = walk.into_iter().chain(rows.into_iter().map(|(how, words)| {
        crate::shared::kit::option_list::item(words, false, None, false, Msg::ExportSteps(how))
    }));
    column![
        head,
        crate::shared::kit::divider::divider(),
        container(column(list.collect::<Vec<_>>()).spacing(space::GAP / 4.0))
            .padding([space::GAP, PAD])
    ]
    .into()
}
