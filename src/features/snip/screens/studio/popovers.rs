//! What the Studio's popovers hold (STUDIO v3.1: small, glance-only things): the
//! Actions list (Copy text, Translate, Ask AI, Edit text, Cut out), Position
//! (X / Y / W / H, the ratio lock, the order), the picture's info (from the
//! status bar's size chip) and More (the tool's own options when the bar is
//! too narrow). `kit::popover` places them.
use iced::widget::{Space, column, container};
use iced::{Center, Element, Fill};

use super::{Msg, Studio};
use crate::features::snip::annotate::{Message, Order};
use crate::features::snip::snip_more::Row;
use crate::shared::app_language::tr;
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::option_list;
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, space};
use crate::t;

/// The popovers (one open at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pop {
    Actions,
    Position,
    Info,
    More,
    /// The narrow bar's colour swatches.
    Colours,
    /// The custom colour: Snip's colour picker (HEX, eyedropper, presets), one component.
    Custom,
}

/// Edit text's glyph (lucide "pencil", in the subset font).
pub(super) const EDIT_TEXT_ICON: char = '\u{e1f8}';

/// The panel's rows, in the design's order (Edit text and Cut out follow them).
pub(super) const ACTS: [Row; 3] = [Row::CopyText, Row::Translate, Row::AskAi];

/// The popover's content width.
const WIDTH: f32 = 300.0;
const FIELD_LABEL: f32 = 14.0;

pub(crate) use crate::features::snip::annotate::tool_of;

impl Studio {
    /// The selection's X, Y, W, H (picture pixels), when something is selected.
    pub(super) fn frame(&self) -> Option<[i32; 4]> {
        let r = self.editor.as_ref()?.selection_bounds()?;
        Some([r.x, r.y, r.width, r.height].map(|v| v.round() as i32))
    }
}

/// The box fields: what is being typed, else the selection's numbers.
fn fields(s: &Studio, frame: [i32; 4]) -> [String; 4] {
    s.bounds_draft
        .clone()
        .unwrap_or_else(|| frame.map(|n| n.to_string()))
}

fn head<'a>(words: &'a str) -> Element<'a, Msg> {
    container(muted(Role::SECTION, words))
        .padding(iced::Padding {
            top: 4.0,
            ..iced::Padding::ZERO
        })
        .into()
}

/// X, Y, W, H (picture pixels): typed numbers move and resize the selection,
/// made one edit on Enter or when the user moves on; a field can be emptied.
fn frame_fields<'a>(f: [String; 4]) -> Element<'a, Msg> {
    let field = |k: &'a str, at: usize| {
        let input = crate::shared::kit::field::input("", &f[at], true, false)
            .on_input(move |typed: String| Msg::BoundsEdit(at, typed))
            .on_submit(Msg::BoundsCommit);
        crate::dir_row![sub(Role::ROW_DESC, k).width(FIELD_LABEL), input.width(Fill)]
            .spacing(space::GAP)
            .align_y(Center)
    };
    column![
        crate::dir_row![field("X", 0), field("Y", 1)].spacing(space::GAP),
        crate::dir_row![field("W", 2), field("H", 3)].spacing(space::GAP),
    ]
    .spacing(space::GAP)
    .into()
}

/// The order buttons.
fn order<'a>() -> Element<'a, Msg> {
    let b = |words, o| {
        button::button(
            label(words),
            Kind::Elevated,
            Some(Msg::Editor(Message::Order(o))),
        )
    };
    column![
        crate::dir_row![
            b(t!(st_to_front), Order::Front),
            b(t!(st_forward), Order::Forward)
        ]
        .spacing(space::GAP),
        crate::dir_row![
            b(t!(st_backward), Order::Backward),
            b(t!(st_to_back), Order::Back)
        ]
        .spacing(space::GAP),
    ]
    .spacing(space::GAP)
    .into()
}

/// Position: the object's frame and its place in the stack.
fn position(s: &Studio) -> Element<'_, Msg> {
    let Some(frame) = s.frame() else {
        return muted(Role::ROW_DESC, t!(st_hint_select)).into();
    };
    let lock = crate::dir_row![
        sub(Role::ROW_DESC, t!(st_lock_ratio)).width(Fill),
        button::toggle(s.lock, Some(Msg::Lock(!s.lock))),
    ]
    .align_y(Center);
    let pasted = (s.editor.as_ref())
        .and_then(|e| e.selection_style())
        .is_some_and(|st| st.opacity.is_some());
    let mut col = column![
        frame_fields(fields(s, frame)),
        lock,
        head(t!(st_order)),
        order()
    ]
    .spacing(space::GAP);
    if pasted {
        col = col.push(button::button(
            label(t!(st_merge)),
            Kind::Accent,
            Some(Msg::Editor(Message::Merge)),
        ));
    }
    col.into()
}

/// The size and the drawings' count.
fn picture_info(s: &Studio) -> Element<'_, Msg> {
    let Some(e) = &s.editor else {
        return Space::new().into();
    };
    let px = e.doc.view().size();
    let kv = |name: &'static str, value: String| {
        crate::dir_row![
            sub(Role::ROW_DESC, name),
            Space::new().width(Fill),
            text(Role::ROW_DESC, value)
        ]
        .width(Fill)
        .align_y(Center)
    };
    column![
        kv(
            t!(st_size),
            format!("{} × {}", px.width.round(), px.height.round())
        ),
        kv(t!(st_drawings), e.doc.items.len().to_string()),
    ]
    .spacing(space::GAP)
    .into()
}

/// Copy text, Translate, Ask AI (Snip More's rows, with the user's keys), Edit
/// text and Cut out.
fn actions(s: &Studio) -> Element<'_, Msg> {
    let acts = s.info();
    let mut rows = column![];
    for row in ACTS {
        let at = Row::ALL.iter().position(|r| *r == row).unwrap_or(0);
        let lead = match row {
            Row::AskAi => option_list::logo_mark(16),
            _ => option_list::glyph(row.glyph(), false),
        };
        rows = rows.push(option_list::action(option_list::ActionRow {
            lead,
            label: tr(row.label(acts.translated)),
            external: row == Row::AskAi && acts.browser,
            caps: acts.caps[at].clone(),
            reading: s.busy() == Some(row),
            focused: false,
            press: Some(Msg::Act(row)),
        }));
    }
    let editing = s.edit.is_some();
    rows = rows.push(option_list::action(option_list::ActionRow {
        // lucide "pencil": Copy text keeps the "type" T (F15: they shared it)
        lead: option_list::glyph(EDIT_TEXT_ICON, false),
        label: tr(crate::shared::app_language::S::st_edit_text),
        external: false,
        caps: Vec::new(),
        reading: s.edit.as_ref().is_some_and(|x| x.lines.is_none()),
        focused: false,
        press: (!editing && !acts.translated).then_some(Msg::EditText),
    }));
    rows = rows.push(option_list::action(option_list::ActionRow {
        lead: option_list::glyph('\u{e151}', false), // lucide "scissors"
        label: tr(crate::shared::app_language::S::st_cut_out),
        external: false,
        caps: Vec::new(),
        reading: s.cut_phase() == super::CutPhase::Working,
        focused: false,
        press: (!editing && !acts.translated && s.cut_phase() == super::CutPhase::Idle)
            .then_some(Msg::CutOut),
    }));
    rows = rows.push(super::text_select::row(s));
    // A selected shape, pen stroke or arrow: a Hide over it (Blur rework)
    if let Some(both) = s.editor.as_ref().and_then(|e| e.hideable()) {
        for (label, outline) in [
            (crate::shared::app_language::S::sn_hide_shape, false),
            (crate::shared::app_language::S::sn_hide_outline, true),
        ]
        .into_iter()
        .filter(|&(_, outline)| both || !outline)
        {
            rows = rows.push(option_list::action(option_list::ActionRow {
                lead: option_list::glyph('\u{e0b8}', false), // the Hide drop
                label: tr(label),
                external: false,
                caps: Vec::new(),
                reading: false,
                focused: false,
                press: Some(Msg::Editor(Message::HideShape(outline))),
            }));
        }
    }
    rows.into()
}

/// The open popover's content (`More`: the tool's own options, from the bar).
pub fn content<'a>(s: &'a Studio, pop: Pop) -> Element<'a, Msg> {
    let body: Element<'a, Msg> = match pop {
        Pop::Actions => actions(s),
        Pop::Position => position(s),
        Pop::Info => picture_info(s),
        Pop::Colours => match &s.editor {
            Some(e) => {
                let n = crate::features::snip::screens::options_row::SWATCHES;
                let swatches = super::options_bar::colour_group(e, n);
                let custom = super::options_bar::custom_colour(s, e, true);
                crate::shared::kit::dir::row([swatches, custom])
                    .spacing(crate::shared::kit::option_bar::GROUP_GAP)
                    .into()
            }
            None => Space::new().into(),
        },
        Pop::More => match &s.editor {
            Some(e) => super::options_bar::tool_group(e),
            None => Space::new().into(),
        },
        Pop::Custom => match &s.editor {
            Some(e) => Element::from(
                crate::features::snip::screens::snip_toolbar::toolbar::colour_panel(e),
            )
            .map(Msg::Editor),
            None => Space::new().into(),
        },
    };
    let width = match pop {
        Pop::Custom => crate::shared::kit::colour::PICKER_W,
        _ => WIDTH,
    };
    container(body).width(width).into()
}

/// Where each popover's opener was last drawn (the popover hangs from it).
#[derive(Clone, Default)]
pub struct Anchors {
    actions: crate::shared::kit::popover::Anchor,
    position: crate::shared::kit::popover::Anchor,
    more: crate::shared::kit::popover::Anchor,
    colours: crate::shared::kit::popover::Anchor,
    custom: crate::shared::kit::popover::Anchor,
    info: crate::shared::kit::popover::Anchor,
}

impl Anchors {
    pub fn of(&self, pop: Pop) -> &crate::shared::kit::popover::Anchor {
        match pop {
            Pop::Actions => &self.actions,
            Pop::Position => &self.position,
            Pop::More => &self.more,
            Pop::Colours => &self.colours,
            Pop::Custom => &self.custom,
            Pop::Info => &self.info,
        }
    }
}

impl Pop {
    /// The card's size (an estimate: the card is placed by it) and whether it hangs by its
    /// end edge from its opener (the bar's end buttons) or by its start edge.
    pub fn card(self) -> (iced::Size, bool) {
        let pad = 2.0 * (space::GAP + 4.0) + crate::shared::kit::popover::X_WIDTH;
        match self {
            Pop::Actions => (iced::Size::new(WIDTH + pad, 5.0 * 32.0 + pad), true),
            Pop::Position => (iced::Size::new(WIDTH + pad, 270.0), true),
            Pop::More => (iced::Size::new(WIDTH + pad, 160.0), true),
            Pop::Colours => (iced::Size::new(8.0 * 30.0 + 40.0 + pad, 32.0 + pad), false),
            // the picker alone, as in Snip: no card around it
            Pop::Custom => (
                iced::Size::new(crate::shared::kit::colour::PICKER_W, 400.0),
                false,
            ),
            Pop::Info => (iced::Size::new(WIDTH + pad, 70.0), false),
        }
    }
}
