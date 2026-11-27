//! The Steps sidebar (STUDIO v3.1): the only side panel. Its header names it and
//! holds its toggle; then one Add step menu (Pictures / Paste / This picture), the
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
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::muted;
use crate::shared::kit::tokens::{Role, radius, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{dir, dropdown, scroll, thumb};
use crate::t;

/// The floating card's id (the tests read its bounds).
pub(super) const CARD: &str = "studio-steps-card";

/// The sidebar's side padding.
pub(super) const PAD: f32 = 12.0;

/// The whole sidebar. `steps`: the Steps strip's steps.
pub fn view<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let wide = super::SIDEBAR - 2.0 * PAD;
    // one button, one menu (Pictures... / Paste / This picture)
    let rows = add_rows(s);
    let add = dropdown::menu_labelled(super::view::PLUS, t!(st_add_step), rows, wide);
    let tools = container(add).padding([space::GAP, PAD]);
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
    let panel = card(head(s, steps.len()), body);
    // (a file dropped while the pointer is here is a step)
    mouse_area(panel)
        .on_enter(Msg::OverSidebar(true))
        .on_exit(Msg::OverSidebar(false))
        .into()
}

/// The floating card (design r6): the header, a hairline and the scrolling body on a raised
/// rounded card. The card is the only surface (no square fill under it), so its corners stay round.
fn card<'a>(head: Element<'a, Msg>, body: Element<'a, Msg>) -> Element<'a, Msg> {
    let head = container(head)
        .padding(iced::Padding::ZERO.left(PAD).right(PAD))
        .height(size::HEADER)
        .center_y(size::HEADER);
    surface(
        Material::Widget,
        radius::DRAWER,
        column![
            head,
            crate::shared::kit::divider::divider(),
            scroll::scroll(body).height(iced::Fill)
        ],
    )
    .id(CARD)
    .width(super::SIDEBAR)
    .height(iced::Fill)
    .into()
}

/// The closed panel's handle: a slim tab on the window's edge with an arrow toward the picture
/// and the step count; a press slides the panel in.
pub fn handle<'a>(count: usize, on_left: bool) -> Element<'a, Msg> {
    hint(
        crate::shared::kit::edge_slide::handle(count, on_left, Msg::Library),
        t!(st_steps),
        Why::Keys(super::keys::of(super::keys::Cmd::StepsSidebar)),
        super::view::inward(on_left),
    )
}

/// The header: "Steps" and how many, then the pin (docked or over the picture) and the arrow
/// that slides the panel away, toward the edge it came from.
fn head<'a>(s: &Studio, count: usize) -> Element<'a, Msg> {
    use crate::shared::kit::icon::Icon;
    use crate::shared::kit::icon_button::{State, square};
    let on_left = s.sidebar_at_start() != dir::rtl();
    let tip = super::view::inward(on_left);
    let pinned = s.panels.pinned;
    let pin = hint(
        square(
            Icon::Pin.glyph(),
            State {
                active: pinned,
                focused: false,
            },
            Some(Msg::PinSteps),
        ),
        match pinned {
            true => t!(st_unpin_steps),
            false => t!(st_pin_steps),
        },
        Why::Icon,
        tip,
    );
    // the arrow points at the edge the panel hides toward
    let away = match on_left {
        true => Icon::Back,
        false => Icon::Forward,
    };
    let away = hint(
        square(away.glyph(), State::default(), Some(Msg::Library)),
        t!(st_hide_steps),
        Why::Keys(super::keys::of(super::keys::Cmd::StepsSidebar)),
        tip,
    );
    dir::row([
        crate::shared::kit::text::text(Role::CONTROL, t!(st_steps)).into(),
        crate::shared::kit::chip::badge(count.to_string()),
        Space::new().width(iced::Fill).into(),
        pin,
        away,
    ])
    .spacing(space::GAP / 2.0)
    .width(iced::Fill)
    .align_y(iced::Center)
    .into()
}

/// The Add step menu: Pictures, Paste and, with a picture open, This picture.
pub(super) fn add_rows(s: &Studio) -> Vec<(String, bool, Msg)> {
    let mut rows = vec![
        (t!(st_step_pictures).to_string(), false, Msg::PickFiles),
        (t!(st_step_paste).to_string(), false, Msg::PasteStep),
    ];
    if s.editor.is_some() {
        rows.push((
            t!(st_step_this).to_string(),
            false,
            Msg::Editor(Message::AddStep),
        ));
    }
    rows
}

/// The Steps strip's steps, four to a row: click = open on the canvas, drag =
/// reorder, x = delete; Copy all and Export at the foot.
fn steps_list<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    if steps.is_empty() && s.steps_pending == 0 {
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
            let shot = thumb::cell(
                steps[i].thumb(),
                place + 1,
                look,
                Some(Msg::StepDelete(i)),
                super::STEP_CELL,
            );
            let shot = mouse_area(shot)
                .on_press(Msg::StepPress(i))
                .on_enter(Msg::StepHover(i, true))
                .on_exit(Msg::StepHover(i, false));
            hint(shot, t!(st_step_tip), Why::Icon, tip_at)
        })
        .collect();
    let mut cells = cells;
    // the steps being made: their places, numbered, until they arrive
    cells.extend((0..s.steps_pending).map(|k| thumb::pending(n + k + 1, super::STEP_CELL)));
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
