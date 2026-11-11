//! The editor's toolbar: the grouped Snip toolbar (owner approved, 2026-11-02;
//! SNIP_TOOLBAR_SPEC), composed from kit parts only. One bar, always the same:
//! grip (moves the toolbar) · size chip (drags the picture out) · Select ·
//! Shapes · Draw · Text · Hide · Colour · Undo · More · Copy · Save. A group
//! button shows its last tool; a click on the active one opens ONE panel by
//! it (the group's tools, then that tool's options). No ×: Esc closes.
use iced::widget::{Space, column, container, tooltip};
#[cfg(test)]
use iced::widget::{Space as Gap, row};
use iced::{Element, Fill, Point, Rectangle, Size, Vector};

use crate::actions::share;
use crate::actions::snip_more::{self, Read, Row};
use crate::app_language::{S, fmt, tr};
use crate::snip::annotate::{
    Annotator, Group, Menu, Message, PRESETS, ScreenTool, Tool, ToolInfo, Width, dragout, info,
};
use crate::t;
use crate::ui::kit::dir;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, State};
use crate::ui::kit::surface::{self, Material, surface};
use crate::ui::kit::text::{muted, sub, text as caption};
use crate::ui::kit::tokens::{Role, radius, size, space};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::kit::{button, chip, colour, divider, grip, keycap, option_list, segmented, slider};
use crate::ui::screens::bar::geometry as bar;

const SPACING: f32 = bar::SPACING;
/// A button's side.
const CONTROL: f32 = size::ICON_BUTTON;
/// The size chip's room (fits `[picture] 9999 × 9999`).
const CHIP_W: f32 = 112.0;
const UNDO: char = '\u{e2a0}'; // lucide "undo-2"
const REDO: char = '\u{e29f}'; // lucide "redo-2"
pub const COPY: char = '\u{e0a2}'; // lucide "copy"
pub const SAVE: char = '\u{e0b6}'; // lucide "download"
const PIN: char = '\u{e258}'; // lucide "pin"
const SCROLL: char = '\u{e4d6}'; // lucide "gallery-vertical-end"
const PIPETTE: char = crate::ui::screens::tools::picker::ICON;
const SHARE: char = '\u{e158}'; // lucide "share"
const LIVE_PIN: char = '\u{e3b2}'; // lucide "picture-in-picture-2"
const LIST_PLUS: char = '\u{e23e}'; // lucide "list-plus"
/// A tool's keycap in its panel.
const KEY_W: f32 = 22.0;
/// The Text tool's size presets (display px).
const TEXT_PRESETS: [f32; 3] = [16.0, 26.0, 38.0];

/// The glyphs used here (the font test checks them).
#[cfg(test)]
pub const GLYPHS: &[char] = &[
    UNDO,
    REDO,
    COPY,
    SAVE,
    PIN,
    SCROLL,
    PIPETTE,
    SHARE,
    crate::ui::screens::tools::ruler::ICON,
    LIVE_PIN,
    LIST_PLUS,
];

/// Snip's Live pin (Windows; Labs Pins); it works only over the live screen
/// (`Annotator::scroll`). Still pins left Labs (owner, 2026-10-31): Pin is always there.
fn live_pin() -> bool {
    crate::labs::has(crate::labs::Feature::Pins) && crate::capture::live::SUPPORTED
}

/// The labelled primary in Copy's place while the Steps strip is open
/// (fits "Update step 12").
const PRIMARY_W: f32 = 128.0;

/// The bar's items' widths, in order (`build`); `primary`: the labelled one.
fn widths(primary: bool) -> Vec<f32> {
    let mut w = vec![bar::HANDLE, CHIP_W, bar::DIVIDER];
    w.extend(Group::ALL.map(|_| CONTROL));
    w.extend([
        CONTROL,
        CONTROL,
        bar::DIVIDER,
        CONTROL,
        bar::DIVIDER,
        if primary { PRIMARY_W } else { CONTROL },
        CONTROL,
    ]);
    w
}

/// The toolbar's size (one line, the same in every state)...
pub fn size() -> Size {
    sized(false)
}

/// ...and with the Steps strip open ("Add step N" in Copy's place).
pub fn size_for(e: &Annotator) -> Size {
    sized(steps_on(e))
}

fn sized(primary: bool) -> Size {
    let w = widths(primary);
    let line = w.iter().sum::<f32>() + SPACING * (w.len() - 1) as f32;
    Size::new(line + 2.0 * bar::PAD, bar::HEIGHT)
}

/// The Steps strip is open, or one of its steps is edited.
fn steps_on(e: &Annotator) -> bool {
    e.steps.is_some() || e.updating.is_some()
}

/// What Tab walks on the toolbar, in its order (left to right), and what
/// Enter / Space on each sends.
pub fn stops(e: &Annotator) -> Vec<Message> {
    let mut stops: Vec<Message> = e.last.iter().map(|&t| Message::Slot(t)).collect();
    stops.extend([
        Message::Picker,
        Message::UndoClick,
        Message::Menu(Menu::More),
        if steps_on(e) {
            Message::AddStep
        } else {
            Message::Copy
        },
        Message::Save,
    ]);
    stops
}

/// A row of More: glyph, name (Latin, Arabic), its static key, its key caps,
/// what it sends (`None`: dimmed), and which of the three text rows it is.
pub struct MoreRow {
    pub glyph: char,
    pub name: S,
    pub caps: Vec<String>,
    pub message: Option<Message>,
    pub of: Option<Row>,
}

impl MoreRow {
    fn plain(glyph: char, name: S, key: Option<&'static str>, message: Option<Message>) -> Self {
        MoreRow {
            glyph,
            name,
            caps: key.map(snip_more::caps).unwrap_or_default(),
            message,
            of: None,
        }
    }
}

/// The rows before the hairline: Copy text, Ask AI, Translate.
pub const TEXT_ROWS: usize = 3;

/// More's rows (Labs and platform rows only where they work): the three
/// picture actions first (each shows the user's own shortcut), then today's.
pub fn more_rows(e: &Annotator) -> Vec<MoreRow> {
    // a long capture running: only what finishes it stays live
    let idle = !e.capturing;
    let live = |m: Message| idle.then_some(m);
    let info = &e.more;
    let mut rows: Vec<MoreRow> = Row::ALL
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let message = match (snip_more::why(row, info, e.capturing), row) {
                (Some(_), _) => None,
                (None, Row::CopyText) => Some(Message::CopyText),
                (None, Row::AskAi) => Some(Message::AskAi),
                (None, Row::Translate) => Some(Message::Translate),
            };
            MoreRow {
                glyph: row.glyph(),
                name: row.label(info.translated),
                caps: info.caps[i].clone(),
                message,
                of: Some(row),
            }
        })
        .collect();
    rows.push(MoreRow::plain(
        SCROLL,
        S::sn_long_capture,
        None,
        (e.scroll && idle).then_some(Message::Scroll),
    ));
    // while the strip is open Copy's place adds the step: Copy moves here
    rows.push(match steps_on(e) {
        false => MoreRow::plain(
            LIST_PLUS,
            S::sn_add_to_steps,
            Some("Ctrl+Enter"),
            live(Message::AddStep),
        ),
        true => MoreRow::plain(COPY, S::c_copy, Some("Ctrl+C"), Some(Message::Copy)),
    });
    rows.push(MoreRow::plain(
        PIN,
        S::sn_pin,
        Some("Ctrl+P"),
        Some(Message::Pin),
    ));
    if live_pin() {
        let m = (e.scroll && idle).then_some(Message::LivePin);
        rows.push(MoreRow::plain(LIVE_PIN, S::sn_live_pin, None, m));
    }
    if share::SUPPORTED {
        rows.push(MoreRow::plain(
            SHARE,
            S::sn_share,
            None,
            Some(Message::Share),
        ));
    }
    rows.push(MoreRow::plain(
        PIPETTE,
        S::sn_colour_picker,
        None,
        live(Message::ScreenTool(ScreenTool::Picker)),
    ));
    rows.push(MoreRow::plain(
        crate::ui::screens::tools::ruler::ICON,
        S::sn_ruler,
        None,
        live(Message::ScreenTool(ScreenTool::Ruler)),
    ));
    rows
}

/// The toolbar with its open panel by it (where nothing limits the room).
#[cfg(test)]
pub fn view(e: &Annotator) -> Element<'_, Message> {
    let (bar, panel) = build(e, tooltip::Position::Bottom);
    match panel {
        None => bar,
        Some((panel, left)) => column![bar, row![Gap::new().width(left.max(0.0)), panel]]
            .spacing(POPOVER_GAP)
            .into(),
    }
}

/// The toolbar shown at `bar` (screen px) and its open panel as a floating
/// layer for the host to put over the whole screen at (0, 0): laid out there
/// at its full size, then moved by `place_popover` (`side`: where the
/// tooltips and the panel go, away from Snip's area).
pub fn view_at(
    e: &Annotator,
    bar: Rectangle,
    side: tooltip::Position,
) -> (Element<'_, Message>, Option<Element<'_, Message>>) {
    let (toolbar, panel) = build(e, side);
    let above = side == tooltip::Position::Top;
    let panel = panel.map(|(panel, left)| {
        iced::widget::float(panel)
            .translate(move |laid, screen| {
                let y = match above {
                    true => bar.y - POPOVER_GAP - laid.height,
                    false => bar.y + bar.height + POPOVER_GAP,
                };
                let at = Rectangle::new(Point::new(bar.x + left, y), laid.size());
                at.position() - laid.position() + place_popover(at, bar.height, screen)
            })
            .into()
    });
    (toolbar, panel)
}

thread_local! {
    /// Where tooltips go (the toolbar's side away from the area), for this build.
    static TIP_SIDE: std::cell::Cell<tooltip::Position> =
        const { std::cell::Cell::new(tooltip::Position::Bottom) };
}

/// A tool's tooltip, `Name (K)`, in the app's language: built once per tool
/// and language (a handful of short strings kept for the run).
fn tip(t: &'static ToolInfo) -> &'static str {
    type Cache = std::collections::HashMap<(char, bool), &'static str>;
    thread_local!(static TIPS: std::cell::RefCell<Cache> = std::cell::RefCell::default());
    let key = (t.key, dir::rtl());
    TIPS.with(|c| {
        *(c.borrow_mut().entry(key)).or_insert_with(|| {
            let name = format!("{} ({})", tr(t.name), t.key.to_ascii_uppercase());
            Box::leak(name.into_boxed_str())
        })
    })
}

/// A line of items, mirrored right to left.
fn line<'a>(items: Vec<Element<'a, Message>>) -> iced::widget::Row<'a, Message> {
    iced::widget::Row::with_children(dir::order(items))
        .spacing(SPACING)
        .align_y(iced::Center)
}

/// A button, named (and given its key) in a tooltip beside it.
fn hinted<'a>(button: Element<'a, Message>, label: &'a str, why: Why<'a>) -> Element<'a, Message> {
    hint(button, label, why, TIP_SIDE.get())
}

/// A square icon button with its tooltip.
fn icon<'a>(glyph: char, on: bool, message: Option<Message>, tip: &'a str) -> Element<'a, Message> {
    let state = State {
        active: on,
        focused: false,
    };
    hinted(icon_button::square(glyph, state, message), tip, Why::Icon)
}

/// A divider between parts of the bar.
fn divide<'a>() -> Element<'a, Message> {
    container(divider::vertical(bar::DIVIDER_H))
        .center_x(bar::DIVIDER)
        .into()
}

/// The toolbar, and the open panel with its left edge (from the toolbar's).
#[allow(clippy::type_complexity)]
fn build(
    e: &Annotator,
    side: tooltip::Position,
) -> (Element<'_, Message>, Option<(Element<'_, Message>, f32)>) {
    TIP_SIDE.set(side);
    let stops = stops(e);
    let focused = |m: &Message| e.focus.and_then(|i| stops.get(i)).is_some_and(|s| s == m);
    let state = |active: bool, m: &Message| State {
        active,
        focused: focused(m),
    };
    // a long capture running: only what finishes it stays live
    let idle = !e.capturing;
    let live = |m: Message| idle.then_some(m);
    let mut items: Vec<Element<'_, Message>> =
        vec![hinted(grip::mover(Message::Grab), t!(sn_move), Why::Icon)];
    let px = e.doc.view().size();
    let label = format!("{} × {}", px.width.round(), px.height.round());
    let chip = match dragout::SUPPORTED && idle {
        true => hinted(
            chip::size_handle(label, e.area_held(), Message::DragOut),
            t!(sn_drag_out),
            Why::Drag,
        ),
        false => chip::readout(label, e.area_held(), None),
    };
    items.push(container(chip).center_x(CHIP_W).into());
    items.push(divide());
    // where each panel's button is (its index among the items)
    let mut at: Vec<(Menu, usize)> = Vec::new();
    let current = info(e.tool).group;
    for (g, &last) in Group::ALL.into_iter().zip(&e.last) {
        let shown = info(info(last).slot);
        let m = Message::Slot(last);
        let s = state(idle && current == g, &m);
        at.push((Menu::Options(g), items.len()));
        // "Shapes · Arrow (A)"; Select's own name once
        let (label, why) = match g.name() == shown.name {
            true => (tip(shown), Why::Icon),
            false => (tr(g.name()), Why::Keys(tip(shown))),
        };
        items.push(hinted(
            icon_button::square(shown.icon, s, live(m)),
            label,
            why,
        ));
    }
    at.push((Menu::Colour, items.len()));
    items.push(hinted(
        colour::swatch(e.ink(), e.menu == Some(Menu::Colour), live(Message::Picker)),
        t!(sn_colour),
        Why::Icon,
    ));
    let undo = Message::UndoClick;
    let s = state(e.undid || e.menu == Some(Menu::Undo), &undo);
    at.push((Menu::Undo, items.len()));
    items.push(hinted(
        icon_button::square(UNDO, s, live(undo)),
        t!(sn_undo),
        Why::Keys("Ctrl+Z"),
    ));
    items.push(divide());
    let more = Message::Menu(Menu::More);
    let s = state(e.menu == Some(Menu::More), &more);
    at.push((Menu::More, items.len()));
    items.push(hinted(
        icon_button::square(Icon::More.glyph(), s, Some(more)),
        t!(sn_more),
        Why::Icon,
    ));
    items.push(divide());
    items.push(primary(
        e,
        focused(&Message::AddStep) || focused(&Message::Copy),
    ));
    let save = button::tool(
        SAVE,
        button::Kind::Elevated,
        focused(&Message::Save),
        Some(Message::Save),
    );
    items.push(hinted(save, t!(c_save), Why::Keys("Ctrl+S")));

    let toolbar = surface(
        match e.held {
            true => Material::Held,
            false => Material::Widget,
        },
        radius::WINDOW,
        line(items),
    )
    .padding(bar::PAD);
    let Some(menu) = e.menu else {
        return (toolbar.into(), None);
    };
    let (panel, width) = match menu {
        Menu::Colour => (colour_panel(e), colour::PICKER_W),
        Menu::Options(group) => (options_panel(e, group), size::PANEL),
        Menu::Undo => (undo_panel(e), size::PANEL),
        Menu::More => (more_panel(e), option_list::MENU_WIDTH),
    };
    // centred on its button (mirrored right to left)
    let w = widths(steps_on(e));
    let i = at.iter().find(|(m, _)| *m == menu).map_or(0, |(_, i)| *i);
    let mut x = bar::PAD + w[..i].iter().map(|w| w + SPACING).sum::<f32>();
    if dir::rtl() {
        x = size_for(e).width - x - w[i];
    }
    let left = x + w[i] / 2.0 - width / 2.0;
    (toolbar.into(), Some((panel, left)))
}

/// An option's row: its name, a muted note after it, then `value` and
/// `control` at the end.
fn option<'a>(
    name: &'a str,
    note: Option<&'a str>,
    value: Option<String>,
    control: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![caption(Role::ROW_TITLE, name).into()];
    items.extend(note.map(|n| muted(Role::META, n).into()));
    items.push(Space::new().width(Fill).into());
    items.extend(value.map(|v| sub(Role::KEYCAP, v).into()));
    items.extend(control);
    line(items).width(Fill).into()
}

/// An on / off option.
fn switch<'a>(
    name: &'a str,
    note: Option<&'a str>,
    on: bool,
    m: Option<Message>,
) -> Element<'a, Message> {
    option(name, note, None, Some(button::toggle(on, m)))
}

/// A size option: its name and value, then the presets and a slider.
fn sizes<'a>(
    name: &'a str,
    value: String,
    presets: Element<'a, Message>,
    slider: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut controls = vec![presets];
    controls.extend(slider);
    column![option(name, None, Some(value), None), line(controls)]
        .spacing(SPACING)
        .into()
}

/// The slider's room beside the presets.
const SLIDER_W: f32 = size::PANEL - 2.0 * space::PANEL - 102.0 - SPACING;

/// A tool's name in the panel.
fn name_of(tool: Tool) -> &'static str {
    tr(info(tool).name)
}

/// A group's panel (a click on its active button): the group's tools, the
/// current one's name and key, then its options. They apply to the next
/// item and to the selected one.
fn options_panel(e: &Annotator, group: Group) -> Element<'_, Message> {
    let base = info(info(e.tool).slot);
    let tools = group
        .tools()
        .map(|t| {
            icon(
                t.icon,
                base.tool == t.tool,
                Some(Message::Tool(t.tool)),
                tip(t),
            )
        })
        .collect();
    let key = base.key.to_ascii_uppercase().to_string();
    let named = line(vec![
        sub(Role::META, name_of(base.tool)).into(),
        Space::new().width(Fill).into(),
        container(keycap::keycap(key)).width(KEY_W).into(),
    ])
    .width(Fill);
    let mut parts: Vec<Element<'_, Message>> = vec![line(tools).into(), named.into()];
    let options = tool_options(e, base.tool);
    if !options.is_empty() {
        parts.push(divider::divider());
        parts.extend(options);
    }
    surface::panel(column(parts).spacing(SPACING + 2.0)).into()
}

/// `tool`'s options (one design for every tool).
fn tool_options(e: &Annotator, tool: Tool) -> Vec<Element<'_, Message>> {
    let width_at = Width::ALL.iter().position(|&w| w == e.width);
    let stroke = || {
        let px = e.stroke.round() as u8;
        let chosen = Width::ALL.iter().position(|w| w.stroke() == e.stroke);
        let presets = segmented::presets(chosen, None, false, |i| Message::Width(Width::ALL[i]));
        let slider = slider::bare(crate::config::STROKE, px, SLIDER_W, false, Message::Stroke);
        sizes(t!(sn_size), format!("{px} px"), presets, Some(slider))
    };
    let font = || {
        let px = e.text_size.round() as u8;
        let chosen = TEXT_PRESETS.iter().position(|&p| p == e.text_size);
        let presets = segmented::presets(chosen, Some("A"), false, |i| {
            Message::TextSize(TEXT_PRESETS[i])
        });
        let slider = slider::bare(crate::config::TEXT_SIZE, px, SLIDER_W, false, |v| {
            Message::TextSize(f32::from(v))
        });
        sizes(t!(sn_font_size), format!("{px} px"), presets, Some(slider))
    };
    let outline = || {
        switch(
            t!(sn_outline),
            None,
            e.outline,
            Some(Message::Outline(!e.outline)),
        )
    };
    // a flipped option of the tool (an arrow's label, a step's pointer)
    let variant = |name, plain: Tool, other: Tool| {
        let on = e.tool == other;
        let to = if on { plain } else { other };
        switch(name, None, on, Some(Message::Tool(to)))
    };
    match tool {
        Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow => {
            let shape = crate::snip::annotate::fills_inside(tool);
            let mut o = vec![stroke(), outline()];
            // shown dimmed for lines, so the panel doesn't jump
            let note = (!shape).then(|| t!(sn_fill_note));
            let fill = shape.then_some(Message::Fill(!e.fill));
            o.push(switch(t!(sn_fill), note, e.fill && shape, fill));
            if tool == Tool::Arrow {
                o.push(variant(t!(sn_label), Tool::Arrow, Tool::ArrowLabel));
            }
            o
        }
        Tool::Pen | Tool::Highlighter => vec![stroke()],
        Tool::Text => {
            use crate::snip::annotate::{TextBack as B, TextFont as F, TextStyle};
            let style = e.text_style;
            let bold = TextStyle {
                bold: !style.bold,
                ..style
            };
            let faces = [
                (F::Sans, t!(sn_sans)),
                (F::Serif, t!(sn_serif)),
                (F::Mono, t!(sn_mono)),
            ];
            let backs = [
                (B::None, t!(sn_none)),
                (B::Outline, t!(sn_back_outline)),
                (B::Box, t!(sn_box)),
            ];
            let face = segmented::inline(&faces, style.font, style.font, 1.0, move |font| {
                Message::TextStyle(TextStyle { font, ..style })
            });
            let back = segmented::inline(&backs, style.back, style.back, 1.0, move |back| {
                Message::TextStyle(TextStyle { back, ..style })
            });
            vec![
                font(),
                option(t!(sn_font), None, None, Some(face)),
                switch(
                    t!(sn_bold),
                    None,
                    style.bold,
                    Some(Message::TextStyle(bold)),
                ),
                option(t!(sn_back), None, None, Some(back)),
            ]
        }
        Tool::Callout => vec![font(), outline()],
        Tool::Step => {
            let presets =
                segmented::presets(width_at, None, false, |i| Message::Width(Width::ALL[i]));
            vec![
                sizes(t!(sn_size), String::new(), presets, None),
                variant(t!(sn_pointer), Tool::Step, Tool::Pointer),
                switch(t!(sn_legend), None, e.doc.legend, Some(Message::Legend)),
            ]
        }
        Tool::Blur => {
            let auto = e.blur_size.is_none();
            let at = e
                .blur_size
                .and_then(|w| Width::ALL.iter().position(|&x| x == w));
            let brush =
                segmented::presets(at, None, auto, |i| Message::BlurSize(Some(Width::ALL[i])));
            let to = if auto { Some(Width::Medium) } else { None };
            let chip = button::switch(t!(sn_auto), auto, Message::BlurSize(to));
            let value = if auto { t!(sn_line) } else { "" };
            let cell = Width::ALL.iter().position(|&w| w == e.blur_cell);
            let cells = segmented::presets(cell, None, false, |i| Message::BlurCell(Width::ALL[i]));
            let px = e.blur_cell.cell();
            vec![
                column![
                    option(t!(sn_brush), None, Some(value.into()), Some(chip)),
                    line(vec![brush]),
                ]
                .spacing(SPACING)
                .into(),
                sizes(t!(sn_pixel), format!("{px} px"), cells, None),
                switch(
                    t!(sn_line_help),
                    Some(t!(sn_stays)),
                    e.assist,
                    Some(Message::Assist),
                ),
            ]
        }
        Tool::Redact => vec![muted(Role::META, t!(sn_uses_colour)).into()],
        _ => Vec::new(),
    }
}

/// Undo's second click: Undo, Redo and where the history is.
fn undo_panel(e: &Annotator) -> Element<'_, Message> {
    let (done, all) = e.doc.steps();
    let steps = fmt(S::sn_steps_of, &[&done, &all]);
    let items = vec![
        icon(
            UNDO,
            false,
            (done > 0).then_some(Message::Undo),
            t!(sn_undo),
        ),
        icon(
            REDO,
            false,
            (done < all).then_some(Message::Redo),
            t!(sn_redo),
        ),
        Space::new().width(Fill).into(),
        sub(Role::META, steps).into(),
    ];
    surface::panel(line(items).width(Fill)).into()
}

/// The primary: Copy, or while the Steps strip is open "Add step N" (the
/// next step), "Update step N" (one being edited) or "Steps full" (dimmed).
fn primary<'a>(e: &Annotator, focused: bool) -> Element<'a, Message> {
    let accent = button::Kind::Accent;
    if !steps_on(e) {
        let copy = button::tool(COPY, accent, focused, Some(Message::Copy));
        return hinted(copy, t!(c_copy), Why::Keys("Ctrl+C"));
    }
    let n = e.steps.unwrap_or(0);
    let (label, tip, why) = match e.updating {
        Some(i) => (
            step_label(S::sn_update_step, i + 1),
            t!(sn_update_tip),
            Why::Keys("Enter"),
        ),
        None if !e.adds_step() => (
            t!(sn_steps_full).to_string(),
            t!(sn_steps_full_tip),
            Why::Icon,
        ),
        None => (
            step_label(S::sn_add_step, n + 1),
            t!(sn_add_tip),
            Why::Keys("Enter"),
        ),
    };
    let on = e.adds_step().then_some(Message::AddStep);
    let add = button::labelled(LIST_PLUS, label, PRIMARY_W, accent, focused, on);
    hinted(add, tip, why)
}

/// "Add step 2": the words, then the number.
fn step_label(words: S, n: usize) -> String {
    format!("{} {n}", tr(words))
}

/// More's width: 268 px, 304 in Arabic (the longest label grows it).
pub const MORE_W: f32 = 268.0;
pub const MORE_W_AR: f32 = 304.0;

/// More: the three picture actions, a hairline, then long capture, pin,
/// share, the screen tools (arrow keys walk it); its foot says why a row is
/// off (arrow keys skip those) or where Ask AI goes.
fn more_panel(e: &Annotator) -> Element<'_, Message> {
    let rtl = dir::rtl();
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    for (i, r) in more_rows(e).into_iter().enumerate() {
        if i == TEXT_ROWS {
            items.push(option_list::separator());
        }
        let label = tr(r.name);
        let lead = match r.of {
            Some(Row::AskAi) => option_list::logo_mark(16),
            _ => option_list::glyph(r.glyph, false),
        };
        let reading = r.of.is_some_and(|row| {
            e.more.read == Read::Reading && matches!(row, Row::CopyText | Row::Translate)
        });
        items.push(option_list::action(option_list::ActionRow {
            lead,
            label,
            external: r.of == Some(Row::AskAi) && e.more.browser,
            caps: r.caps,
            reading,
            focused: e.row == Some(i),
            press: r.message,
        }));
    }
    let foot = snip_more::foot(&e.more, e.capturing).map(|words| option_list::foot(tr(words)));
    option_list::action_menu(if rtl { MORE_W_AR } else { MORE_W }, items, foot)
}

/// Between the toolbar and its panel.
const POPOVER_GAP: f32 = 8.0;

/// THE placement of every toolbar panel: `at` = where it goes by its button,
/// `bar_height` = the toolbar's. Flipped to the bar's other side when it runs
/// off the screen there; always kept inside `screen`. Returns the shift.
pub fn place_popover(at: Rectangle, bar_height: f32, screen: Rectangle) -> Vector {
    let mut p = at.position();
    let flip = at.height + bar_height + 2.0 * POPOVER_GAP;
    if p.y + at.height > screen.y + screen.height {
        p.y -= flip;
    } else if p.y < screen.y {
        p.y += flip;
    }
    let clamp = |v: f32, lo: f32, hi: f32| v.min(hi).max(lo);
    p.x = clamp(
        p.x,
        screen.x + 8.0,
        screen.x + screen.width - 8.0 - at.width,
    );
    p.y = clamp(p.y, screen.y, screen.y + screen.height - at.height);
    p - at.position()
}

/// The kit's colour picker with the eyedropper, the presets (keys 1-9) and
/// copy / paste style under its hex field.
fn colour_panel(e: &Annotator) -> Element<'_, Message> {
    let eyedropper = hinted(
        icon_button::square(PIPETTE, State::default(), Some(Message::Eyedropper)),
        t!(sn_pick_image),
        Why::Keys("D"),
    );
    let style = line(vec![
        eyedropper,
        chip::action(t!(sn_copy_style).into(), Message::CopyStyle),
        chip::action(t!(sn_paste_style).into(), Message::PasteStyle),
    ]);
    let preset = |i: usize| -> Element<'_, Message> {
        let p = &PRESETS[i];
        hinted(
            chip::action(tr(p.short).into(), Message::Preset(i)),
            tr(p.name),
            Why::Keys(p.key),
        )
    };
    let pairs = (0..PRESETS.len())
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| line(pair.iter().map(|&i| preset(i)).collect()).into())
        .collect::<Vec<Element<'_, Message>>>();
    let more = column![
        style,
        caption(Role::SECTION, t!(sn_presets)),
        column(pairs).spacing(SPACING)
    ]
    .spacing(SPACING);
    colour::picker_with(
        &crate::snip::annotate::style::ANNOTATION,
        e.ink(),
        &e.hex,
        |c| Message::Colour(c.into_rgba8()),
        Message::Hex,
        Some(more.into()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::snip_more::Info;
    use crate::snip::annotate::Image;
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    fn editor() -> Annotator {
        let image = Image {
            width: 688,
            height: 330,
            rgba: vec![255; 688 * 330 * 4],
        };
        let mut e = Annotator::new(image, 1.0);
        e.movable = true;
        e
    }

    /// The bar, each group's panel, Colour, Undo's and More, the drag states
    /// (the toolbar held; the area held: its size live), in Sapphire and
    /// Daylight, Latin and Arabic (right to left).
    #[test]
    fn the_toolbar_matches_its_golden_images() {
        let mut failed = Vec::new();
        let panel = |tool: Tool| {
            let mut e = editor();
            e.update(Message::Tool(tool));
            e.update(Message::Slot(tool));
            e
        };
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for rtl in [false, true] {
                let _ar = rtl.then(crate::ui::kit::testing::arabic);
                let mut colour = editor();
                colour.update(Message::Picker);
                let mut undo = editor();
                undo.update(Message::UndoClick);
                undo.update(Message::UndoClick);
                // More: the three picture actions with the user's own keys, and
                // each state of the design (reading, no text, the browser, a
                // long capture, the translation on)
                let more_with = |info: Info, capturing: bool| {
                    let mut e = editor();
                    e.more = Info {
                        caps: snip_more::caps_from(&crate::config::Config::default()),
                        ..info
                    };
                    e.capturing = capturing;
                    e.update(Message::Menu(Menu::More));
                    e
                };
                let state = |read, browser, translated| Info {
                    read,
                    browser,
                    translated,
                    ..Info::default()
                };
                let more = more_with(state(Read::Text, false, false), false);
                let more_reading = more_with(state(Read::Reading, false, false), false);
                let more_notext = more_with(state(Read::NoText, false, false), false);
                let more_browser = more_with(state(Read::Text, true, false), false);
                let more_long = more_with(state(Read::Text, false, false), true);
                let more_original = more_with(state(Read::Text, false, true), false);
                let mut held = editor();
                held.held = true;
                // the Steps strip open: Add step 2, full, a step edited, More
                let with = |steps: Option<usize>, updating: Option<usize>| {
                    let mut e = editor();
                    (e.steps, e.updating) = (steps, updating);
                    e
                };
                let mut add_more = with(Some(1), None);
                add_more.update(Message::Menu(Menu::More));
                let cases: Vec<(&str, Annotator, f32)> = vec![
                    ("add-step", with(Some(1), None), 60.0),
                    ("steps-full", with(Some(12), None), 60.0),
                    ("update-step", with(Some(3), Some(1)), 60.0),
                    ("add-step-more", add_more, 430.0),
                    ("bar", editor(), 60.0),
                    ("held", held, 60.0),
                    ("shapes", panel(Tool::Arrow), 330.0),
                    ("rect", panel(Tool::Rect), 330.0),
                    ("draw", panel(Tool::Pen), 230.0),
                    ("text", panel(Tool::Text), 360.0),
                    ("steps", panel(Tool::Step), 330.0),
                    ("hide", panel(Tool::Blur), 360.0),
                    ("colour", colour, 440.0),
                    ("undo", undo, 130.0),
                    ("more", more, 430.0),
                    ("more-reading", more_reading, 430.0),
                    ("more-notext", more_notext, 430.0),
                    ("more-browser", more_browser, 430.0),
                    ("more-long", more_long, 430.0),
                    ("more-original", more_original, 430.0),
                ];
                let dir = if rtl { "-ar" } else { "" };
                for (name, e, height) in cases {
                    let file = format!("snip-toolbar-{name}{dir}-{}", theme.id);
                    let at = Size::new(size_for(&e).width, height);
                    failed.extend(golden::check("screens", &file, at, &[1.0], || view(&e)));
                }
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
