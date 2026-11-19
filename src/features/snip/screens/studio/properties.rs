//! Properties (right panel, STUDIO_V3_SPEC §5, boards 2 to 5): its own 40 px
//! header, then what is selected: nothing = the picture (its size, the picture
//! actions, the save and export defaults); one object; several; a pasted
//! picture. The numbers are read out; edits that need the editing model
//! (X / Y / W / H, order, Duplicate) are dimmed until it lands.
use iced::widget::{Space, column, container};
use iced::{Center, Element, Fill};

use super::{Msg, Studio};
use crate::features::snip::annotate::{Message, Tool, info};
use crate::features::snip::export;
use crate::features::snip::snip_more::Row;
use crate::shared::app_language::{fmt, tr};
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::{option_list, segmented, side_panel, slider};
use crate::t;

/// The panel's rows, in the design's order (Edit text and Cut out follow them).
pub(super) const ACTS: [Row; 3] = [Row::CopyText, Row::Translate, Row::AskAi];

/// A read-out row's height, the panel's side padding.
const KV_H: f32 = 22.0;
const PAD: f32 = 12.0;

/// A heading line (the object's name).
fn title<'a>(words: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Msg> {
    text(Role::ROW_TITLE, words).into()
}

/// What the panel is about.
#[cfg_attr(not(test), allow(dead_code))] // ponytail: wired when W86's / W88's APIs land
pub enum Subject {
    Picture,
    Object(ObjectInfo),
    Many { count: usize },
    Pasted(PastedInfo),
}

pub struct ObjectInfo {
    pub tool: Tool,
    /// X, Y, W, H in picture pixels.
    pub frame: [i32; 4],
}

pub struct PastedInfo {
    pub frame: [i32; 4],
    /// Opacity, per cent.
    pub opacity: u8,
}

fn kv<'a>(name: &'a str, value: String) -> Element<'a, Msg> {
    crate::dir_row![
        sub(Role::ROW_DESC, name),
        Space::new().width(Fill),
        text(Role::ROW_DESC, value)
    ]
    .width(Fill)
    .height(KV_H)
    .align_y(Center)
    .into()
}

fn head<'a>(words: &'a str) -> Element<'a, Msg> {
    container(muted(Role::SECTION, words))
        .padding(iced::Padding {
            top: 8.0,
            ..iced::Padding::ZERO
        })
        .into()
}

/// X, Y, W, H as four read-outs.
fn frame<'a>(f: [i32; 4]) -> Element<'a, Msg> {
    let cell = |k: &'a str, v: i32| kv(k, v.to_string());
    column![
        cell("X", f[0]),
        cell("Y", f[1]),
        cell("W", f[2]),
        cell("H", f[3])
    ]
    .into()
}

/// The order buttons (dimmed: they act through the editing model).
fn order<'a>() -> Element<'a, Msg> {
    let b = |words| button::button(label(words), Kind::Elevated, None);
    column![
        crate::dir_row![b(t!(st_to_front)), b(t!(st_forward))].spacing(space::GAP),
        crate::dir_row![b(t!(st_backward)), b(t!(st_to_back))].spacing(space::GAP),
    ]
    .spacing(space::GAP)
    .into()
}

fn duplicate_delete<'a>(delete: Option<Msg>) -> Element<'a, Msg> {
    crate::dir_row![
        button::button(label(t!(st_duplicate)), Kind::Elevated, None),
        button::button(label(t!(st_delete)), Kind::Elevated, delete),
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
        lead: option_list::glyph(info(Tool::Text).icon, false),
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
    rows.into()
}

/// Format, Quality (JPG), Size: the export defaults, then Save as… / Add to steps.
fn export_defaults(s: &Studio) -> Element<'_, Msg> {
    let how = s.export;
    let formats: Vec<_> = export::Format::ALL.iter().map(|&f| (f, f.name())).collect();
    let mut col = column![segmented::inline(
        &formats,
        how.format,
        how.format,
        1.0,
        Msg::ExportFormat
    )]
    .spacing(space::GAP);
    if how.format.has_quality() {
        col = col.push(
            crate::dir_row![
                slider::bare(1..=100, how.quality, 140.0, false, Msg::ExportQuality),
                sub(Role::KEYCAP, format!("{} %", how.quality)),
            ]
            .spacing(space::GAP)
            .align_y(Center),
        );
    }
    col = col.push(super::view::sizes(
        &export::SIZES,
        how.max_edge,
        Msg::ExportSize,
    ));
    col = col.push(
        crate::dir_row![
            button::button(label(t!(st_save_as)), Kind::Elevated, Some(Msg::Export)),
            button::button(
                label(t!(st_add_steps)),
                Kind::Elevated,
                Some(Msg::Editor(Message::AddStep))
            ),
        ]
        .spacing(space::GAP),
    );
    col.into()
}

/// The panel for `subject`.
pub fn view(s: &Studio, subject: Subject) -> Element<'_, Msg> {
    let mut body = column![].spacing(space::GAP);
    match &subject {
        Subject::Picture => {
            if let Some(e) = &s.editor {
                let px = e.doc.view().size();
                body = body
                    .push(kv(
                        t!(st_size),
                        format!("{} × {}", px.width.round(), px.height.round()),
                    ))
                    .push(kv(t!(st_drawings), e.doc.items.len().to_string()));
            }
            body = body
                .push(head(t!(st_actions)))
                .push(actions(s))
                .push(head(t!(st_save_export)))
                .push(export_defaults(s));
        }
        Subject::Object(o) => {
            let t = info(o.tool);
            body = body
                .push(title(tr(t.name)))
                .push(frame(o.frame))
                .push(head(t!(st_order)))
                .push(order())
                .push(duplicate_delete(Some(Msg::Editor(Message::Delete))))
                .push(head(t!(st_actions)))
                .push(actions(s));
        }
        Subject::Many { count } => {
            body = body
                .push(title(fmt(
                    crate::shared::app_language::S::st_n_objects,
                    &[count],
                )))
                .push(head(t!(st_order)))
                .push(order())
                .push(duplicate_delete(Some(Msg::Editor(Message::Delete))));
        }
        Subject::Pasted(p) => {
            body = body
                .push(title(t!(st_pasted)))
                .push(frame(p.frame))
                .push(kv(t!(st_opacity), format!("{} %", p.opacity)))
                .push(head(t!(st_order)))
                .push(order())
                .push(button::button(label(t!(st_merge)), Kind::Accent, None));
        }
    }
    let list = container(body).padding([space::GAP, PAD]).width(Fill);
    side_panel::panel(
        super::panel_width(),
        t!(st_properties),
        super::view::PANEL_RIGHT,
        Msg::Panel,
        t!(st_hide_panel),
        list.into(),
    )
}

impl Studio {
    /// What Properties is about: the selected object, else the picture.
    pub(super) fn subject(&self) -> Subject {
        let Some(e) = &self.editor else {
            return Subject::Picture;
        };
        let Some(item) = e.doc.selected.and_then(|i| e.doc.items.get(i)) else {
            return Subject::Picture;
        };
        let f = item.frame();
        Subject::Object(ObjectInfo {
            tool: super::library::tool_of(&item.shape),
            frame: [f.x, f.y, f.width, f.height].map(|v| v.round() as i32),
        })
    }
}
