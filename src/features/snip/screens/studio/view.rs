//! OpenCircle Studio's window, from kit parts only (layout v2): header (the panel
//! toggles before the caption buttons) · library | rail | options bar, stage,
//! status bar | picture panel. The editor's own canvas and option rows
//! (`snip_toolbar`) are reused as they are: one editor, two placements.
use iced::advanced::layout::Layout;
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Shell};
use iced::widget::{Space, column, container, tooltip::Position};
use iced::{Center, Element, Event, Fill, Length, Rectangle, Vector, mouse};

use super::{Msg, Studio};
use crate::features::snip::annotate::{Annotator, Group, Message, Tool, info, style};
use crate::features::snip::screens::snip_toolbar::canvas::{self, ScreenMap};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::shared::app_language::tr;
use crate::shared::kit::button::{self, Kind, cancel, label};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, radius, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::wrap::{Hook, wrap};
use crate::shared::kit::{chip, colour, dir, divider, keycap, scroll, thumb};
use crate::t;

const PANEL_LEFT: char = '\u{e12d}'; // lucide "panel-left"
const PANEL_RIGHT: char = '\u{e435}'; // lucide "panel-right"
const OPEN: char = '\u{e246}'; // lucide "folder-open"
const PASTE: char = '\u{e3eb}'; // lucide "clipboard-paste"
const PLUS: char = '\u{e140}'; // lucide "plus"
const MINUS: char = '\u{e11f}'; // lucide "minus"

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[PANEL_LEFT, PANEL_RIGHT, OPEN, PASTE, PLUS, MINUS];

/// A tool's options take the editor panel's column, side by side.
const OPTION_W: f32 = size::PANEL - 2.0 * space::PANEL;
/// The colour swatches on the options bar (the editor's first ones).
const SWATCHES: usize = 8;

pub fn view(s: &Studio, maximised: bool) -> Element<'_, Msg> {
    let _dir = dir::scope(crate::shared::app_language::rtl());
    let p = s.panels;
    let toggles = crate::dir_row![
        toggle(PANEL_LEFT, p.library, Msg::Library, "Ctrl+["),
        toggle(PANEL_RIGHT, p.options, Msg::Panel, "Ctrl+]"),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let header = crate::shared::kit::header::regular(
        crate::shared::kit::logo::logo(16),
        t!(st_title),
        toggles,
        Some(maximised),
        Msg::Caption,
    );
    let mut parts: Vec<Element<'_, Msg>> = Vec::new();
    if p.library {
        parts.push(library(s));
        parts.push(divider::vertical(Fill));
    }
    match &s.editor {
        Some(e) => {
            parts.push(rail(e));
            parts.push(divider::vertical(Fill));
            let work = column![
                options_bar(e),
                divider::divider(),
                stage(s, e),
                divider::divider(),
                status_bar(s, e),
            ];
            parts.push(container(work).width(Fill).height(Fill).into());
        }
        None => parts.push(empty(s)),
    }
    if p.options {
        parts.push(divider::vertical(Fill));
        parts.push(picture_panel(s));
    }
    let main = column![header, divider::divider(), dir::row(parts).height(Fill)];
    // the question is a card over the window: the picture under it takes no press
    let main: Element<'_, Msg> = match s.asking() {
        true => iced::widget::stack![
            main,
            iced::widget::opaque(container(question()).center(Fill))
        ]
        .into(),
        false => main.into(),
    };
    let rim = if maximised { 0.0 } else { 1.0 };
    crate::shared::kit::focus::scope(
        surface(Material::Window, 0.0, main)
            .padding(rim)
            .width(Fill)
            .height(Fill),
    )
}

/// A panel's toggle: raised while its panel shows.
fn toggle<'a>(glyph: char, shown: bool, m: Msg, keys: &'a str) -> Element<'a, Msg> {
    let state = State {
        active: shown,
        focused: false,
    };
    let words = match (glyph == PANEL_LEFT, shown) {
        (true, true) => t!(st_hide_library),
        (true, false) => t!(st_show_library),
        (false, true) => t!(st_hide_panel),
        (false, false) => t!(st_show_panel),
    };
    hint(
        icon_button::square(glyph, state, Some(m)),
        words,
        Why::Keys(keys),
        Position::Bottom,
    )
}

/// Every tool on the rail, groups split by a hairline; Undo / Redo at its foot.
fn rail(e: &Annotator) -> Element<'_, Msg> {
    let base = info(e.tool).slot;
    let mut tools: Vec<Element<'_, Msg>> = Vec::new();
    for (i, g) in Group::ALL.into_iter().enumerate() {
        if i > 0 {
            tools.push(
                container(divider::divider())
                    .width(size::ICON_BUTTON)
                    .into(),
            );
        }
        let mut shown: Vec<_> = g.tools().collect();
        if g == Group::Select {
            shown.push(info(Tool::Crop)); // the Studio has no area handles: Crop is a tool
        }
        for t in shown {
            let state = State {
                active: base == t.tool,
                focused: false,
            };
            let press = Some(Message::Tool(t.tool));
            let button = icon_button::square(t.icon, state, press).map(Msg::Editor);
            let name = match t.tool {
                Tool::Crop => tr(t.name),
                _ => toolbar::tip(t),
            };
            tools.push(hint(button, name, Why::Icon, Position::Right));
        }
    }
    let (done, all) = e.doc.steps();
    let undo = (done > 0).then_some(Msg::Editor(Message::Undo));
    let redo = (done < all).then_some(Msg::Editor(Message::Redo));
    let foot = column![
        hint(
            icon_button::square(toolbar::UNDO, State::default(), undo),
            t!(sn_undo),
            Why::Keys("Ctrl+Z"),
            Position::Right,
        ),
        hint(
            icon_button::square(toolbar::REDO, State::default(), redo),
            t!(sn_redo),
            Why::Keys("Ctrl+Y"),
            Position::Right,
        ),
    ]
    .spacing(space::GAP);
    let tools = scroll::quiet(column(tools).spacing(space::GAP).align_x(Center)).height(Fill);
    container(column![tools, foot].spacing(space::GAP).align_x(Center))
        .padding([space::SIDEBAR_PAD, 0.0])
        .center_x(super::RAIL)
        .height(Fill)
        .into()
}

/// The tool's name and key, the colours, then its options (the editor panel's
/// own rows), always shown.
fn options_bar(e: &Annotator) -> Element<'_, Msg> {
    let base = info(info(e.tool).slot);
    let mut items: Vec<Element<'_, Message>> = vec![
        crate::dir_row![
            text(Role::ROW_TITLE, tr(base.name)),
            container(keycap::keycap(base.key.to_ascii_uppercase().to_string()))
                .width(size::KEYCAP_H),
        ]
        .spacing(space::GAP)
        .align_y(Center)
        .into(),
    ];
    if !matches!(base.tool, Tool::Select | Tool::Crop | Tool::Eraser) {
        let swatches = (style::ANNOTATION.iter().take(SWATCHES)).map(|c| {
            let rgba = c.into_rgba8();
            colour::swatch(*c, rgba == e.colour, Some(Message::Colour(rgba)))
        });
        items.push(dir::row(swatches).spacing(space::TRACK_GAP).into());
    }
    let options = toolbar::tool_options(e, base.tool);
    items.extend((options.into_iter()).map(|o| container(o).width(OPTION_W).center_y(Fill).into()));
    let line = dir::row(items)
        .spacing(space::PAGE[1] + space::PAGE[1])
        .align_y(Center);
    let bar = container(line)
        .padding([0.0, space::PAGE[1]])
        .height(super::OPTIONS);
    Element::from(scroll::across(bar).width(Fill)).map(Msg::Editor)
}

/// The picture, fitted or zoomed (both ways scrollable past the stage).
fn stage<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let drawn = canvas::canvas(e, 1.0, ScreenMap::ONE).map(Msg::Editor);
    let (shown, room) = (e.size(), s.stage());
    // a px less than the stage: no scrollbar for a fitted picture
    let content = container(drawn)
        .center_x((room.width - 2.0).max(shown.width))
        .center_y((room.height - 2.0).max(shown.height));
    let area = scroll::both(content)
        .id(super::CANVAS)
        .on_scroll(|v| Msg::Scrolled(Vector::new(v.absolute_offset().x, v.absolute_offset().y)));
    surface(Material::Recessed, 0.0, wrap(area, CtrlWheel(s.ctrl)))
        .width(Fill)
        .height(Fill)
        .into()
}

/// Ctrl + wheel over the stage zooms round the pointer instead of scrolling
/// (the area under it never sees that wheel).
struct CtrlWheel(bool);

/// One wheel notch in pixels (touchpads send pixels).
const WHEEL_PX: f32 = 40.0;

impl Hook<Msg> for CtrlWheel {
    fn update(
        &mut self,
        content: &mut Element<'_, Msg>,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        viewport: &Rectangle,
    ) {
        if let (true, Event::Mouse(mouse::Event::WheelScrolled { delta })) = (self.0, event)
            && let Some(at) = cursor.position_in(layout.bounds())
        {
            let notches = match delta {
                mouse::ScrollDelta::Lines { y, .. } => *y,
                mouse::ScrollDelta::Pixels { y, .. } => *y / WHEEL_PX,
            };
            shell.publish(Msg::Wheel { at, notches });
            shell.capture_event();
            return;
        }
        content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
}

/// The size, the zoom, Fit, Save and Copy.
fn status_bar<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let px = e.doc.view().size();
    let size_label = format!("{} × {}", px.width.round(), px.height.round());
    let percent = s.percent().map_or_else(String::new, |p| format!("{p} %"));
    let zoom_button = |glyph, m, words, keys| {
        hint(
            icon_button::square(glyph, State::default(), Some(m)),
            words,
            Why::Keys(keys),
            Position::Top,
        )
    };
    let save = button::tool(
        toolbar::SAVE,
        Kind::Elevated,
        false,
        Some(Msg::Editor(Message::Save)),
    );
    let copy = button::tool(
        toolbar::COPY,
        Kind::Accent,
        false,
        Some(Msg::Editor(Message::Copy)),
    );
    let line = crate::dir_row![
        chip::readout(size_label, false, None),
        Space::new().width(Fill),
        zoom_button(MINUS, Msg::ZoomOut, t!(st_zoom_out), "Ctrl+-"),
        container(sub(Role::KEYCAP, percent)).center_x(56.0),
        zoom_button(PLUS, Msg::ZoomIn, t!(st_zoom_in), "Ctrl+="),
        hint(
            button::button(label(t!(st_fit)), Kind::Quiet, Some(Msg::Fit)),
            t!(st_fit),
            Why::Keys("Ctrl+0"),
            Position::Top,
        ),
        divider::vertical(size::ICON_BUTTON - 10.0),
        hint(save, t!(c_save), Why::Keys("Ctrl+S"), Position::Top),
        hint(copy, t!(c_copy), Why::Keys("Ctrl+C"), Position::Top),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    container(line)
        .padding([0.0, space::PAGE[1]])
        .center_y(super::STATUS)
        .width(Fill)
        .into()
}

/// Opened alone: drop a picture, Open, Paste (the drop zone lights while a
/// file is over the window).
fn empty(s: &Studio) -> Element<'_, Msg> {
    let labelled = |glyph, words: &'static str, kind, m| {
        let w = button::labelled_width(words);
        button::labelled(glyph, words, w, kind, false, Some(m))
    };
    let open = labelled(OPEN, t!(st_open), Kind::Accent, Msg::Open);
    let paste = labelled(PASTE, t!(st_paste), Kind::Elevated, Msg::Paste);
    let card = column![
        chip::emblem(Icon::Picture),
        text(Role::CARD_HEADING, t!(st_empty_title)),
        sub(Role::ROW_DESC, t!(st_empty_hint)),
        crate::dir_row![
            hint(open, t!(st_open), Why::Keys("Ctrl+O"), Position::Bottom),
            hint(paste, t!(st_paste), Why::Keys("Ctrl+V"), Position::Bottom),
        ]
        .spacing(space::GAP),
    ]
    .spacing(space::PAGE[0])
    .align_x(Center);
    let zone = match s.hovered {
        true => Material::Held, // its accent edge: "drop it here"
        false => Material::Recessed,
    };
    container(
        surface(zone, radius::CARD, container(card).center(Fill))
            .width(Fill)
            .height(Fill),
    )
    .padding(space::PAGE[1])
    .width(Fill)
    .height(Fill)
    .into()
}

/// The library: Recent captures (two to a row), or why there are none.
fn library(s: &Studio) -> Element<'_, Msg> {
    let mut list = column![text(Role::SECTION, t!(st_recent))].spacing(space::PAGE[0]);
    if !crate::shared::config::current().snip.history {
        list = list.push(muted(Role::ROW_DESC, t!(st_recent_off)));
    } else if s.recent.is_empty() {
        list = list.push(muted(Role::ROW_DESC, t!(st_recent_none)));
    }
    for pair in s.recent.chunks(2) {
        let shots = pair
            .iter()
            .map(|(path, picture)| thumb::picture(picture.clone(), Msg::OpenRecent(path.clone())));
        list = list.push(dir::row(shots).spacing(space::GAP));
    }
    side(list, super::LIBRARY)
}

/// The picture panel: what the open picture is.
fn picture_panel(s: &Studio) -> Element<'_, Msg> {
    let mut list = column![text(Role::SECTION, t!(st_picture))].spacing(space::PAGE[0]);
    if let Some(e) = &s.editor {
        let px = e.doc.view().size();
        let line = |name, value: String| {
            crate::dir_row![
                sub(Role::ROW_DESC, name),
                Space::new().width(Fill),
                text(Role::ROW_DESC, value)
            ]
            .width(Fill)
        };
        list = list
            .push(line(
                t!(st_size),
                format!("{} × {}", px.width.round(), px.height.round()),
            ))
            .push(line(t!(st_drawings), e.doc.items.len().to_string()));
    }
    side(list, super::PANEL)
}

fn side(list: iced::widget::Column<'_, Msg>, width: f32) -> Element<'_, Msg> {
    let list = container(list).padding(space::PAGE[1]).width(Fill);
    container(scroll::scroll(list).height(Fill))
        .width(Length::Fixed(width))
        .height(Fill)
        .into()
}

/// Closing or opening another picture with unsaved drawings: Don't save /
/// Cancel / Save.
fn question<'a>() -> Element<'a, Msg> {
    container(crate::shared::kit::card::card(question_row()))
        .width(Length::Fixed(560.0))
        .into()
}

fn question_row<'a>() -> Element<'a, Msg> {
    let b = |s, kind, m| button::button(label(s), kind, Some(m));
    crate::dir_row![
        text(Role::ROW_TITLE, t!(st_ask_close)),
        Space::new().width(Fill),
        b(t!(st_dont_save), Kind::Elevated, Msg::DontSave),
        cancel(label(t!(c_cancel)), Kind::Elevated, Msg::Cancel),
        b(t!(c_save), Kind::Accent, Msg::SaveFirst),
    ]
    .spacing(space::GAP)
    .align_y(Center)
    .into()
}
