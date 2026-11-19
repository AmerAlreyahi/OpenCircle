//! OpenCircle Studio's window, from kit parts only (layout v2): header (the panel
//! toggles before the caption buttons) · library | rail | options bar, stage,
//! status bar | picture panel. The editor's own canvas and option rows
//! (`snip_toolbar`) are reused as they are: one editor, two placements.
use crate::shared::config::{StudioBackdrop, StudioWheel};
use iced::advanced::layout::Layout;
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Shell};
use iced::widget::{Space, column, container, mouse_area, tooltip::Position};
use iced::{Center, Element, Event, Fill, Length, Rectangle, Size, Vector, mouse};
use std::sync::Arc;

use super::{Msg, Studio, Walk};
use crate::features::snip::annotate::{Annotator, Group, Message, Tool, info, style};
use crate::features::snip::export;
use crate::features::snip::screens::snip_toolbar::canvas::{self, ScreenMap};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::features::snip::screens::steps::Step;
use crate::shared::app_language::tr;
use crate::shared::kit::button::{self, Kind, cancel, label};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub, text};
use crate::shared::kit::tokens::{Role, radius, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::wrap::{Hook, wrap};
use crate::shared::kit::{chip, colour, dir, divider, dropdown, scroll, segmented, slider, thumb};
use crate::t;

pub(super) const PANEL_LEFT: char = '\u{e12d}'; // lucide "panel-left"
pub(super) const PANEL_RIGHT: char = '\u{e435}'; // lucide "panel-right"
const OPEN: char = '\u{e246}'; // lucide "folder-open"
const PASTE: char = '\u{e3eb}'; // lucide "clipboard-paste"
pub(super) const PLUS: char = '\u{e140}'; // lucide "plus"
pub(super) const MINUS: char = '\u{e11f}'; // lucide "minus"

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[PANEL_LEFT, PANEL_RIGHT, OPEN, PASTE, PLUS, MINUS];

/// A tool's options take the editor panel's column, side by side.
const OPTION_W: f32 = size::PANEL - 2.0 * space::PANEL;
/// The colour swatches on the options bar (the editor's first ones).
const SWATCHES: usize = 8;

/// `steps`: the Steps strip's steps (the Library's Steps section shows them).
pub fn view<'a>(s: &'a Studio, steps: &'a [Arc<Step>], maximised: bool) -> Element<'a, Msg> {
    let _dir = dir::scope(crate::shared::app_language::rtl());
    let header = crate::shared::kit::header::with_menu(
        crate::shared::kit::logo::logo(16),
        Some(super::menu_bar::bar(s)),
        &s.title,
        s.dirty(),
        Space::new(),
        Some(maximised),
        Msg::Caption,
    );
    let mut parts: Vec<Element<'_, Msg>> = Vec::new();
    if s.library_shown() {
        parts.push(super::library::view(s, steps));
        parts.push(divider::vertical(Fill));
    }
    match &s.editor {
        Some(e) => {
            parts.push(rail(e));
            parts.push(divider::vertical(Fill));
            let work = column![
                match (&s.edit, s.comparing()) {
                    (Some(x), _) => edit_bar(x),
                    (None, Some(before)) => cut_bar(before),
                    (None, None) => super::options_bar::view(e),
                },
                divider::divider(),
                stage(s, e),
                divider::divider(),
                super::status_bar::view(s, e),
            ];
            parts.push(container(work).width(Fill).height(Fill).into());
        }
        None => parts.push(empty(s)),
    }
    if s.panel_shown() {
        parts.push(divider::vertical(Fill));
        parts.push(super::properties::view(s, s.subject()));
    }
    let body: Element<'_, Msg> = dir::row(parts).height(Fill).into();
    // narrow: the Library over the stage, above the status bar
    let body: Element<'_, Msg> = match (s.narrow(), s.overlay) {
        (true, true) => iced::widget::stack![
            body,
            container(super::library::view(s, steps))
                .height(Fill)
                .padding(iced::Padding {
                    bottom: super::STATUS + 1.0,
                    ..iced::Padding::ZERO
                })
        ]
        .into(),
        _ => body,
    };
    let main = column![header, divider::divider(), body];
    let main = super::menu_bar::over(main.into(), s);
    let main: Element<'_, Msg> = match &s.context {
        Some(c) => iced::widget::stack![
            main,
            iced::widget::opaque(
                iced::widget::mouse_area(
                    iced::widget::pin(super::sheets::context(&c.entries))
                        .x(c.at.x)
                        .y(c.at.y)
                )
                .on_press(Msg::Cancel)
                .on_right_press(Msg::Cancel)
            )
        ]
        .into(),
        None => main,
    };
    let main: Element<'_, Msg> = match s.shortcuts {
        true => iced::widget::stack![
            main,
            iced::widget::opaque(container(super::sheets::shortcuts(s)).center(Fill))
        ]
        .into(),
        false => main,
    };
    // after a crash: the kept copy
    let main: Element<'_, Msg> = match s.restorable.as_ref().filter(|_| !s.later) {
        Some(r) => iced::widget::stack![
            main,
            iced::widget::opaque(
                container(super::sheets::restore(
                    r,
                    Msg::Restore,
                    Msg::DiscardCopy,
                    Msg::Later,
                    s.restore_failed
                ))
                .center(Fill)
            )
        ]
        .into(),
        None => main,
    };
    // the question and the walkthrough dialog are cards over the window: the picture under them takes no press
    let main: Element<'_, Msg> = match (s.asking(), s.exporting) {
        (true, _) => iced::widget::stack![
            main,
            iced::widget::opaque(container(question()).center(Fill))
        ]
        .into(),
        (false, true) => iced::widget::stack![
            main,
            iced::widget::opaque(container(export_sheet(s, steps)).center(Fill))
        ]
        .into(),
        (false, false) => main,
    };
    // the download card (Copy text / Translate need models): over the window
    let main: Element<'_, Msg> = match &s.card() {
        Some(card) => iced::widget::stack![
            main,
            iced::widget::opaque(
                container(card.card(Msg::Download, Msg::DismissCard, Msg::Provider)).center(Fill)
            )
        ]
        .into(),
        None => main,
    };
    let rim = if maximised { 0.0 } else { 1.0 };
    crate::shared::kit::focus::scope(
        surface(Material::Window, 0.0, main)
            .padding(rim)
            .width(Fill)
            .height(Fill),
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
            Why::Keys(super::keys::UNDO),
            Position::Right,
        ),
        hint(
            icon_button::square(toolbar::REDO, State::default(), redo),
            t!(sn_redo),
            Why::Keys(super::keys::REDO),
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

/// Edit text's options bar (S8): Font size, Colour, Bold (the editor's own
/// controls, set on the line), Cancel / Done. The text itself is typed on the
/// picture (`inline_field`).
fn edit_bar(x: &super::edit_text::Session) -> Element<'_, Msg> {
    let line: Element<'_, Msg> = match (&x.lines, x.active) {
        (None, _) => muted(Role::ROW_DESC, t!(st_edit_reading)).into(),
        (_, None) => muted(Role::ROW_DESC, t!(st_edit_pick)).into(),
        _ => {
            let now = x.colour();
            let swatches = (style::ANNOTATION.iter().take(SWATCHES)).map(|c| {
                let rgba = c.into_rgba8();
                colour::swatch(*c, rgba[..3] == now[..3], Some(Message::Colour(rgba)))
            });
            let bold = crate::features::snip::annotate::TextStyle {
                bold: x.bold(),
                ..crate::features::snip::annotate::TextStyle::default()
            };
            let options: Vec<Element<'_, Message>> = vec![
                container(toolbar::text_size_option(x.size()))
                    .width(OPTION_W)
                    .into(),
                dir::row(swatches).spacing(space::TRACK_GAP).into(),
                container(toolbar::bold_option(bold)).width(OPTION_W).into(),
            ];
            let options = dir::row(options)
                .spacing(space::PAGE[1] + space::PAGE[1])
                .align_y(Center);
            Element::from(scroll::across(container(options).center_y(super::OPTIONS)))
                .map(Msg::Editor)
        }
    };
    let ready = x.active.is_some();
    let row = crate::dir_row![
        container(line).width(Fill),
        container(cancel(label(t!(c_cancel)), Kind::Elevated, Msg::Cancel)).width(EDIT_BUTTON),
        super::options_bar::done(ready),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    container(row)
        .padding([0.0, space::PAGE[1]])
        .center_y(super::OPTIONS)
        .width(Fill)
        .into()
}

/// The picked line's new text, typed on the picture over the line in a field
/// ring (an input sized to the line, at least a words' width).
fn inline_field<'a>(
    x: &'a super::edit_text::Session,
    origin: iced::Point,
    zoom: f32,
) -> Option<Element<'a, Msg>> {
    let line = x.lines.as_ref()?.get(x.active?)?;
    let [bx, by, bw, bh] = line.bounds;
    let at = iced::Point::new((bx - origin.x) * zoom, (by - origin.y) * zoom);
    let field = crate::shared::kit::field::input(t!(st_edit_type), &x.draft, false, false)
        .id(super::FIELD)
        .on_input(Msg::Draft)
        .on_submit(Msg::EditDone)
        .width(Length::Fixed((bw * zoom).max(INLINE_MIN)));
    let _ = bh;
    Some(iced::widget::pin(field).position(at).into())
}

/// Cancel's and Done's width on the edit bar.
const EDIT_BUTTON: f32 = 84.0;

/// The inline field's least width, logical px.
const INLINE_MIN: f32 = 140.0;

/// Cut out's options bar (S9): Before | After, and where the work stays.
fn cut_bar<'a>(before: bool) -> Element<'a, Msg> {
    let seg = crate::shared::kit::segmented::inline(
        &[(true, t!(st_cut_before)), (false, t!(st_cut_after))],
        before,
        before,
        1.0,
        Msg::CutView,
    );
    let line = crate::dir_row![seg, muted(Role::ROW_DESC, t!(st_cut_note))]
        .spacing(space::PAGE[1])
        .align_y(Center);
    container(line)
        .padding([0.0, space::PAGE[1]])
        .center_y(super::OPTIONS)
        .width(Fill)
        .into()
}

/// The picture, fitted or zoomed (both ways scrollable past the stage).
fn stage<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let prefs = crate::shared::config::current().studio;
    let drawn = canvas::canvas(e, 1.0, ScreenMap::ONE).map(Msg::Editor);
    let (shown, room) = (e.size(), s.stage());
    // a cut-out (or see-through) picture sits on a checkerboard
    let drawn: Element<'a, Msg> = match s.transparent {
        true => iced::widget::stack![super::cut::checker(shown), drawn].into(),
        false => drawn,
    };
    // Edit text: the found lines over the picture (dashed, the picked one solid)
    let drawn: Element<'a, Msg> = match s
        .edit
        .as_ref()
        .and_then(|x| x.lines.as_deref().map(|l| (x, l)))
    {
        Some((x, lines)) => iced::widget::stack![
            drawn,
            super::edit_text::overlay(
                super::edit_text::Overlay {
                    lines,
                    active: x.active,
                    origin: e.doc.view().position(),
                    zoom: e.zoom,
                    look: crate::shared::theme::accent_of(&crate::shared::config::look()),
                },
                shown,
            ),
            match inline_field(x, e.doc.view().position(), e.zoom) {
                Some(f) => f,
                None => Space::new().into(),
            }
        ]
        .into(),
        None => drawn,
    };
    // a px less than the stage: no scrollbar for a fitted picture
    let content = container(drawn)
        .center_x((room.width - 2.0).max(shown.width))
        .center_y((room.height - 2.0).max(shown.height));
    let area = scroll::both(content)
        .id(super::CANVAS)
        .on_scroll(|v| Msg::Scrolled(Vector::new(v.absolute_offset().x, v.absolute_offset().y)));
    // Settings > Studio: the wheel zooms (Ctrl+wheel then scrolls) or scrolls
    let zooms = s.ctrl != (prefs.wheel == StudioWheel::Zoom);
    let area: Element<'a, Msg> = match prefs.background {
        StudioBackdrop::Checkerboard => iced::widget::stack![
            super::cut::checker(Size::new(room.width, room.height)),
            area
        ]
        .into(),
        _ => area.into(),
    };
    let material = match prefs.background {
        StudioBackdrop::Dark => Material::Stage,
        _ => Material::Recessed,
    };
    let stage: Element<'a, Msg> = mouse_area(wrap(area, CtrlWheel(zooms, s.pointer.clone())))
        .on_right_press(Msg::ContextOpen)
        .into();
    // Space held: a layer over the picture takes the drag (a pan), not the tools
    let stage: Element<'a, Msg> = match s.space {
        true => iced::widget::stack![
            stage,
            mouse_area(Space::new().width(Fill).height(Fill))
                .on_press(Msg::PanStart)
                .on_release(Msg::PanEnd)
                .on_move(Msg::Pointer)
                .interaction(mouse::Interaction::Grab)
        ]
        .into(),
        false => stage,
    };
    surface(material, 0.0, stage)
        .width(Fill)
        .height(Fill)
        .into()
}

/// Ctrl + wheel over the stage zooms round the pointer instead of scrolling
/// (the area under it never sees that wheel).
struct CtrlWheel(bool, super::PointerCell);

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
        if let Some(at) = cursor.position_in(layout.bounds()) {
            self.1.set(at); // (kept for the right-click menu: no message per move)
        }
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
            hint(
                open,
                t!(st_open),
                Why::Keys(super::keys::OPEN),
                Position::Bottom
            ),
            hint(
                paste,
                t!(st_paste),
                Why::Keys(super::keys::PASTE),
                Position::Bottom
            ),
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

/// The Steps strip's steps, one under the other: click = open on the canvas,
/// drag = reorder, x = delete; Copy all and Export at the foot.
pub(super) fn steps_list<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let pad = super::library::PAD;
    if steps.is_empty() {
        let none = muted(Role::ROW_DESC, t!(st_steps_none));
        return container(none).padding([0.0, pad]).into();
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
    // four to a row
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
    let half = (super::LIBRARY - 2.0 * pad - space::GAP) / 2.0;
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
    // the walkthrough first (STUDIO_V2_SPEC); one step is no walkthrough
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
        container(list).padding([0.0, pad]),
        container(foot).padding([space::GAP, pad])
    ]
    .into()
}

/// Export picture: the format, the JPG quality, the size; Cancel / Export…
/// (then the host's save dialog).
/// The Export sheet (STUDIO_V2_SPEC "Export dialog", boards S14 / S15 / S17):
/// a title and the Picture | Walkthrough tabs, then the tab's choices, a note
/// that it is made on this PC, Cancel and Export.
fn export_sheet<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let tabs = [(false, t!(st_export_pic)), (true, t!(st_export_walk))];
    let head = crate::dir_row![
        text(Role::ROW_TITLE, t!(st_export)).width(Fill),
        segmented::inline(&tabs, s.walk_tab, s.walk_tab, 1.0, Msg::ExportTab),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let (body, go, busy) = match s.walk_tab {
        true => walk_tab(s, s.walk, steps),
        false => picture_tab(s),
    };
    let progress: Element<'_, Msg> = match busy {
        Some((done, total)) => column![
            sub(
                Role::ROW_DESC,
                crate::shared::app_language::fmt(
                    crate::shared::app_language::S::st_walk_busy,
                    &[&done, &total]
                )
            ),
            crate::shared::kit::progress::bar(done as f32 / total.max(1) as f32),
        ]
        .into(),
        None => Space::new().into(),
    };
    let foot = crate::dir_row![
        sub(Role::META, t!(st_walk_pc)).width(Fill),
        button::regular(t!(c_cancel), false, Some(Msg::WalkCancel)),
        button::regular(t!(st_export_go), true, go),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    let column = column![head, body, progress, foot].spacing(space::PAGE[1]);
    surface(
        Material::Widget,
        radius::CARD,
        container(column).padding(space::PAGE),
    )
    .width(Length::Fixed(540.0))
    .into()
}

/// A labelled field.
fn field<'a>(words: &'a str, body: Element<'a, Msg>) -> Element<'a, Msg> {
    column![sub(Role::ROW_DESC, words), body]
        .spacing(space::GAP)
        .into()
}

/// The Size row's options.
pub(super) fn sizes<'a>(
    list: &[u32],
    chosen: u32,
    pick: impl Fn(u32) -> Msg + 'a,
) -> Element<'a, Msg> {
    let list: Vec<_> = (list.iter())
        .map(|&px| match px {
            0 => (px, t!(st_original)),
            2048 => (px, "2048"),
            1568 => (px, "1568"),
            1280 => (px, "1280"),
            1024 => (px, "1024"),
            960 => (px, "960"),
            _ => (px, "640"),
        })
        .collect();
    segmented::inline(&list, chosen, chosen, 1.0, pick)
}

/// The Picture tab: Format, Quality (JPG), Size, and what the file will be.
fn picture_tab(s: &Studio) -> (Element<'_, Msg>, Option<Msg>, Option<(usize, usize)>) {
    let how = s.export;
    let formats: Vec<_> = (export::Format::ALL.iter())
        .map(|&f| (f, f.name()))
        .collect();
    let mut items = vec![field(
        t!(st_walk_format),
        segmented::inline(&formats, how.format, how.format, 1.0, Msg::ExportFormat),
    )];
    if how.format.has_quality() {
        let quality = crate::dir_row![
            slider::bare(1..=100, how.quality, QUALITY_W, false, Msg::ExportQuality),
            container(sub(Role::KEYCAP, format!("{} %", how.quality))).width(QUALITY_VALUE_W),
        ];
        items.push(field(
            t!(st_quality),
            quality.spacing(space::GAP).align_y(Center).into(),
        ));
    }
    items.push(field(
        t!(st_size),
        sizes(&export::SIZES, how.max_edge, Msg::ExportSize),
    ));
    let note = match how.format {
        export::Format::Pdf => t!(st_pdf_note),
        export::Format::Jpg => "",
        _ => t!(st_lossless),
    };
    if let Some(e) = &s.editor {
        let v = e.doc.view().size();
        let (w, h) =
            crate::shared::picture::fitted_size(v.width as u32, v.height as u32, how.max_edge);
        let size = [w.to_string(), h.to_string()].join(" × ");
        let line = match note.is_empty() {
            true => size,
            false => [size, note.to_string()].join(" · "),
        };
        items.push(sub(Role::ROW_DESC, line).into());
    }
    let go = s.editor.is_some().then_some(Msg::ExportGo);
    (column(items).spacing(space::PAGE[1]).into(), go, None)
}

/// The Walkthrough tab: From, the frames with each one's time, Format and
/// Size, Frame time, the two switches, the estimate.
fn walk_tab<'a>(
    s: &'a Studio,
    w: Walk,
    steps: &'a [Arc<Step>],
) -> (Element<'a, Msg>, Option<Msg>, Option<(usize, usize)>) {
    let edits = s.editor.as_ref().map_or(0, |e| e.doc.depth());
    let n = if w.edits {
        (edits + 1).min(export::EDIT_FRAMES)
    } else {
        steps.len()
    };
    let label = |words: &'a str| sub(Role::ROW_DESC, words);
    // From: the Steps, or the picture's edits (none to replay: not offered)
    let sources = [(false, t!(st_walk_steps)), (true, t!(st_walk_edits))];
    let sources: Vec<_> = if edits > 0 {
        sources.to_vec()
    } else {
        sources[..1].to_vec()
    };
    let from = segmented::inline(&sources, w.edits, w.edits, 1.0, Msg::WalkEdits);
    // the frames, with each one's time (at most six shown)
    let secs = format!("{:.1} {}", f32::from(w.tenths) / 10.0, t!(st_walk_s));
    let frames: Vec<Element<'a, Msg>> = (0..n.min(WALK_SHOWN))
        .map(|i| {
            let picture: Element<'a, Msg> = match (w.edits, steps.get(i)) {
                (false, Some(step)) => {
                    thumb::step(step.thumb(), i + 1, thumb::Look::default(), None)
                }
                _ => surface(
                    Material::Recessed,
                    radius::TRACK,
                    container(sub(Role::ROW_TITLE, (i + 1).to_string())).center(Fill),
                )
                .width(thumb::WELL.width)
                .height(thumb::WELL.height)
                .into(),
            };
            column![picture, sub(Role::KEYCAP, secs.clone())]
                .spacing(space::GAP / 2.0)
                .align_x(Center)
                .into()
        })
        .collect();
    let mut strip = dir::row(frames).spacing(space::GAP);
    if n > WALK_SHOWN {
        strip = strip.push(sub(Role::ROW_DESC, format!("+{}", n - WALK_SHOWN)));
    }
    let movies: Vec<_> = (export::Movie::ALL.iter())
        .map(|&m| (m, m.name()))
        .collect();
    let format = segmented::inline(&movies, w.how.movie, w.how.movie, 1.0, Msg::WalkFormat);
    let size = sizes(&[0, 1280, 960, 640], w.how.max_edge, Msg::WalkSize);
    let grid = crate::dir_row![
        field(t!(st_walk_format), format),
        Space::new().width(Fill),
        field(t!(st_size), size)
    ]
    .spacing(space::PAGE[1]);
    let time = crate::dir_row![
        slider::bare(5..=60, w.tenths, WALK_SLIDER_W, false, Msg::WalkTime),
        sub(Role::ROW_DESC, secs.clone()),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    // a video plays once or loops as its player decides: Loop is the GIF's
    let looped = (w.how.movie == export::Movie::Gif).then_some(Msg::WalkLoop(!w.how.repeat));
    let switches = crate::dir_row![
        label(t!(st_walk_number)),
        Space::new().width(space::GAP),
        button::toggle(w.how.numbered, Some(Msg::WalkNumbered(!w.how.numbered))),
        Space::new().width(Fill),
        label(t!(st_walk_loop)),
        Space::new().width(space::GAP),
        button::toggle(w.how.repeat, looped),
    ]
    .align_y(Center);
    let mut notes = column![
        sub(Role::ROW_DESC, walk_estimate(s, w, steps, n)).width(Fill),
        sub(Role::META, t!(st_walk_note)).width(Fill),
    ];
    if w.how.movie != export::Movie::Gif {
        notes = notes.push(sub(Role::META, t!(st_walk_cap)).width(Fill));
    }
    let body = column![
        field(t!(st_walk_from), from),
        strip,
        grid,
        field(t!(st_walk_frame), time.into()),
        switches,
        notes,
    ]
    .spacing(space::PAGE[1]);
    let go = (n > 0).then_some(Msg::WalkGo);
    (body.into(), go, w.busy)
}

/// "Frames 4 · 8 s · about 1.1 MB".
fn walk_estimate(s: &Studio, w: Walk, steps: &[Arc<Step>], n: usize) -> String {
    let bytes: usize = match (w.edits, &s.editor) {
        (true, Some(e)) => {
            let size = e.doc.view().size();
            w.how
                .movie
                .guess((size.width * size.height * 0.25) as usize)
                * n
        }
        _ => steps.iter().map(|x| w.how.movie.guess(x.png().len())).sum(),
    };
    let secs = n as f32 * f32::from(w.tenths) / 10.0;
    let size = match bytes >= 1_048_576 {
        true => format!("{:.1} MB", bytes as f32 / 1_048_576.0),
        false => format!("{} KB", bytes.div_ceil(1024)),
    };
    [
        format!("{} {n}", t!(st_walk_frames)),
        format!("{secs:.0} {}", t!(st_walk_s)),
        format!("{} {size}", t!(st_walk_about)),
    ]
    .join(" · ")
}

/// The frames the sheet's strip shows at most.
const WALK_SHOWN: usize = 6;
const WALK_SLIDER_W: f32 = 240.0;
/// The Picture tab's quality slider.
const QUALITY_W: f32 = 120.0;
const QUALITY_VALUE_W: f32 = 40.0;
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
