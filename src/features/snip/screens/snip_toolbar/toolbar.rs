//! The editor's toolbar: the grouped Snip toolbar (owner approved, 2026-11-02;
//! SNIP_TOOLBAR_SPEC), composed from kit parts only. One bar, always the same:
//! grip (moves the toolbar) · size chip (drags the picture out) · Select ·
//! Shapes · Draw · Text · Hide · Colour · Undo · More · Copy · Save. A group
//! button shows its last tool; the active tool's choices (its group's tools)
//! and options are in the row under the bar (`options_row`), no panel, no second click.
//! The quiet x at the end, and Esc, close.
use iced::widget::{Space, column, container, tooltip};
#[cfg(test)]
use iced::widget::{Space as Gap, row};
use iced::{Element, Fill, Point, Rectangle, Size, Vector};

use crate::features::circle::actions::share;
use crate::features::circle::screens::bar::geometry as bar;
use crate::features::snip::annotate::{
    Annotator, Group, Menu, Message, PRESETS, ScreenTool, Tool, ToolInfo, dragout, info,
};
use crate::features::snip::snip_more::{self, Read, Row};
use crate::shared::app_language::{S, fmt, tr};
use crate::shared::kit::dir;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::surface::{self, segment_row, tray};
use crate::shared::kit::text::{muted, sub, text as caption};
use crate::shared::kit::tokens::{Role, radius, size, space, widget_e};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{
    button, chip, colour, colour_picker, grip, option_list, segmented, slider,
};
use crate::t;

const SPACING: f32 = bar::SPACING;
/// A button's side.
const CONTROL: f32 = size::ICON_BUTTON;
/// The size chip's room (fits `[picture] 9999 × 9999`).
const CHIP_W: f32 = 112.0;
pub const UNDO: char = '\u{e2a0}'; // lucide "undo-2"
pub const REDO: char = '\u{e29f}'; // lucide "redo-2"
pub const COPY: char = '\u{e0a2}'; // lucide "copy"
pub const SAVE: char = '\u{e0b6}'; // lucide "download"
const PIN: char = '\u{e258}'; // lucide "pin"
const SCROLL: char = '\u{e4d6}'; // lucide "gallery-vertical-end"
const PIPETTE: char = crate::features::snip::screens::tools::picker::ICON;
const SHARE: char = '\u{e158}'; // lucide "share"
const LIVE_PIN: char = '\u{e3b2}'; // lucide "picture-in-picture-2"
const LIST_PLUS: char = '\u{e23e}'; // lucide "list-plus"
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
    crate::features::snip::screens::tools::ruler::ICON,
    LIVE_PIN,
    LIST_PLUS,
];

/// Snip's Live pin (Windows; Labs Pins); it works only over the live screen
/// (`Annotator::scroll`). Still pins left Labs (owner, 2026-10-31): Pin is always there.
fn live_pin() -> bool {
    crate::features::settings::labs::has(crate::features::settings::labs::Feature::Pins)
        && crate::features::snip::capture::live::SUPPORTED
}

/// The labelled primary in Copy's place while the Steps strip is open
/// (fits "Update step 12").
const PRIMARY_W: f32 = 128.0;

/// Extra width added by one style E raised segment (padding + 1 px border each side).
const SEG_EXTRA: f32 = 2.0 * widget_e::SEGMENT_PAD_X;
const SEG_GAP: f32 = widget_e::SEGMENT_GAP;

/// The four raised segments' widths, and within each segment its items' widths,
/// matching `build` so `size()` and `centred_on()` stay exact.
fn group_widths(primary: bool) -> [Vec<f32>; 4] {
    let g0 = vec![bar::HANDLE, CHIP_W];
    let mut g1: Vec<f32> = Group::ALL.map(|_| CONTROL).to_vec();
    g1.extend([CONTROL, CONTROL]); // Colour, Undo
    let g2 = vec![CONTROL]; // More
    let g3 = vec![
        CONTROL,                                   // Open in Studio
        if primary { PRIMARY_W } else { CONTROL }, // Copy / Add step N
        CONTROL,                                   // Save
        widget_e::QUIET_X,                         // quiet x
    ];
    [g0, g1, g2, g3]
}

fn seg_width(items: &[f32]) -> f32 {
    SEG_EXTRA + items.iter().sum::<f32>() + SEG_GAP * items.len().saturating_sub(1) as f32
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
    let groups = group_widths(primary);
    let line =
        groups.iter().map(|g| seg_width(g)).sum::<f32>() + space::GAP * (groups.len() - 1) as f32;
    Size::new(line + 2.0 * widget_e::TRAY_PAD_X, bar::HEIGHT)
}

/// The Steps strip is open, or one of its steps is edited.
fn steps_on(e: &Annotator) -> bool {
    e.steps.is_some() || e.updating.is_some()
}

/// What Tab walks on the toolbar, in its order (left to right), and what
/// Enter / Space on each sends.
pub fn stops(e: &Annotator) -> Vec<Message> {
    let mut stops = bar_stops(e);
    // the options row's controls follow the toolbar's (C4)
    if !e.capturing {
        stops.extend(crate::features::snip::screens::options_row::snip_row_stops(
            e,
        ));
    }
    stops
}

/// The toolbar's own stops (the row's come after them).
pub fn bar_stops(e: &Annotator) -> Vec<Message> {
    let mut stops: Vec<Message> = e.last.iter().map(|&t| Message::Slot(t)).collect();
    stops.extend([
        Message::Picker,
        Message::UndoClick,
        Message::Menu(Menu::More),
    ]);
    // (a long capture running: Open in Studio is dimmed, so Tab skips it)
    if !e.capturing {
        stops.push(Message::OpenStudio);
    }
    stops.extend([
        if steps_on(e) {
            Message::AddStep
        } else {
            Message::Copy
        },
        Message::Save,
        Message::Close,
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
    /// Why a dimmed row is off, in its tooltip (F15; the text rows say it in the foot).
    pub why: Option<S>,
}

impl MoreRow {
    fn plain(glyph: char, name: S, key: Option<&'static str>, message: Option<Message>) -> Self {
        MoreRow {
            glyph,
            name,
            caps: key.map(snip_more::caps).unwrap_or_default(),
            message,
            of: None,
            why: None,
        }
    }
}

/// The rows before the hairline: Copy text, Ask AI, Translate.
pub const TEXT_ROWS: usize = 3;
/// The style rows at the foot of More (a hairline above them): Copy style, Paste style, the presets.
pub const STYLE_ROWS: usize = 2 + PRESETS.len();

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
                why: None,
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
        crate::features::snip::screens::tools::ruler::ICON,
        S::sn_ruler,
        None,
        live(Message::ScreenTool(ScreenTool::Ruler)),
    ));
    // the style tools (not in the colour picker): copy and paste a style, the presets (keys 1-4)
    rows.push(MoreRow::plain(
        COPY,
        S::sn_copy_style,
        Some("Alt+C"),
        live(Message::CopyStyle),
    ));
    rows.push(MoreRow::plain(
        LIST_PLUS,
        S::sn_paste_style,
        Some("Alt+V"),
        live(Message::PasteStyle),
    ));
    for (i, p) in PRESETS.iter().enumerate() {
        rows.push(MoreRow::plain(
            crate::features::snip::annotate::info(p.tool).icon,
            p.short,
            Some(p.key),
            live(Message::Preset(i)),
        ));
    }
    // a dimmed row says why in its tooltip (F15)
    for r in rows
        .iter_mut()
        .filter(|r| r.of.is_none() && r.message.is_none())
    {
        r.why = match (e.capturing, r.name) {
            (true, _) => Some(S::sn_why_capturing),
            (false, S::sn_long_capture) => Some(S::sn_why_no_long),
            _ => None,
        };
    }
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
/// panel goes, away from Snip's area; `tips`: where the tooltips go, away from the row too).
pub fn view_at(
    e: &Annotator,
    bar: Rectangle,
    side: tooltip::Position,
    tips: tooltip::Position,
) -> (Element<'_, Message>, Option<Element<'_, Message>>) {
    let (toolbar, panel) = build(e, tips);
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
pub(crate) fn tip(t: &'static ToolInfo) -> &'static str {
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

/// The toolbar (style E: four raised `widget` segments on a recessed `track`
/// tray, no dividers), and the open panel with its left edge (from the toolbar's).
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

    // Segment 0: drag grip + size pill
    let px = e.doc.view().size();
    let label = format!("{} × {}", px.width.round(), px.height.round());
    let chip = match dragout::SUPPORTED && idle {
        true => hinted(
            chip::size_pill(label, e.area_held(), Message::DragOut),
            t!(sn_drag_out),
            Why::Drag,
        ),
        false => chip::size_pill_static(label, e.area_held()),
    };
    let seg_lead = segment_row([
        hinted(grip::mover(Message::Grab), t!(sn_move), Why::Icon),
        container(chip).center_x(CHIP_W).into(),
    ]);

    // Segment 1: tool groups + colour swatch + undo
    let mut tools: Vec<Element<'_, Message>> = Vec::new();
    let current = info(e.tool).group;
    for (g, &last) in Group::ALL.into_iter().zip(&e.last) {
        let shown = info(info(last).slot);
        let m = Message::Slot(last);
        let s = state(idle && current == g, &m);
        // "Shapes · Arrow (A)"; Select's own name once
        let (label, why) = match g.name() == shown.name {
            true => (tip(shown), Why::Icon),
            false => (tr(g.name()), Why::Keys(tip(shown))),
        };
        tools.push(hinted(
            icon_button::tool_icon(shown.icon, s, live(m)),
            label,
            why,
        ));
    }
    let colour_at = (1usize, tools.len());
    tools.push(hinted(
        colour::swatch_e(e.ink(), e.menu == Some(Menu::Colour), live(Message::Picker)),
        t!(sn_colour),
        Why::Icon,
    ));
    let undo = Message::UndoClick;
    let s = state(e.undid || e.menu == Some(Menu::Undo), &undo);
    let undo_at = (1usize, tools.len());
    tools.push(hinted(
        icon_button::tool_icon(UNDO, s, live(undo)),
        t!(sn_undo),
        Why::Keys("Ctrl+Z"),
    ));
    let seg_tools = segment_row(tools);

    // Segment 2: More
    let more = Message::Menu(Menu::More);
    let s = state(e.menu == Some(Menu::More), &more);
    let more_at = (2usize, 0usize);
    let seg_more = segment_row([hinted(
        icon_button::tool_icon(Icon::More.glyph(), s, Some(more)),
        t!(sn_more),
        Why::Icon,
    )]);

    // Segment 3: Studio (OpenCircle logo) + Copy/Add step + Save + quiet x
    let studio = State {
        active: false,
        focused: focused(&Message::OpenStudio),
    };
    let studio_logo = iced::widget::image(crate::shared::kit::logo::logo(size::ICON as u32))
        .width(size::ICON)
        .height(size::ICON)
        .opacity(if idle { 1.0 } else { 0.45_f32 });
    let save = icon_button::tool_icon(
        SAVE,
        State {
            active: false,
            focused: focused(&Message::Save),
        },
        Some(Message::Save),
    );
    let close = Message::Close;
    let seg_actions = segment_row([
        hinted(
            icon_button::tool_option(studio_logo, studio, live(Message::OpenStudio)),
            t!(sn_studio_btn),
            Why::Keys("Ctrl+E"),
        ),
        primary(e, focused(&Message::AddStep) || focused(&Message::Copy)),
        hinted(save, t!(c_save), Why::Keys("Ctrl+S")),
        hinted(
            icon_button::quiet_close(focused(&close), live(close)),
            t!(sn_close),
            Why::Icon,
        ),
    ]);

    let segments = dir::row([seg_lead, seg_tools, seg_more, seg_actions])
        .spacing(space::GAP)
        .align_y(iced::Center);
    let toolbar = tray(e.held, radius::FULL, segments)
        .padding([widget_e::TRAY_PAD_Y, widget_e::TRAY_PAD_X])
        .height(bar::HEIGHT);
    let Some(menu) = e.menu else {
        // the first Snip ever: a one-time hint by the Studio button (seg 3, item 0)
        let hint = (e.studio_hint && idle).then(|| {
            let (_, left) = centred_on(e, (3, 0), STUDIO_HINT_W);
            (studio_hint(), left)
        });
        return (toolbar.into(), hint);
    };
    let (panel, width, target) = match menu {
        Menu::Colour => (colour_panel(e), colour::PICKER_W, colour_at),
        Menu::Undo => (undo_panel(e), size::PANEL, undo_at),
        Menu::More => (more_panel(e), option_list::MENU_WIDTH, more_at),
    };
    (
        toolbar.into(),
        Some((panel, centred_on(e, target, width).1)),
    )
}

/// The one-time hint's width, and the bubble (Snip's first run).
const STUDIO_HINT_W: f32 = 232.0;

fn studio_hint<'a>() -> Element<'a, Message> {
    tray(
        false,
        radius::WINDOW,
        container(caption(Role::ROW_TITLE, t!(sn_studio_hint))).padding(space::PANEL),
    )
    .width(STUDIO_HINT_W)
    .into()
}

/// Segment `seg`, item `item`'s centre and a `width`-wide panel's left edge
/// to sit centred on it (mirrored right to left).
fn centred_on(e: &Annotator, (seg, item): (usize, usize), width: f32) -> (f32, f32) {
    let groups = group_widths(steps_on(e));
    let mut x = widget_e::TRAY_PAD_X;
    for g in &groups[..seg] {
        x += seg_width(g) + space::GAP;
    }
    x += widget_e::SEGMENT_PAD_X;
    let items = &groups[seg];
    x += items[..item].iter().map(|w| w + SEG_GAP).sum::<f32>();
    if dir::rtl() {
        x = size_for(e).width - x - items[item];
    }
    let centre = x + items[item] / 2.0;
    (centre, centre - width / 2.0)
}

/// The font size option (presets and slider; `Message::TextSize`): the Text
/// tool's, and Studio's Edit text's.
pub fn text_size_option<'a>(size: f32) -> Element<'a, Message> {
    let px = size.round() as u8;
    let chosen = TEXT_PRESETS.iter().position(|&p| p == size);
    let presets = segmented::presets(chosen, Some("A"), false, |i| {
        Message::TextSize(TEXT_PRESETS[i])
    });
    let slider = slider::bare(crate::shared::config::TEXT_SIZE, px, SLIDER_W, false, |v| {
        Message::TextSize(f32::from(v))
    });
    sizes(t!(sn_font_size), format!("{px} px"), presets, Some(slider))
}

/// The Bold switch (`Message::TextStyle` with the flipped weight).
pub fn bold_option<'a>(style: crate::features::snip::annotate::TextStyle) -> Element<'a, Message> {
    let flipped = crate::features::snip::annotate::TextStyle {
        bold: !style.bold,
        ..style
    };
    switch(
        t!(sn_bold),
        None,
        style.bold,
        Some(Message::TextStyle(flipped)),
    )
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

/// The row under the toolbar (UX C4): its room, and the gap above it.
/// Attached directly under (or above) the Snip toolbar as one piece (owner, 2026-11-26).
pub const ROW_GAP: f32 = 0.0;
pub const ROW_H: f32 = crate::shared::kit::option_bar::HEIGHT + 8.0;
pub const ROW_ROOM: f32 = ROW_GAP + ROW_H;

/// The widest an options row is meant to get (Hide's, at 100 %): the room the host keeps for it.
pub const ROW_WIDE: f32 = 1250.0;

/// About how wide the active tool's row is (the room the host asks for it; the row itself is as wide
/// as its content, and scrolls only past what the screen gives): Hide's is the widest, Text's next.
pub fn row_need(e: &Annotator) -> f32 {
    match crate::features::snip::screens::options_row::base_tool(e) {
        Tool::Blur => ROW_WIDE,
        Tool::Text => 860.0,
        Tool::Magnifier => 760.0,
        _ => 620.0,
    }
}

/// Where the row goes along the screen (W107, owner 2026-11-25): `(x0, x1)`, the strip it is centred in.
/// Its centre is the toolbar's centre (the same right to left); only when that would push it off the
/// screen's margins does it shift inward, still under the toolbar.
pub fn row_span(at_x: f32, bar_w: f32, screen_w: f32, need: f32) -> (f32, f32) {
    use crate::features::circle::screens::bar::geometry::GAP;
    let (lo, hi) = (GAP, screen_w - GAP);
    let need = need.min(hi - lo);
    let centre = at_x + bar_w / 2.0;
    let half = (centre - lo).min(hi - centre);
    if half * 2.0 >= need {
        return (centre - half, centre + half);
    }
    let x0 = (centre - need / 2.0).clamp(lo, hi - need);
    (x0, x0 + need)
}

/// The active tool's options in a slim row, at most `max_w` wide (it scrolls across past that, as the
/// Studio's bar does; `None`: it has none, or a long capture is running): the Studio's option parts,
/// on the toolbar's surface.
#[cfg(test)]
pub fn options_row(e: &Annotator, max_w: f32) -> Option<Element<'_, Message>> {
    options_row_tips(e, max_w, tooltip::Position::Bottom)
}

/// `options_row`, its tooltips opening on `tips` (away from the toolbar and the area).
pub fn options_row_tips(
    e: &Annotator,
    max_w: f32,
    tips: tooltip::Position,
) -> Option<Element<'_, Message>> {
    if e.capturing {
        return None;
    }
    crate::shared::kit::option_bar::set_tips(tips);
    // the focus ring on the row's control the keyboard is on
    let on = e.focus.and_then(|f| f.checked_sub(bar_stops(e).len()));
    let row = crate::features::snip::screens::options_row::snip_row_focused(e, on)?;
    Some(
        tray(false, radius::FULL, crate::shared::kit::scroll::across(row))
            .padding([4.0, widget_e::TRAY_PAD_X])
            .height(ROW_H)
            .max_width(max_w)
            .align_y(iced::Center)
            .into(),
    )
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
    let all = more_rows(e);
    let n = all.len();
    for (i, r) in all.into_iter().enumerate() {
        if i == TEXT_ROWS || i + STYLE_ROWS == n {
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
        let row = option_list::action(option_list::ActionRow {
            lead,
            label,
            external: r.of == Some(Row::AskAi) && e.more.browser,
            caps: r.caps,
            reading,
            focused: e.row == Some(i),
            press: r.message,
        });
        items.push(match r.why {
            Some(why) => crate::shared::kit::tooltip::cut(row, tr(why).to_string()),
            None => row,
        });
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

/// The kit's colour picker (the square, the hue slider, the preview, the recent colours and
/// the preset swatches) with the eyedropper. Copy / Paste style and the style presets are
/// not in it (W112): they are rows of Snip's More and of the Studio's Edit menu.
pub(crate) fn colour_panel(e: &Annotator) -> Element<'_, Message> {
    let eyedropper = hinted(
        icon_button::square(PIPETTE, State::default(), Some(Message::Eyedropper)),
        t!(sn_pick_image),
        Why::Keys("D"),
    );
    let hsv = e.hsv().map_or_else(
        || {
            let [r, g, b, _] = e.colour;
            colour_picker::Hsv::of([r, g, b])
        },
        colour_picker::Hsv::from_array,
    );
    colour_picker::view(colour_picker::Picker {
        hsv,
        old: e.picked_from(),
        hex: &e.hex,
        presets: &crate::features::snip::annotate::style::ANNOTATION,
        recent: e.recent.clone(),
        eyedropper: Some(eyedropper),
        recent_label: t!(cp_recent),
        on_hsv: std::rc::Rc::new(|h| Message::Hsv(h.array(), h.rgb())),
        done: Message::HsvDone,
        on_set: std::rc::Rc::new(|h| Message::HsvSet(h.array(), h.rgb())),
        on_rgb: std::rc::Rc::new(|[r, g, b]| Message::Colour([r, g, b, 255])),
        on_hex: std::rc::Rc::new(Message::Hex),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::snip::annotate::{Image, Output};
    use crate::features::snip::snip_more::Info;
    use crate::shared::kit::testing::golden;
    use crate::shared::kit::themes::{self, DAYLIGHT, SAPPHIRE};

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

    /// A measurement, not a check: the toolbar with its Text panel open, English
    /// and Arabic (`cargo test toolbar_frame_cost -- --ignored --nocapture`).
    #[test]
    #[ignore = "a measurement"]
    fn toolbar_frame_cost() {
        let cost = |ar: bool| {
            let _dir = ar.then(crate::shared::kit::testing::arabic);
            let mut e = editor();
            e.update(Message::Tool(Tool::Text));
            e.update(Message::Slot(Tool::Text));
            let at = (size_for(&e).width as u32, 360);
            let _ = crate::shared::kit::testing::shot::render(view(&e), at.0, at.1); // warm
            let t = std::time::Instant::now();
            for _ in 0..40 {
                let _ = crate::shared::kit::testing::shot::render(view(&e), at.0, at.1);
            }
            t.elapsed().as_secs_f64() * 1000.0 / 40.0
        };
        let (en, ar) = (cost(false), cost(true));
        println!("toolbar + Text panel (build+layout+draw): English {en:.2} ms, Arabic {ar:.2} ms");
    }

    /// The bar, each group's panel, Colour, Undo's and More, the drag states
    /// (the toolbar held; the area held: its size live), in Sapphire and
    /// Daylight, Latin and Arabic (right to left).
    #[test]
    fn the_toolbar_matches_its_golden_images() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for rtl in [false, true] {
                let _ar = rtl.then(crate::shared::kit::testing::arabic);
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
                        caps: snip_more::caps_from(&crate::shared::config::Config::default()),
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
                let mut hint = editor();
                hint.studio_hint = true;
                let cases: Vec<(&str, Annotator, f32)> = vec![
                    ("studio-hint", hint, 110.0),
                    ("add-step", with(Some(1), None), 60.0),
                    ("steps-full", with(Some(12), None), 60.0),
                    ("update-step", with(Some(3), Some(1)), 60.0),
                    ("add-step-more", add_more, 560.0),
                    ("bar", editor(), 60.0),
                    ("held", held, 60.0),
                    ("colour", colour, 440.0),
                    ("undo", undo, 130.0),
                    ("more", more, 560.0),
                    ("more-reading", more_reading, 560.0),
                    ("more-notext", more_notext, 560.0),
                    ("more-browser", more_browser, 560.0),
                    ("more-long", more_long, 560.0),
                    ("more-original", more_original, 560.0),
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

    /// W112: the familiar colour picker (square, hue slider, before | now preview, eyedropper, the
    /// hex as a small read-out, recent colours, presets) in Sapphire and Daylight, Latin and
    /// Arabic; the same panel the Studio's "+" opens. No style tools in it.
    #[test]
    fn w112_the_colour_picker_matches_its_golden_images() {
        use crate::shared::kit::colour_picker::Hsv;
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for rtl in [false, true] {
                let _ar = rtl.then(crate::shared::kit::testing::arabic);
                let mut e = editor();
                e.update(Message::Colour([0xff, 0x95, 0x00, 255]));
                let blue = Hsv {
                    h: 212.0,
                    s: 0.82,
                    v: 0.96,
                };
                e.update(Message::HsvSet(blue.array(), blue.rgb()));
                e.recent = vec![
                    [0x0a, 0x84, 0xff],
                    [0x34, 0xc7, 0x59],
                    [0xaf, 0x52, 0xde],
                    [0xff, 0x2d, 0x9a],
                    [0x1c, 0x1c, 0x1e],
                ];
                let words = crate::shared::kit::testing::shot::texts(colour_panel(&e));
                assert!(
                    !words.iter().any(|w| w == "Copy style" || w == "Presets"),
                    "{words:?}"
                );
                let dir = if rtl { "-ar" } else { "" };
                let file = format!("snip-colour-picker{dir}-{}", theme.id);
                let at = Size::new(colour::PICKER_W, 336.0);
                failed.extend(golden::check("screens", &file, at, &[1.0], || {
                    colour_panel(&e)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// W112 review: Copy style, Paste style and the four style presets are rows of More (one list,
    /// a hairline above them), each with its key; they send their messages and are not in the picker.
    #[test]
    fn w112_more_has_the_style_rows() {
        let e = editor();
        let rows = more_rows(&e);
        let style = &rows[rows.len() - STYLE_ROWS..];
        let names: Vec<&str> = style.iter().map(|r| tr(r.name)).collect();
        assert_eq!(
            names,
            [
                "Copy style",
                "Paste style",
                "Red box",
                "Highlight",
                "Callout",
                "Redact"
            ]
        );
        assert_eq!(style[0].message, Some(Message::CopyStyle));
        assert_eq!(style[1].message, Some(Message::PasteStyle));
        for (i, r) in style[2..].iter().enumerate() {
            assert_eq!(r.message, Some(Message::Preset(i)));
            assert_eq!(r.caps, vec![PRESETS[i].key.to_string()]);
        }
        assert_eq!(style[0].caps, vec!["Alt".to_string(), "C".to_string()]);
        let mut open = editor();
        open.update(Message::Menu(Menu::More));
        let words = crate::shared::kit::testing::shot::texts(more_panel(&open));
        assert!(words.iter().any(|w| w == "Copy style"), "{words:?}");
    }

    /// W107-1: the row is one piece with the toolbar: its centre on the toolbar's centre, the same in
    /// Arabic; shifted inward only when centring would leave the screen's margins, and always given its room.
    #[test]
    fn w107_1_the_row_is_centred_on_the_toolbar() {
        use crate::features::circle::screens::bar::geometry::GAP;
        // the owner's shot: a centred toolbar, a short row: the strip is symmetric about the toolbar's centre
        let (x0, x1) = row_span(400.0, 900.0, 1600.0, 620.0);
        assert_eq!((x0 + x1) / 2.0, 850.0, "{x0} {x1}");
        assert!(x1 - x0 >= 620.0);
        // a toolbar near the left edge: the row moves inward, never off the margin
        let (x0, x1) = row_span(8.0, 676.0, 1280.0, 1000.0);
        assert!(
            x0 >= GAP && x1 - x0 >= 1000.0 && x1 <= 1280.0 - GAP,
            "{x0} {x1}"
        );
        for need in [560.0, 620.0, 860.0, ROW_WIDE] {
            for (at, bar, screen) in [
                (8.0, 676.0, 1280.0),
                (600.0, 676.0, 1280.0),
                (400.0, 900.0, 1600.0),
            ] {
                let (x0, x1) = row_span(at, bar, screen, need);
                assert!(x0 >= GAP && x1 <= screen - GAP, "{x0} {x1}");
                assert!(
                    x1 - x0 + 0.5 >= need.min(screen - 2.0 * GAP),
                    "{need} {at}: {x0} {x1}"
                );
            }
        }
    }

    fn row_texts(e: &Annotator) -> Option<Vec<String>> {
        options_row(e, 4000.0).map(crate::shared::kit::testing::shot::texts)
    }

    /// C4-1: picking a tool shows its options at once in the row under the
    /// toolbar: no second click, no panel opened for them.
    #[test]
    fn c4_1_picking_a_tool_shows_its_row_at_once() {
        for (tool, word) in [
            (Tool::Rect, "Fill"),
            (Tool::Arrow, "Label"),
            (Tool::Pen, "Size"),
            (Tool::Text, "Bold"),
            (Tool::Callout, "Outline"),
            (Tool::Step, "Legend"),
            (Tool::Blur, "Strong"),
            (Tool::Eraser, "Whole drawing"),
        ] {
            let mut e = editor();
            e.update(Message::Tool(tool));
            assert_eq!(e.menu, None, "{tool:?}: no panel");
            let texts = row_texts(&e).unwrap_or_else(|| panic!("{tool:?}: no row"));
            assert!(texts.iter().any(|t| t == word), "{tool:?} {texts:?}");
        }
    }

    /// C4-2: the row is the ACTIVE tool's: it changes with the tool, and a
    /// tool without options (Select) shows none.
    #[test]
    fn c4_2_the_row_follows_the_active_tool() {
        let mut e = editor();
        e.update(Message::Tool(Tool::Select));
        assert!(options_row(&e, 4000.0).is_none());
        e.update(Message::Tool(Tool::Text));
        let text = row_texts(&e).unwrap();
        e.update(Message::Tool(Tool::Pen));
        assert_ne!(row_texts(&e).unwrap(), text);
        // a long capture running: the toolbar alone
        e.capturing = true;
        assert!(options_row(&e, 4000.0).is_none());
    }

    /// C4-3: the row never delays the copy: Copy works with any tool picked,
    /// the row open or not.
    #[test]
    fn c4_3_copy_is_one_press_with_the_row_open() {
        let mut e = editor();
        e.update(Message::Tool(Tool::Blur));
        assert!(options_row(&e, 4000.0).is_some());
        assert!(matches!(e.update(Message::Copy), Some(Output::Copy(_))));
    }

    /// C4-4: the row's controls send the editor's own messages (Studio parts, one component) and a
    /// second click on the group's button opens no panel at all (W107-2).
    #[test]
    fn c4_4_a_second_click_opens_no_panel_and_the_row_has_the_options() {
        let mut e = editor();
        e.update(Message::Tool(Tool::Text));
        e.update(Message::Slot(Tool::Text));
        assert_eq!(e.menu, None);
        assert!(row_texts(&e).unwrap().iter().any(|t| t == "Bold"));
    }

    /// W107-2: the active tool's choices are the FIRST group of its row: Shapes offers rectangle,
    /// ellipse, line and arrow (each one click, each its own message, the tooltip `Name (K)`); Draw and
    /// Hide offer their two, Text its three; Select has no row; no panel opens in between.
    #[test]
    fn w107_2_the_rows_first_group_is_the_tools_choices() {
        let choices = |tool: Tool| -> Vec<Tool> {
            let mut e = editor();
            e.update(Message::Tool(tool));
            let bar = bar_stops(&e).len();
            stops(&e)[bar..]
                .iter()
                .map_while(|m| match m {
                    Message::Tool(t) => Some(*t),
                    _ => None,
                })
                .collect()
        };
        for tool in [Tool::Rect, Tool::Arrow] {
            let all = [Tool::Rect, Tool::Ellipse, Tool::Line, Tool::Arrow];
            assert_eq!(choices(tool), all);
        }
        assert_eq!(
            choices(Tool::Pen),
            [Tool::Pen, Tool::Highlighter, Tool::Eraser]
        );
        assert_eq!(
            choices(Tool::Blur),
            [Tool::Blur, Tool::Redact, Tool::Spotlight, Tool::Magnifier]
        );
        // a tool with no options of its own (Spotlight) still shows the choices: the way to the others
        let mut e = editor();
        e.update(Message::Tool(Tool::Spotlight));
        assert!(row_texts(&e).is_some());
        assert_eq!(choices(Tool::Text), [Tool::Text, Tool::Callout, Tool::Step]);
        // a click on a choice switches at once and the row follows (no panel in between)
        let mut e = editor();
        e.update(Message::Tool(Tool::Rect));
        e.update(Message::Tool(Tool::Ellipse));
        assert_eq!((e.tool, e.menu), (Tool::Ellipse, None));
        // the tooltips name the tool and its key
        assert_eq!(tip(info(Tool::Ellipse)), "Ellipse (O)");
        // Select has nothing to choose
        e.update(Message::Tool(Tool::Select));
        assert!(options_row(&e, 4000.0).is_none());
    }

    /// C4-5: goldens: each tool's row, English and Arabic (right to left).
    #[test]
    fn c4_5_the_options_rows_match_their_golden_images() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for rtl in [false, true] {
                let _ar = rtl.then(crate::shared::kit::testing::arabic);
                let dir = if rtl { "-ar" } else { "" };
                for (name, tool) in [
                    ("rect", Tool::Rect),
                    ("arrow", Tool::Arrow),
                    ("pen", Tool::Pen),
                    ("text", Tool::Text),
                    ("callout", Tool::Callout),
                    ("step", Tool::Step),
                    ("hide", Tool::Blur),
                    ("redact", Tool::Redact),
                    ("eraser", Tool::Eraser),
                ] {
                    let mut e = editor();
                    e.update(Message::Tool(tool));
                    let file = format!("snip-row-{name}{dir}-{}", theme.id);
                    let at = Size::new(1260.0, ROW_H);
                    failed.extend(golden::check("screens", &file, at, &[1.0], || {
                        options_row(&e, 4000.0).unwrap()
                    }));
                }
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// C4-8: the row's controls join the toolbar's Tab ring after the toolbar's own: Tab reaches the first
    /// of them, Enter / Space on it acts (here: a width), and the ring wraps back to the toolbar.
    #[test]
    fn c4_8_the_rows_controls_are_in_the_tab_ring() {
        let mut e = editor();
        e.update(Message::Tool(Tool::Rect));
        let bar = bar_stops(&e).len();
        let all = stops(&e);
        assert!(all.len() > bar + 3, "{} stops, {bar} in the bar", all.len());
        // the row opens with the four shapes, then the first size dot
        assert!(
            matches!(all[bar], Message::Tool(Tool::Rect)),
            "{:?}",
            all[bar]
        );
        assert!(
            matches!(all[bar + 4], Message::Width(_)),
            "{:?}",
            all[bar + 4]
        );
        // a long capture running: the toolbar alone
        e.capturing = true;
        assert_eq!(stops(&e).len(), bar_stops(&e).len());
    }

    /// C4-9: Tab walks into the row, the focus ring shows on its control, and the arrow keys walk the
    /// row while the focus is in it (Right on in English, Left on in Arabic); Enter acts.
    #[test]
    fn c4_9_tab_and_arrows_walk_the_row_and_enter_acts() {
        use crate::features::snip::screens::snip_toolbar::SnipToolbar;
        use iced::keyboard::{Key, Modifiers, key::Named};
        let arrow =
            |e: &Annotator, named| e.key(&Key::Named(named), None, Modifiers::empty(), None);
        for rtl in [false, true] {
            let _ar = rtl.then(crate::shared::kit::testing::arabic);
            let mut t = SnipToolbar::fitted(
                crate::shared::picture::Crop {
                    width: 40,
                    height: 30,
                    rgba: vec![255; 40 * 30 * 4],
                },
                Size::new(1920.0, 1080.0),
                1.0,
            );
            t.annotator.update(Message::Tool(Tool::Rect));
            let bar = bar_stops(&t.annotator).len();
            // arrows in the toolbar are the area's nudges
            assert!(matches!(
                arrow(&t.annotator, Named::ArrowRight),
                Some(Message::Nudge(..))
            ));
            for _ in 0..=bar {
                t.update(Message::Focus(false));
            }
            assert_eq!(t.annotator.focus, Some(bar), "Tab reached the row");
            let on = if rtl {
                Named::ArrowLeft
            } else {
                Named::ArrowRight
            };
            let off = if rtl {
                Named::ArrowRight
            } else {
                Named::ArrowLeft
            };
            assert!(matches!(
                arrow(&t.annotator, on),
                Some(Message::RowStep(..))
            ));
            assert!(matches!(
                arrow(&t.annotator, off),
                Some(Message::RowStep(..))
            ));
            let dx = if rtl { -1 } else { 1 };
            t.update(Message::RowStep(dx, 0));
            assert_eq!(
                t.annotator.focus,
                Some(bar + 1),
                "the reading direction goes on"
            );
            t.update(Message::RowStep(-dx, 0));
            assert_eq!(t.annotator.focus, Some(bar));
            // Enter acts on the focused control: a size
            t.annotator.focus = Some(bar + 4 + 2); // (after the four shapes)
            t.update(Message::Activate);
            assert_eq!(
                t.annotator.width,
                crate::features::snip::annotate::Width::Thick
            );
        }
    }

    /// W107-5: every tool's row, at its natural width, fits a 1280 px screen's margins (1248 px) in
    /// English and Arabic: no scrolling, no spanning the margin.
    #[test]
    fn w107_5_every_row_fits_a_1280_screen() {
        use crate::features::circle::screens::bar::geometry::GAP;
        let room = 1280.0 - 2.0 * GAP;
        let mut wide = Vec::new();
        for rtl in [false, true] {
            let _ar = rtl.then(crate::shared::kit::testing::arabic);
            // EVERY tool (the options too: Label, Pointer, Arrow's label ...): a tool without a row has none
            for tool in crate::features::snip::annotate::TOOLS
                .iter()
                .map(|t| t.tool)
                .filter(|t| *t != Tool::Crop)
            {
                let mut e = editor();
                e.update(Message::Tool(tool));
                let Some(row) = options_row(&e, 4000.0) else {
                    continue;
                };
                let natural =
                    crate::shared::kit::testing::shot::right_edge(row, Size::new(4000.0, ROW_H));
                if natural > row_need(&e) + 0.5 {
                    wide.push(format!(
                        "{tool:?} rtl={rtl}: {natural:.0} over its estimate {}",
                        row_need(&e)
                    ));
                }
                if natural > room {
                    wide.push(format!("{tool:?} rtl={rtl}: {natural:.0} > {room}"));
                }
            }
        }
        assert!(wide.is_empty(), "{wide:#?}");
    }

    /// C4-10: Hide's styles are small live pictures in the row (one place; the group panel has no
    /// cards any more), the look note shows when a look is picked.
    #[test]
    fn c4_10_hides_styles_are_pictures_in_the_row_only() {
        let mut e = editor();
        e.update(Message::Tool(Tool::Blur));
        // (picture-only cells, their names are the tooltips: the row fits a 1280 px screen, W107-5)
        let bar = bar_stops(&e).len();
        let styles = stops(&e)[bar..]
            .iter()
            .filter(|m| matches!(m, Message::BlurStyle(_)))
            .count();
        assert_eq!(styles, crate::features::snip::annotate::blur_styles().len());
        e.update(Message::Slot(Tool::Blur));
        let panel = crate::shared::kit::testing::shot::texts(view(&e));
        assert!(!panel.iter().any(|t| t == "Pixelate"), "{panel:?}");
        e.update(Message::BlurStyle(
            crate::features::snip::annotate::blur_styles()[3].0,
        ));
        assert!(
            row_texts(&e).unwrap().iter().any(|t| t.contains("looks")),
            "the look note"
        );
    }

    /// C4-11 (review): Hide's stops are each control once, in the order they are drawn: the five styles
    /// (not built twice), the three shapes, the strengths, line help, the brush; Text's: size dots,
    /// faces, Bold, backdrops.
    #[test]
    fn c4_11_hides_and_texts_stops_have_no_duplicates_and_follow_the_visual_order() {
        let kinds = |tool: Tool| -> Vec<&'static str> {
            let mut e = editor();
            e.update(Message::Tool(tool));
            let bar = bar_stops(&e).len();
            stops(&e)[bar..]
                .iter()
                .map(|m| match m {
                    Message::Tool(_) => "choice",
                    Message::BlurStyle(_) => "style",
                    Message::BlurShape(_) => "shape",
                    Message::BlurCell(_) => "strength",
                    Message::Assist => "assist",
                    Message::BlurSize(_) => "brush",
                    Message::TextSize(_) => "size",
                    Message::TextStyle(_) => "text",
                    _ => "other",
                })
                .collect()
        };
        let hide = kinds(Tool::Blur);
        let styles = crate::features::snip::annotate::blur_styles().len();
        assert_eq!(
            hide.iter().filter(|k| **k == "style").count(),
            styles,
            "{hide:?}"
        );
        let mut flat = hide.clone();
        flat.dedup();
        assert_eq!(
            flat,
            ["choice", "style", "shape", "strength", "assist", "brush"],
            "{hide:?}"
        );
        let mut text = kinds(Tool::Text);
        text.dedup();
        assert_eq!(text, ["choice", "size", "text"], "{text:?}");
        assert_eq!(kinds(Tool::Text).len(), 3 + 3 + 3 + 1 + 3);
    }

    /// W91.2: the Studio button sits before Copy / Save (Tab reaches it), More has no
    /// Studio row any more, and the one-time hint shows only while asked for.
    #[test]
    fn the_studio_button_is_in_the_bar_and_not_in_more() {
        let e = editor();
        let at = |m: &Message| stops(&e).iter().position(|s| s == m);
        let (studio, copy, save) = (
            at(&Message::OpenStudio),
            at(&Message::Copy),
            at(&Message::Save),
        );
        assert!(studio.is_some() && studio < copy && copy < save);
        assert!(
            more_rows(&e)
                .iter()
                .all(|r| r.message != Some(Message::OpenStudio)),
            "no second entry in More"
        );
        let (_, panel) = build(&e, tooltip::Position::Bottom);
        assert!(panel.is_none(), "no hint unless it is the first Snip");
        let mut first = editor();
        first.studio_hint = true;
        let (_, panel) = build(&first, tooltip::Position::Bottom);
        assert!(panel.is_some(), "the one-time hint");
    }

    /// W91.2: while a long capture runs Open in Studio is dimmed: no Tab stop, no Ctrl+E.
    #[test]
    fn open_in_studio_is_off_while_a_long_capture_runs() {
        use iced::keyboard::{Key, Modifiers};
        let mut e = editor();
        e.movable = true;
        e.capturing = true;
        assert!(!stops(&e).contains(&Message::OpenStudio));
        let key = e.key(
            &Key::Character("e".into()),
            Some('e'),
            Modifiers::CTRL,
            None,
        );
        assert!(!matches!(key, Some(Message::OpenStudio)));
        assert!(
            e.update(Message::OpenStudio).is_none(),
            "even if it arrives"
        );
        e.capturing = false;
        assert!(stops(&e).contains(&Message::OpenStudio));
    }

    /// W91.2: the bar is exactly one button wider than before.
    #[test]
    fn the_bar_grew_by_one_button() {
        let groups = group_widths(false);
        let count: usize = groups.iter().map(Vec::len).sum();
        assert_eq!(count, 2 + Group::ALL.len() + 2 + 1 + 4, "+ the x (C6)");
    }

    /// C6: the toolbar ends with a quiet x (Tab reaches it last): it asks the host to end
    /// Snip, as Esc does.
    #[test]
    fn c6_the_toolbar_ends_with_a_quiet_x() {
        let mut e = editor();
        assert_eq!(bar_stops(&e).last(), Some(&Message::Close));
        assert!(matches!(
            e.update(Message::Close),
            Some(crate::features::snip::annotate::Output::Close)
        ));
    }

    /// W91.2: Ctrl+E opens the Studio from Snip (not from a plain editor).
    #[test]
    fn ctrl_e_opens_the_studio_in_snip_only() {
        use iced::keyboard::{Key, Modifiers};
        let mut e = editor();
        e.movable = true;
        let key = |e: &Annotator| {
            e.key(
                &Key::Character("e".into()),
                Some('e'),
                Modifiers::CTRL,
                None,
            )
        };
        assert!(matches!(key(&e), Some(Message::OpenStudio)));
        e.movable = false;
        assert!(!matches!(key(&e), Some(Message::OpenStudio)));
    }

    /// F15 (#220): a dimmed More row says why in its tooltip: a long capture running, or no
    /// long capture on this desktop.
    #[test]
    fn f15_dimmed_more_rows_say_why() {
        let why_of = |e: &Annotator, name: S| {
            more_rows(e)
                .into_iter()
                .find(|r| r.name == name)
                .map(|r| (r.message.is_some(), r.why))
        };
        let mut e = editor();
        e.scroll = false;
        assert_eq!(
            why_of(&e, S::sn_long_capture),
            Some((false, Some(S::sn_why_no_long)))
        );
        e.scroll = true;
        assert_eq!(why_of(&e, S::sn_long_capture), Some((true, None)));
        e.capturing = true;
        assert_eq!(
            why_of(&e, S::sn_long_capture),
            Some((false, Some(S::sn_why_capturing)))
        );
    }

    /// W117: in Snip (popover floated and translated to the toolbar position), pressing and
    /// dragging known local points on the colour square and hue bar puts the handle right under
    /// the pointer.
    #[test]
    fn w117_snip_colour_picker_puts_handle_under_pointer() {
        use crate::shared::kit::testing::shot;
        let mut e = editor();
        e.update(Message::Picker);
        assert_eq!(e.menu, Some(Menu::Colour));
        let bar = Rectangle::new(Point::new(360.0, 220.0), size_for(&e));
        let win = Size::new(1280.0, 800.0);
        // Colour button: segment 1, right after the tool groups
        let colour_at = (1, crate::features::snip::annotate::Group::ALL.len());
        let (_, left) = centred_on(&e, colour_at, colour::PICKER_W);
        let top = bar.y + bar.height + POPOVER_GAP;
        let at = Point::new(bar.x + left, top);
        let pad = 10.0_f32;
        let sq_w = colour::PICKER_W - 2.0 * pad;
        let sq_h = 150.0_f32;
        let hue_y = at.y + pad + sq_h + space::GAP + 7.0;

        // Square press + drag
        let from = Point::new(at.x + pad + 0.25 * sq_w, at.y + pad + 0.30 * sq_h);
        let to = Point::new(at.x + pad + 0.70 * sq_w, at.y + pad + 0.65 * sq_h);
        let (_, pop) = view_at(
            &e,
            bar,
            tooltip::Position::Bottom,
            tooltip::Position::Bottom,
        );
        let v_sq = iced::widget::stack![
            iced::widget::Space::new()
                .width(win.width)
                .height(win.height),
            pop.unwrap()
        ]
        .into();
        for m in shot::press_move(v_sq, win, from, to) {
            e.update(m);
        }
        let [_, s, v] = e.hsv().expect("hsv set by square drag");
        assert!(
            (s - 0.70).abs() < 0.02 && (v - 0.35).abs() < 0.02,
            "snip square: ({s:.3}, {v:.3})"
        );

        // Hue bar press + drag
        let h_from = Point::new(at.x + pad + 0.20 * sq_w, hue_y);
        let h_to = Point::new(at.x + pad + 0.75 * sq_w, hue_y);
        let (_, pop) = view_at(
            &e,
            bar,
            tooltip::Position::Bottom,
            tooltip::Position::Bottom,
        );
        let v_hue = iced::widget::stack![
            iced::widget::Space::new()
                .width(win.width)
                .height(win.height),
            pop.unwrap()
        ]
        .into();
        for m in shot::press_move(v_hue, win, h_from, h_to) {
            e.update(m);
        }
        let [h, _, _] = e.hsv().expect("hsv set by hue drag");
        assert!((h - 270.0).abs() < 2.0, "snip hue: {h:.1}");
    }

    /// W117: Snip's Hide row has Light / Medium / Strong and NO Custom strength slider (the slider
    /// is Studio-only), and its style swatches are fixed (identical for any captured picture).
    #[test]
    fn w117_snip_magnifier_row_has_outline_stroke_zoom_and_shape_controls() {
        use crate::features::snip::annotate::magnifier::Kind;
        use crate::features::snip::screens::options_row::snip_row_stops;
        let mut e = editor();
        e.update(Message::Tool(Tool::Magnifier));
        let words = row_texts(&e).expect("magnifier has an options row");
        assert!(words.iter().any(|w| w == "Outline"), "{words:?}");
        for z in ["2×", "3×", "4×"] {
            assert!(words.iter().any(|w| w == z), "missing {z}: {words:?}");
        }
        let stops = snip_row_stops(&e);
        assert!(
            stops.iter().any(|m| matches!(m, Message::Width(_))),
            "stroke dots: {stops:?}"
        );
        assert!(
            stops.iter().any(|m| matches!(m, Message::Outline(false))),
            "outline toggle: {stops:?}"
        );
        assert!(
            stops.iter().any(|m| matches!(m, Message::MagnifierZoom(3))),
            "zoom 3x: {stops:?}"
        );
        assert!(
            stops
                .iter()
                .any(|m| matches!(m, Message::MagnifierShape(Kind::RoundedSquare))),
            "rounded square: {stops:?}"
        );
    }

    #[test]
    fn w117_snip_hide_row_has_no_strength_slider_and_fixed_swatches() {
        use crate::shared::kit::testing::shot;
        let mut white = editor();
        white.update(Message::Tool(Tool::Blur));
        let mut dark_pic = Annotator::new(
            Image {
                width: 688,
                height: 330,
                rgba: vec![15; 688 * 330 * 4],
            },
            1.0,
        );
        dark_pic.movable = true;
        dark_pic.update(Message::Tool(Tool::Blur));

        let a = shot::render(options_row(&white, 4000.0).unwrap(), 1260, ROW_H as u32);
        let b = shot::render(options_row(&dark_pic, 4000.0).unwrap(), 1260, ROW_H as u32);
        assert_eq!(a, b, "Hide row swatches must not depend on the screenshot");

        let words = row_texts(&white).unwrap();
        for preset in ["Light", "Medium", "Strong"] {
            assert!(words.iter().any(|w| w == preset), "{preset} in {words:?}");
        }
        let mut saw_custom = false;
        for x in (8..1240).step_by(8) {
            let msgs = shot::click(
                options_row(&white, 4000.0).unwrap(),
                Size::new(1260.0, ROW_H),
                Point::new(x as f32, ROW_H / 2.0),
            );
            if msgs.iter().any(|m| matches!(m, Message::BlurCustom(_))) {
                saw_custom = true;
            }
        }
        assert!(
            !saw_custom,
            "Snip Hide row must not have the Custom strength slider"
        );

        let mut studio_custom = false;
        for x in (4..100).step_by(6) {
            let msgs = shot::click(
                crate::features::snip::screens::options_row::own_groups(&white, Tool::Blur)
                    .into_iter()
                    .nth(3)
                    .unwrap(),
                Size::new(120.0, 32.0),
                Point::new(x as f32, 16.0),
            );
            if msgs.iter().any(|m| matches!(m, Message::BlurCustom(_))) {
                studio_custom = true;
                break;
            }
        }
        assert!(
            studio_custom,
            "Studio Hide bar keeps the Custom strength slider"
        );
    }

    #[test]
    fn style_e_toolbar_and_attached_row_use_two_tone_segments() {
        use crate::shared::kit::testing::shot;
        assert_eq!(
            ROW_GAP, 0.0,
            "options row stays attached directly under toolbar"
        );
        let mut e = editor();
        e.update(Message::Tool(Tool::Rect));
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let bar_sz = size_for(&e);
            let bar_px = shot::render(view(&e), bar_sz.width as u32, bar_sz.height as u32);
            let row_px = shot::render(options_row(&e, 900.0).unwrap(), 900, ROW_H as u32);
            let pad = crate::shared::kit::tokens::widget_e::TRAY_PAD_Y;
            assert!(
                shot::two_tone_column(
                    &bar_px,
                    bar_sz.width as u32,
                    bar_sz.height as u32,
                    theme,
                    pad
                ),
                "{}: snip toolbar: no track ring round a raised segment",
                theme.id
            );
            // the row's groups are raised too: a `track` row, its group's edge, `widget`
            assert!(
                shot::two_tone_column(&row_px, 900, ROW_H as u32, theme, 4.0),
                "{}: options row: no track ring round a raised group",
                theme.id
            );
        }
        themes::set(&SAPPHIRE);
    }
}
