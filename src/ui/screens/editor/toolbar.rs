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
use crate::editor::{
    Editor, Group, Menu, Message, PRESETS, ScreenTool, Tool, Width, dragout, info,
};
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
const COPY: char = '\u{e0a2}'; // lucide "copy"
const SAVE: char = '\u{e0b6}'; // lucide "download"
const PIN: char = '\u{e258}'; // lucide "pin"
const SCROLL: char = '\u{e4d6}'; // lucide "gallery-vertical-end"
const PIPETTE: char = crate::ui::screens::tools::picker::ICON;
const SHARE: char = '\u{e158}'; // lucide "share"
const LIVE_PIN: char = '\u{e3b2}'; // lucide "picture-in-picture-2"
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
];

/// Snip's Live pin (Windows; Labs Pins); it works only over the live screen
/// (`Editor::scroll`). Still pins left Labs (owner, 2026-10-31): Pin is always there.
fn live_pin() -> bool {
    crate::labs::has(crate::labs::Feature::Pins) && crate::capture::live::SUPPORTED
}

/// The bar's items' widths, in order (`build`).
fn widths() -> Vec<f32> {
    let mut w = vec![bar::HANDLE, CHIP_W, bar::DIVIDER];
    w.extend(Group::ALL.map(|_| CONTROL));
    w.extend([
        CONTROL,
        CONTROL,
        bar::DIVIDER,
        CONTROL,
        bar::DIVIDER,
        CONTROL,
        CONTROL,
    ]);
    w
}

/// The toolbar's size (one line, the same in every state).
pub fn size() -> Size {
    let w = widths();
    let line = w.iter().sum::<f32>() + SPACING * (w.len() - 1) as f32;
    Size::new(line + 2.0 * bar::PAD, bar::HEIGHT)
}

/// What Tab walks on the toolbar, in its order (left to right), and what
/// Enter / Space on each sends.
pub fn stops(e: &Editor) -> Vec<Message> {
    let mut stops: Vec<Message> = e.last.iter().map(|&t| Message::Slot(t)).collect();
    stops.extend([
        Message::Picker,
        Message::UndoClick,
        Message::Menu(Menu::More),
        Message::Copy,
        Message::Save,
    ]);
    stops
}

/// A row of More: glyph, name (Latin, Arabic), key, what it sends (`None`:
/// dimmed).
pub type MoreRow = (
    char,
    (&'static str, &'static str),
    Option<&'static str>,
    Option<Message>,
);

/// More's rows (Labs and platform rows only where they work).
pub fn more_rows(e: &Editor) -> Vec<MoreRow> {
    // a long capture running: only what finishes it stays live
    let idle = !e.capturing;
    let live = |m: Message| idle.then_some(m);
    let mut rows = vec![(
        SCROLL,
        ("Long capture", "لقطة طويلة"),
        None,
        (e.scroll && idle).then_some(Message::Scroll),
    )];
    rows.push((
        PIN,
        ("Pin to screen", "تثبيت على الشاشة"),
        Some("Ctrl+P"),
        Some(Message::Pin),
    ));
    if live_pin() {
        let m = (e.scroll && idle).then_some(Message::LivePin);
        rows.push((LIVE_PIN, ("Live pin", "تثبيت مباشر"), None, m));
    }
    if share::SUPPORTED {
        rows.push((SHARE, ("Share", "مشاركة"), None, Some(Message::Share)));
    }
    rows.push((
        PIPETTE,
        ("Colour picker", "منتقي الألوان"),
        None,
        live(Message::ScreenTool(ScreenTool::Picker)),
    ));
    rows.push((
        crate::ui::screens::tools::ruler::ICON,
        ("Screen ruler", "مسطرة الشاشة"),
        None,
        live(Message::ScreenTool(ScreenTool::Ruler)),
    ));
    rows
}

/// The toolbar with its open panel by it (where nothing limits the room).
#[cfg(test)]
pub fn view(e: &Editor) -> Element<'_, Message> {
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
    e: &Editor,
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
    /// Right to left (the goldens' Arabic; the app's UI is Latin today).
    static RTL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Builds the next toolbars right to left (`true`), with Arabic words.
#[cfg(test)]
pub fn set_rtl(on: bool) {
    RTL.set(on);
}

/// `latin`, or `arabic` right to left.
fn ar(latin: &'static str, arabic: &'static str) -> &'static str {
    if RTL.get() { arabic } else { latin }
}

/// A line of items, mirrored right to left.
fn line<'a>(mut items: Vec<Element<'a, Message>>) -> iced::widget::Row<'a, Message> {
    if RTL.get() {
        items.reverse();
    }
    iced::widget::Row::with_children(items)
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
    e: &Editor,
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
    let mut items: Vec<Element<'_, Message>> = vec![hinted(
        grip::mover(Message::Grab),
        ar("Move toolbar", "تحريك شريط الأدوات"),
        Why::Icon,
    )];
    let px = e.doc.view().size();
    let label = format!("{} × {}", px.width.round(), px.height.round());
    let chip = match dragout::SUPPORTED && idle {
        true => hinted(
            chip::size_handle(label, e.area_held(), Message::DragOut),
            ar("Drag the picture into any app", "اسحب الصورة إلى أي تطبيق"),
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
        let (label, why) = match g.name() == shown.name() {
            true => (shown.tip, Why::Icon),
            false => (g.name(), Why::Keys(shown.tip)),
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
        ar("Colour", "اللون"),
        Why::Icon,
    ));
    let undo = Message::UndoClick;
    let s = state(e.undid || e.menu == Some(Menu::Undo), &undo);
    at.push((Menu::Undo, items.len()));
    items.push(hinted(
        icon_button::square(UNDO, s, live(undo)),
        ar("Undo", "تراجع"),
        Why::Keys("Ctrl+Z"),
    ));
    items.push(divide());
    let more = Message::Menu(Menu::More);
    let s = state(e.menu == Some(Menu::More), &more);
    at.push((Menu::More, items.len()));
    items.push(hinted(
        icon_button::square(Icon::More.glyph(), s, Some(more)),
        ar("More", "المزيد"),
        Why::Icon,
    ));
    items.push(divide());
    let copy = button::tool(
        COPY,
        button::Kind::Accent,
        focused(&Message::Copy),
        Some(Message::Copy),
    );
    items.push(hinted(copy, ar("Copy", "نسخ"), Why::Keys("Ctrl+C")));
    let save = button::tool(
        SAVE,
        button::Kind::Elevated,
        focused(&Message::Save),
        Some(Message::Save),
    );
    items.push(hinted(save, ar("Save", "حفظ"), Why::Keys("Ctrl+S")));

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
    let w = widths();
    let i = at.iter().find(|(m, _)| *m == menu).map_or(0, |(_, i)| *i);
    let mut x = bar::PAD + w[..i].iter().map(|w| w + SPACING).sum::<f32>();
    if RTL.get() {
        x = size().width - x - w[i];
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

/// A tool's name in the panel (Arabic right to left).
fn name_of(tool: Tool) -> &'static str {
    let latin = info(tool).name();
    if !RTL.get() {
        return latin;
    }
    match tool {
        Tool::Select => "تحديد",
        Tool::Rect => "مستطيل",
        Tool::Ellipse => "شكل بيضاوي",
        Tool::Line => "خط",
        Tool::Arrow => "سهم",
        Tool::Pen => "قلم",
        Tool::Highlighter => "قلم تمييز",
        Tool::Text => "نص",
        Tool::Callout => "فقاعة",
        Tool::Step => "خطوات مرقمة",
        Tool::Blur => "تمويه",
        Tool::Redact => "حجب",
        Tool::Spotlight => "تسليط الضوء",
        Tool::Magnifier => "تكبير",
        _ => latin,
    }
}

/// A group's panel (a click on its active button): the group's tools, the
/// current one's name and key, then its options. They apply to the next
/// item and to the selected one.
fn options_panel(e: &Editor, group: Group) -> Element<'_, Message> {
    let base = info(info(e.tool).slot);
    let tools = group
        .tools()
        .map(|t| {
            icon(
                t.icon,
                base.tool == t.tool,
                Some(Message::Tool(t.tool)),
                t.tip,
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
fn tool_options(e: &Editor, tool: Tool) -> Vec<Element<'_, Message>> {
    let width_at = Width::ALL.iter().position(|&w| w == e.width);
    let stroke = || {
        let px = e.stroke.round() as u8;
        let chosen = Width::ALL.iter().position(|w| w.stroke() == e.stroke);
        let presets = segmented::presets(chosen, None, false, |i| Message::Width(Width::ALL[i]));
        let slider = slider::bare(crate::config::STROKE, px, SLIDER_W, false, Message::Stroke);
        sizes(
            ar("Size", "الحجم"),
            format!("{px} px"),
            presets,
            Some(slider),
        )
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
        sizes(
            ar("Font size", "حجم الخط"),
            format!("{px} px"),
            presets,
            Some(slider),
        )
    };
    let outline = || {
        switch(
            ar("Outline", "الإطار"),
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
            let shape = crate::editor::fills_inside(tool);
            let mut o = vec![stroke(), outline()];
            // shown dimmed for lines, so the panel doesn't jump
            let note = (!shape).then(|| ar("rectangle, ellipse", "مستطيل، شكل بيضاوي"));
            let fill = shape.then_some(Message::Fill(!e.fill));
            o.push(switch(ar("Fill", "التعبئة"), note, e.fill && shape, fill));
            if tool == Tool::Arrow {
                o.push(variant(ar("Label", "تسمية"), Tool::Arrow, Tool::ArrowLabel));
            }
            o
        }
        Tool::Pen | Tool::Highlighter => vec![stroke()],
        Tool::Text => {
            let style = e.text_style;
            let bold = crate::editor::TextStyle {
                bold: !style.bold,
                ..style
            };
            vec![
                font(),
                switch(
                    ar("Bold", "عريض"),
                    None,
                    style.bold,
                    Some(Message::TextStyle(bold)),
                ),
                variant(ar("Background", "خلفية"), Tool::Text, Tool::Label),
            ]
        }
        Tool::Callout => vec![font(), outline()],
        Tool::Step => {
            let presets =
                segmented::presets(width_at, None, false, |i| Message::Width(Width::ALL[i]));
            vec![
                sizes(ar("Size", "الحجم"), String::new(), presets, None),
                variant(ar("Pointer", "مؤشر"), Tool::Step, Tool::Pointer),
                switch(
                    ar("Legend", "وسيلة الإيضاح"),
                    None,
                    e.doc.legend,
                    Some(Message::Legend),
                ),
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
            let chip = button::switch(ar("Auto", "تلقائي"), auto, Message::BlurSize(to));
            let value = if auto { ar("line", "السطر") } else { "" };
            let cell = Width::ALL.iter().position(|&w| w == e.blur_cell);
            let cells = segmented::presets(cell, None, false, |i| Message::BlurCell(Width::ALL[i]));
            let px = [4, 6, 9][e.blur_cell as usize];
            vec![
                column![
                    option(ar("Brush", "الفرشاة"), None, Some(value.into()), Some(chip)),
                    line(vec![brush]),
                ]
                .spacing(SPACING)
                .into(),
                sizes(
                    ar("Pixel size", "حجم البكسل"),
                    format!("{px} px"),
                    cells,
                    None,
                ),
                switch(
                    ar("Line help", "مساعدة السطر"),
                    Some(ar("stays on the line", "تبقى على السطر")),
                    e.assist,
                    Some(Message::Assist),
                ),
            ]
        }
        Tool::Redact => vec![muted(Role::META, ar("Uses the colour", "يستخدم اللون")).into()],
        _ => Vec::new(),
    }
}

/// Undo's second click: Undo, Redo and where the history is.
fn undo_panel(e: &Editor) -> Element<'_, Message> {
    let (done, all) = e.doc.steps();
    let steps = match RTL.get() {
        true => format!("{done} من {all} خطوات"),
        false => format!("{done} of {all} steps"),
    };
    let items = vec![
        icon(
            UNDO,
            false,
            (done > 0).then_some(Message::Undo),
            ar("Undo", "تراجع"),
        ),
        icon(
            REDO,
            false,
            (done < all).then_some(Message::Redo),
            ar("Redo", "إعادة"),
        ),
        Space::new().width(Fill).into(),
        sub(Role::META, steps).into(),
    ];
    surface::panel(line(items).width(Fill)).into()
}

/// More: long capture, pin, share, the screen tools (arrow keys walk it).
fn more_panel(e: &Editor) -> Element<'_, Message> {
    let rows = more_rows(e)
        .into_iter()
        .enumerate()
        .map(|(i, (glyph, (latin, arabic), key, m))| {
            let label = if RTL.get() { arabic } else { latin };
            option_list::action(glyph, label, key, e.row == Some(i), m)
        })
        .collect();
    option_list::menu(rows)
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
fn colour_panel(e: &Editor) -> Element<'_, Message> {
    let eyedropper = hinted(
        icon_button::square(PIPETTE, State::default(), Some(Message::Eyedropper)),
        ar("Pick a colour from the image", "اختر لونًا من الصورة"),
        Why::Keys("D"),
    );
    let style = line(vec![
        eyedropper,
        chip::action(ar("Copy style", "نسخ النمط").into(), Message::CopyStyle),
        chip::action(ar("Paste style", "لصق النمط").into(), Message::PasteStyle),
    ]);
    let preset = |i: usize| -> Element<'_, Message> {
        let p = &PRESETS[i];
        hinted(
            chip::action(p.short.into(), Message::Preset(i)),
            p.name,
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
        caption(Role::SECTION, ar("Presets", "الإعدادات المسبقة")),
        column(pairs).spacing(SPACING)
    ]
    .spacing(SPACING);
    colour::picker_with(
        &crate::editor::style::ANNOTATION,
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
    use crate::editor::Image;
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    fn editor() -> Editor {
        let image = Image {
            width: 688,
            height: 330,
            rgba: vec![255; 688 * 330 * 4],
        };
        let mut e = Editor::new(image, 1.0);
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
                set_rtl(rtl);
                let mut colour = editor();
                colour.update(Message::Picker);
                let mut undo = editor();
                undo.update(Message::UndoClick);
                undo.update(Message::UndoClick);
                let mut more = editor();
                more.update(Message::Menu(Menu::More));
                let mut held = editor();
                held.held = true;
                let cases: Vec<(&str, Editor, f32)> = vec![
                    ("bar", editor(), 60.0),
                    ("held", held, 60.0),
                    ("shapes", panel(Tool::Arrow), 330.0),
                    ("rect", panel(Tool::Rect), 330.0),
                    ("draw", panel(Tool::Pen), 230.0),
                    ("text", panel(Tool::Text), 330.0),
                    ("steps", panel(Tool::Step), 330.0),
                    ("hide", panel(Tool::Blur), 360.0),
                    ("colour", colour, 440.0),
                    ("undo", undo, 130.0),
                    ("more", more, 300.0),
                ];
                let dir = if rtl { "-ar" } else { "" };
                for (name, e, height) in cases {
                    let file = format!("snip-toolbar-{name}{dir}-{}", theme.id);
                    let at = Size::new(size().width, height);
                    failed.extend(golden::check("screens", &file, at, &[1.0], || view(&e)));
                }
            }
        }
        set_rtl(false);
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
