//! Extensions (Settings round 2, "Three tiers"): the optional tools added on demand. Nothing ships as an
//! extension yet, so the page says so plainly (no "Coming soon", no invented list): the three tiers
//! and an empty state. A tool joins as one card here when the first one exists.
use crate::shared::kit::icon::Icon;
use iced::widget::{column, container};
use iced::{Center, Element, Fill};

use super::super::state::{Message, State};
use crate::shared::app_language::S;
use crate::shared::kit::icon::glyph;
use crate::shared::kit::section::section;
use crate::shared::kit::text::{sub, text};
use crate::shared::kit::themes::c;
use crate::shared::kit::tokens::{Role, settings, size, space};
use crate::t;

pub fn view(_: &State) -> Element<'_, Message> {
    let tier = |name: S, note: S| {
        crate::dir_row![
            text(Role::CONTROL, crate::shared::app_language::tr(name)),
            sub(Role::ROW_DESC, crate::shared::app_language::tr(note))
        ]
        .spacing(space::GAP)
        .align_y(Center)
    };
    let tiers = crate::dir_row![
        tier(S::ext_tier_core, S::ext_tier_core_d),
        glyph(Icon::Forward.glyph(), size::ICON_SMALL).color(c().text_muted),
        tier(S::ext_tier_ext, S::ext_tier_ext_d),
        glyph(Icon::Forward.glyph(), size::ICON_SMALL).color(c().text_muted),
        tier(S::ext_tier_labs, S::ext_tier_labs_d),
    ]
    .spacing(space::GAP * 2.0)
    .align_y(Center);
    let none = container(
        column![
            glyph(Icon::Puzzle.glyph(), settings::EMPTY_GLYPH).color(c().accent),
            text(Role::CARD_HEADING, t!(ext_none_title)),
            sub(Role::ROW_DESC, t!(ext_none_desc)),
        ]
        .spacing(space::GAP)
        .align_x(Center),
    )
    .center_x(Fill)
    .padding([settings::EMPTY_PAD_Y, 0.0]);
    column![
        section(None, None, || vec![tiers.into()]),
        section(None, None, || vec![none.into()])
    ]
    .spacing(settings::SECTION_GAP)
    .into()
}
