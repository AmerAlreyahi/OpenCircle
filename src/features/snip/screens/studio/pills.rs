//! Round 6's floating bottom pills over the stage (no status bar): bottom-start
//! the size chip (drags the picture out); bottom-end the zoom (- % +, Fit), Save
//! and Copy (the whole picture, accent). Style E trays with raised segments.
use iced::widget::tooltip::Position;
use iced::widget::{Space, container, opaque, stack};
use iced::{Bottom, Center, Element, Fill};

use super::view::{MINUS, PLUS};
use super::{Msg, PILL_EDGE, Studio, keys};
use crate::features::snip::annotate::{Annotator, Message};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::option_bar::{glyph_cell, text_cell, value};
use crate::shared::kit::surface::{Join, segment, segment_row, tray};
use crate::shared::kit::tokens::{radius, space, widget_e};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{chip, dir};
use crate::t;

/// The pills' ids (the tests read their bounds).
pub(super) const SIZE_PILL: &str = "studio-pill-size";
pub(super) const ACTIONS_PILL: &str = "studio-pill-actions";

/// `stage` with the two pills floating over its bottom corners.
pub fn over<'a>(s: &'a Studio, e: &'a Annotator, below: Element<'a, Msg>) -> Element<'a, Msg> {
    let px = e.doc.view().size();
    let size_label = format!("{} × {}", px.width.round(), px.height.round());
    let size = tray(
        false,
        radius::FULL,
        Join::None,
        segment_row([chip::size_pill(
            size_label,
            false,
            Msg::Editor(Message::DragOut),
        )]),
    )
    .id(SIZE_PILL)
    .padding([widget_e::TRAY_PAD_Y, widget_e::TRAY_PAD_X]);
    let percent = s.percent().map_or_else(String::new, |p| format!("{p} %"));
    let zoom = segment_row([
        glyph_cell(MINUS, false, Some(Msg::ZoomOut)),
        value(percent),
        glyph_cell(PLUS, false, Some(Msg::ZoomIn)),
        text_cell(t!(st_fit), false, Some(Msg::Fit)),
    ]);
    let (save_words, copy_words) = (t!(c_save), t!(c_copy));
    let save = segment(hint(
        button::labelled(
            toolbar::SAVE,
            save_words,
            button::labelled_width(save_words),
            Kind::Quiet,
            false,
            Some(Msg::Editor(Message::Save)),
        ),
        t!(c_save),
        Why::Keys(keys::SAVE),
        Position::Top,
    ));
    let copy = hint(
        button::labelled(
            toolbar::COPY,
            copy_words,
            button::labelled_width(copy_words),
            Kind::Accent,
            false,
            Some(Msg::Editor(Message::Copy)),
        ),
        t!(st_copy_picture),
        Why::Keys(keys::COPY_PICTURE),
        Position::Top,
    );
    let actions = tray(
        false,
        radius::FULL,
        Join::None,
        dir::row([zoom, save.into(), copy])
            .spacing(space::GAP)
            .align_y(Center),
    )
    .id(ACTIONS_PILL)
    .padding([widget_e::TRAY_PAD_Y, widget_e::TRAY_PAD_X]);
    let line =
        crate::dir_row![opaque(size), Space::new().width(Fill), opaque(actions)].align_y(Center);
    let floating = container(line)
        .padding(PILL_EDGE)
        .width(Fill)
        .height(Fill)
        .align_y(Bottom);
    stack![below, floating].into()
}
