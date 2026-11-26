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
use crate::features::snip::transition::{self, Transition};
use crate::shared::app_language::tr;
use crate::shared::kit::button::{self, Kind, cancel, label};
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::modal;
use crate::shared::kit::slide_over::{self, Scrim};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub};
use crate::shared::kit::tokens::{Role, radius, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::wrap::{Hook, wrap};
use crate::shared::kit::{colour, dir, divider, popover, scroll, segmented, slider, thumb};
use crate::t;

pub(super) const PANEL_LEFT: char = '\u{e12d}'; // lucide "panel-left"
pub(super) const PANEL_RIGHT: char = '\u{e435}'; // lucide "panel-right"
/// lucide "folder-open"
pub(super) const OPEN: char = '\u{e246}';
/// lucide "settings" (Studio preferences)
pub(super) const GEAR: char = '\u{e157}';
pub(super) const PASTE: char = '\u{e3eb}'; // lucide "clipboard-paste"
pub(super) const PLUS: char = '\u{e140}'; // lucide "plus"
pub(super) const MINUS: char = '\u{e11f}'; // lucide "minus"

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[PANEL_LEFT, PANEL_RIGHT, OPEN, PASTE, PLUS, MINUS, GEAR];

/// OpenCircle Studio's window (v3.1): the header with the menu bar; below it
/// the tools rail (full height, at the window's edge), then the options bar
/// across the rest, and under it the canvas with its status bar and, at the
/// other edge, the one Steps sidebar. Popovers and slide-overs sit over it all.
/// `steps`: the Steps strip's steps (the sidebar shows them).
pub fn view<'a>(s: &'a Studio, steps: &'a [Arc<Step>], maximised: bool) -> Element<'a, Msg> {
    let _dir = dir::scope(crate::shared::app_language::rtl());
    // Ctrl+= / Ctrl+- / Ctrl+0: the Studio's own UI scale (one factor through the view)
    crate::shared::kit::text::zoomed(f32::from(s.ui_zoom) / 100.0, || build(s, steps, maximised))
}

fn build<'a>(s: &'a Studio, steps: &'a [Arc<Step>], maximised: bool) -> Element<'a, Msg> {
    let header = crate::shared::kit::header::with_menu(
        crate::shared::kit::logo::logo(16),
        Some(super::menu_bar::bar(s)),
        &s.title,
        s.dirty(),
        Space::new(),
        Some(maximised),
        Msg::Caption,
    );
    // Settings > Studio > Steps sidebar side: at the reading start the rail takes the
    // other edge (the options bar always runs beside the rail, over the sidebar)
    let first = s.sidebar_at_start();
    let side = |parts: &mut Vec<Element<'a, Msg>>| {
        if s.sidebar_shown() {
            let bar = [divider::vertical(Fill), super::sidebar::view(s, steps)];
            match first {
                true => {
                    let stage: Vec<_> = std::mem::take(parts);
                    parts.extend(bar.into_iter().rev());
                    parts.extend(stage);
                }
                false => parts.extend(bar),
            }
        }
    };
    let body: Element<'a, Msg> = match &s.editor {
        Some(e) => {
            let mut under: Vec<Element<'a, Msg>> = vec![
                column![
                    stage(s, e),
                    divider::divider(),
                    super::status_bar::view(s, e)
                ]
                .width(Fill)
                .height(Fill)
                .into(),
            ];
            side(&mut under);
            let right = column![
                match &s.edit {
                    Some(x) => edit_bar(x),
                    None => super::options_bar::view(s, e),
                },
                divider::divider(),
                dir::row(under).height(Fill)
            ]
            .width(Fill)
            .height(Fill);
            let mut parts: Vec<Element<'a, Msg>> = vec![
                rail(e, inward(rail_on_left(first, dir::rtl()))),
                divider::vertical(Fill),
                right.into(),
            ];
            if first {
                parts.reverse();
            }
            dir::row(parts).height(Fill).into()
        }
        None => {
            // no picture: the options bar's row stays (its picture controls dimmed), so the
            // Steps toggle sits where it does with a picture; the sidebar starts under it
            let mut parts: Vec<Element<'a, Msg>> = vec![super::start::view(s)];
            side(&mut parts);
            column![
                super::options_bar::empty(s),
                divider::divider(),
                dir::row(parts).height(Fill)
            ]
            .width(Fill)
            .height(Fill)
            .into()
        }
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
        true => modal::over_rising(
            main,
            super::sheets::shortcuts(s, s.window),
            Msg::Shortcuts(false),
            s.rise(),
        ),
        false => main,
    };
    // the Studio's preferences (Ctrl+, / File > Preferences), over everything but the
    // question; the keys are the dialog's (Esc closes it)
    let main: Element<'_, Msg> = match &s.prefs {
        Some(d) => modal::over_rising(
            main,
            d.view(s.window).map(Msg::Prefs),
            Msg::Prefs(super::preferences::Msg::Close),
            s.rise(),
        ),
        None => main,
    };
    // a popover (Actions, Position, picture info, More) hangs from its opener
    // (never over a slide-over: the picture must take its presses)
    let main: Element<'_, Msg> = match s.popover.filter(|_| {
        s.editor.is_some() && !s.exporting && s.edit.is_none() && s.comparing().is_none()
    }) {
        Some(pop) => {
            let (corner, offset) = match pop {
                super::popovers::Pop::Info => (
                    popover::Corner::BottomStart,
                    iced::Padding {
                        left: 12.0,
                        right: 12.0,
                        bottom: super::STATUS + 6.0,
                        ..iced::Padding::ZERO
                    },
                ),
                super::popovers::Pop::Colours | super::popovers::Pop::Custom => (
                    popover::Corner::TopStart,
                    iced::Padding {
                        left: if first { 8.0 } else { super::RAIL + 40.0 },
                        right: if first { super::RAIL + 40.0 } else { 8.0 },
                        top: super::HEADER + super::OPTIONS + 4.0,
                        ..iced::Padding::ZERO
                    },
                ),
                _ => (
                    popover::Corner::TopEnd,
                    iced::Padding {
                        left: 8.0,
                        right: 8.0,
                        top: super::HEADER + super::OPTIONS + 4.0,
                        ..iced::Padding::ZERO
                    },
                ),
            };
            let (size, end) = pop.card();
            // the custom colour is Snip's picker panel alone (no card, no X)
            let place = match pop {
                super::popovers::Pop::Custom => popover::anchored_bare,
                _ => popover::anchored_rising,
            };
            place(
                main,
                super::popovers::content(s, pop),
                s.anchors.of(pop).get(),
                size,
                end,
                s.popover_room(),
                (corner, offset),
                Msg::Pop(None),
                s.rise(),
            )
        }
        None => main,
    };
    // after a crash: the kept copy
    let main: Element<'_, Msg> = match s.restorable.as_ref().filter(|_| !s.later) {
        Some(r) => modal::over(
            main,
            super::sheets::restore(
                r,
                Msg::Restore,
                Msg::DiscardCopy,
                Msg::Later,
                s.restore_failed,
            ),
            super::sheets::RESTORE_W,
            Msg::Later,
        ),
        None => main,
    };
    // the big temporary tasks slide in from the end edge: the walkthrough /
    // picture export, Edit text and Cut out (the picture stays usable beside
    // the last two)
    let main: Element<'_, Msg> = match (s.exporting, &s.edit, s.comparing()) {
        (true, _, _) => {
            let scrim = match s.walk.busy {
                Some(_) => Scrim::Holds,
                None => Scrim::Closes,
            };
            slide_over::over_wide(
                main,
                t!(st_export),
                export_sheet(s, steps),
                Msg::WalkCancel,
                scrim,
                super::HEADER,
                EXPORT_W,
            )
        }
        (false, None, Some(before)) => slide_over::over(
            main,
            t!(st_cut_out),
            cut_panel(before),
            Msg::CutClose,
            Scrim::None,
            super::HEADER,
        ),
        _ => main,
    };
    // the question is a solid modal over the dimmed window: the picture under it takes no press
    let main: Element<'_, Msg> = match s.asking() {
        true => modal::over_lifted(main, question(), QUESTION_W, Msg::Cancel, s.rise()),
        false => main,
    };
    // the download card (Copy text / Translate need models): over the window
    let main: Element<'_, Msg> = match &s.card() {
        Some(card) => iced::widget::stack![
            main,
            iced::widget::opaque(
                container(card.card(
                    Msg::Download,
                    Msg::DismissCard,
                    Msg::Provider,
                    Msg::CardDetails
                ))
                .center(Fill)
            )
        ]
        .into(),
        None => main,
    };
    // a big picture is being prepared: the kit's turning logo over the window (no press blocked)
    let main: Element<'_, Msg> = match s.opening_since() {
        Some(since) => {
            let turn = crate::shared::kit::spinner::turn(s.now.saturating_duration_since(since));
            let spin = crate::shared::kit::spinner::spinner(
                crate::shared::kit::logo::logo(56),
                28.0,
                turn,
                crate::shared::kit::motion::reduced(),
            );
            iced::widget::stack![main, container(spin).center(Fill)].into()
        }
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
/// A tooltip beside a control of a side panel, toward the picture: `on_left` = the panel
/// is at the window's left edge. (A tooltip pointing off the window is pushed back over
/// its own control.)
pub(super) fn inward(on_left: bool) -> Position {
    match on_left {
        true => Position::Right,
        false => Position::Left,
    }
}

/// Is the tools rail at the window's left edge? It takes the edge the Steps sidebar
/// does not: the sidebar at the reading start puts the rail at the end.
pub(super) fn rail_on_left(sidebar_first: bool, rtl: bool) -> bool {
    sidebar_first == rtl
}

fn rail(e: &Annotator, tip_at: Position) -> Element<'_, Msg> {
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
            tools.push(hint(button, name, Why::Icon, tip_at));
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
            tip_at,
        ),
        hint(
            icon_button::square(toolbar::REDO, State::default(), redo),
            t!(sn_redo),
            Why::Keys(super::keys::REDO),
            tip_at,
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

/// Edit text's bar (S8), in the options bar's place while it runs: Font size,
/// ONE colour row, Bold (the editor's own controls, set on the line), Cancel /
/// Done. The text itself is typed on the picture (`inline_field`).
fn edit_bar(x: &super::edit_text::Session) -> Element<'_, Msg> {
    let line: Element<'_, Msg> = match (&x.lines, x.active) {
        (None, _) => muted(Role::ROW_DESC, t!(st_edit_reading)).into(),
        (_, None) => muted(Role::ROW_DESC, t!(st_edit_pick)).into(),
        _ => {
            let now = x.colour();
            let swatches = (style::ANNOTATION
                .iter()
                .take(crate::features::snip::screens::options_row::SWATCHES))
            .map(|c| {
                let rgba = c.into_rgba8();
                colour::swatch(*c, rgba[..3] == now[..3], Some(Message::Colour(rgba)))
            });
            let bold = crate::features::snip::annotate::TextStyle {
                bold: x.bold(),
                ..crate::features::snip::annotate::TextStyle::default()
            };
            let options: Vec<Element<'_, Message>> = vec![
                toolbar::text_size_option(x.size()),
                dir::row(swatches).spacing(space::TRACK_GAP).into(),
                toolbar::bold_option(bold),
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

/// Cut out's slide-over (S9): Before | After, and where the work stays.
fn cut_panel<'a>(before: bool) -> Element<'a, Msg> {
    let seg = crate::shared::kit::segmented::inline(
        &[(true, t!(st_cut_before)), (false, t!(st_cut_after))],
        before,
        before,
        1.0,
        Msg::CutView,
    );
    column![seg, muted(Role::ROW_DESC, t!(st_cut_note))]
        .spacing(space::PAGE[1])
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
    // the wheel zooms the picture at the pointer (Settings > Studio: or scrolls it); Ctrl+wheel always zooms
    let zooms = s.ctrl || prefs.wheel == StudioWheel::Zoom;
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
    // the middle button drags the picture (Photoshop); the pointer is followed
    // only while it does
    let stage = mouse_area(wrap(area, CtrlWheel(zooms, s.pointer.clone())))
        .on_right_press(Msg::ContextOpen)
        .on_middle_press(Msg::Middle(true))
        .on_middle_release(Msg::Middle(false));
    let stage: Element<'a, Msg> = match s.middle {
        true => stage
            .on_move(Msg::Pointer)
            .interaction(mouse::Interaction::Grabbing)
            .into(),
        false => stage.into(),
    };
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

/// A wheel over the stage that zooms (the default, or Ctrl held) zooms round the
/// pointer instead of scrolling (the area under it never sees that wheel).
struct CtrlWheel(bool, super::PointerCell);

use crate::features::snip::annotate::WHEEL_PX;

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

/// Export picture: the format, the JPG quality, the size; Cancel / Export…
/// (then the host's save dialog).
/// The Export sheet (STUDIO_V2_SPEC "Export dialog", boards S14 / S15 / S17):
/// a title and the Picture | Walkthrough tabs, then the tab's choices, a note
/// that it is made on this PC, Cancel and Export.
fn export_sheet<'a>(s: &'a Studio, steps: &'a [Arc<Step>]) -> Element<'a, Msg> {
    let tabs = [(false, t!(st_export_pic)), (true, t!(st_export_walk))];
    let head = segmented::inline(&tabs, s.walk_tab, s.walk_tab, 1.0, Msg::ExportTab);
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
    column![head, body, progress, foot]
        .spacing(space::PAGE[1])
        .align_x(dir::start())
        .into()
}

/// A labelled field.
fn field<'a>(words: &'a str, body: Element<'a, Msg>) -> Element<'a, Msg> {
    column![sub(Role::ROW_DESC, words), body]
        .spacing(space::GAP)
        .align_x(dir::start())
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
    // ONE transition for all the steps, and how long it takes (a Cut has no length)
    let kinds: Vec<_> = Transition::ALL
        .iter()
        .map(|&k| (k, transition_name(k)))
        .collect();
    let lengths = transition::LENGTHS;
    let mut change = column![field(
        t!(mv_transition),
        segmented::inline(
            &kinds,
            w.how.transition,
            w.how.transition,
            1.0,
            Msg::WalkTransition
        ),
    )]
    .spacing(space::GAP);
    if w.how.transition != Transition::Cut {
        change = change.push(field(
            t!(mv_duration),
            segmented::inline(
                &lengths,
                w.how.transition_ms,
                w.how.transition_ms,
                1.0,
                Msg::WalkTransitionMs,
            ),
        ));
    }
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
    // (the estimate mixes digits and Latin units: it reads from the start edge, left to right)
    let at = dir::start();
    let mut notes = column![
        sub(Role::ROW_DESC, walk_estimate(s, w, steps, n))
            .width(Fill)
            .align_x(at),
        sub(Role::META, t!(st_walk_note)).width(Fill).align_x(at),
    ];
    if w.how.movie != export::Movie::Gif {
        notes = notes.push(sub(Role::META, t!(st_walk_cap)).width(Fill).align_x(at));
    }
    // sections, a hairline between: the frames, the video, the change between steps, the
    // switches, then what comes out
    let rule = || container(crate::shared::kit::divider::divider()).padding([space::GAP, 0.0]);
    let body = column![
        field(t!(st_walk_from), from),
        strip,
        field(t!(st_walk_frame), time.into()),
        rule(),
        grid,
        rule(),
        change,
        rule(),
        switches,
        rule(),
        notes,
    ]
    .spacing(space::PAGE[1])
    .align_x(dir::start());
    let go = (n > 0).then_some(Msg::WalkGo);
    (body.into(), go, w.busy)
}

fn transition_name(kind: Transition) -> &'static str {
    match kind {
        Transition::Cut => t!(mv_tr_cut),
        Transition::Crossfade => t!(mv_tr_fade),
        Transition::Slide => t!(mv_tr_slide),
        Transition::Push => t!(mv_tr_push),
        Transition::Zoom => t!(mv_tr_zoom),
        Transition::Wipe => t!(mv_tr_wipe),
    }
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
    // the transitions' frames weigh like the average step
    let bytes = bytes + bytes / n.max(1) * w.how.mix_frames(n);
    let secs = n as f32 * f32::from(w.tenths) / 10.0
        + (w.how.mix_frames(n) as f32) * transition::TICK as f32 / 1000.0;
    let size = match bytes >= 1_048_576 {
        true => format!("{:.1} MB", bytes as f32 / 1_048_576.0),
        false => format!("{} KB", bytes.div_ceil(1024)),
    };
    [
        format!("{} {n}", t!(st_walk_frames)),
        format!("{secs:.0} {}", t!(st_walk_s)),
        format!("{} \u{2066}{size}\u{2069}", t!(st_walk_about)),
    ]
    .join(" · ")
}

/// The frames the sheet's strip shows at most.
const WALK_SHOWN: usize = 6;
const WALK_SLIDER_W: f32 = 300.0;
/// The Export panel's width: room for the sections, the preview strip and two-column rows.
const EXPORT_W: f32 = 560.0;
/// The Picture tab's quality slider.
const QUALITY_W: f32 = 120.0;
const QUALITY_VALUE_W: f32 = 40.0;
const QUESTION_W: f32 = 460.0;

/// Closing or opening another picture with unsaved drawings: a solid modal over the
/// dimmed window: Don't save / Cancel (secondary), Save (the accent primary); Esc
/// cancels, Enter saves, the X cancels.
fn question<'a>() -> Element<'a, Msg> {
    let labels = [t!(st_dont_save), t!(c_cancel), t!(c_save)];
    let w = button::dialog_width(&labels);
    let buttons = crate::dir_row![
        Space::new().width(Fill),
        button::dialog(labels[0], w, Kind::Elevated, Msg::DontSave),
        button::dialog(labels[1], w, Kind::Elevated, Msg::Cancel),
        button::dialog(labels[2], w, Kind::Accent, Msg::SaveFirst),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    modal::pad(vec![
        modal::header(t!(st_ask_close), Some(t!(st_ask_hint)), Msg::Cancel),
        buttons.into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W90d.3: the rail's and the sidebar's tooltips point toward the picture, never off the
    /// window (where they land over their own control).
    #[test]
    fn side_panel_tooltips_point_inward() {
        // default: rail at the reading start; English = left edge
        assert!(rail_on_left(false, false));
        assert!(
            !rail_on_left(false, true),
            "Arabic: the rail is at the right edge"
        );
        assert!(matches!(inward(rail_on_left(false, true)), Position::Left));
        assert!(matches!(
            inward(rail_on_left(false, false)),
            Position::Right
        ));
        assert!(matches!(inward(!rail_on_left(true, true)), Position::Left));
    }
}
