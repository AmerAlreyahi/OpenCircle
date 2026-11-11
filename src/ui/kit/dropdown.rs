//! A dropdown: the design's language selector (`.oc-input`, as wide as a
//! settings row's control column; the value and "▾", the edge `accent` under
//! the cursor and while open) and its menu (`.oc-dropdown-menu`: the
//! `dropdown` material, a `rim` edge, radius 14), built from kit parts: one
//! `option_list::option` row per choice in the one `scroll` area (its
//! scrollbar is every other one's). It opens below (above when there is no
//! room); a pick, a click outside or Esc closes it. `combo`: the same with a
//! search field at the top of the menu (typing filters, Enter picks) and an
//! action at its foot that leaves it open (the Model row). `open` opens a
//! combo from code (its search field's id): an error card's "Choose another model".
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard::{self, key::Named};
use iced::widget::{Space, column, container, text};
use iced::{Border, Center, Element, Event, Fill, Length, Point, Rectangle, Size, Vector};

use super::option_list::option;
use super::scroll::scroll;
use super::text::{MONO, font};
use super::themes::c;
use super::tokens::{Role, radius, size};

/// The opener's line and room: 12 px text on a 1.3 line, 7x11 px round it
/// (32 px with the edge, as `.oc-input`).
const LINE: f32 = 1.3;
const PAD: [f32; 2] = [7.0, 11.0];
/// The menu: 4 px in, rows 30 px, at most this tall (then it scrolls), and
/// its gap under the opener.
const MENU_PAD: f32 = 4.0;
const ROW_H: f32 = 30.0;
const MENU_MAX: f32 = 280.0;
const MENU_GAP: f32 = 4.0;

pub fn dropdown<'a, Message: Clone + 'a, T: ToString + PartialEq + Clone + 'a>(
    options: &'a [T],
    chosen: Option<T>,
    placeholder: &str,
    on_pick: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let shown = chosen.as_ref().map(ToString::to_string);
    let rows = options.iter().map(|o| {
        let active = chosen.as_ref() == Some(o);
        option(
            o.to_string(),
            None,
            active,
            (INNER, ROW_H),
            on_pick(o.clone()),
        )
    });
    let menu = super::option_list::frame(scroll(column(rows)).height(Length::Shrink))
        .padding(MENU_PAD)
        .width(size::CONTROL_COLUMN)
        .max_height(MENU_MAX);
    Element::new(Dropdown {
        opener: opener(shown, placeholder, false),
        menu: menu.into(),
        on_open: None,
        foot: false,
        id: None,
    })
}

/// The rows' width (the scroller beside them, 2 px apart).
const INNER: f32 = size::CONTROL_COLUMN - 2.0 * MENU_PAD - super::scroll::WIDTH - 2.0;

/// A `combo` row: its label, its tag (a quiet `badge`), whether it is the
/// current choice, and what a press sends.
pub type Row<'a, Message> = (String, Option<String>, bool, Message);

/// A `combo`'s search field: its widget id (the caller focuses it on
/// `on_open`), hint, text, and what typing and Enter send.
pub struct Search<'a, Message> {
    pub id: &'static str,
    pub hint: &'a str,
    pub query: &'a str,
    pub on_query: Box<dyn Fn(String) -> Message + 'a>,
    pub on_submit: Message,
}

/// A dropdown with a search: the opener shows `value` (mono: a value that is
/// code) or `placeholder`; the menu holds the search field, an optional
/// muted `note` line, the `rows` (filtered by the caller) and an optional
/// `foot` action (no message = a muted status line) whose press keeps the
/// menu open. `on_open` is sent when the menu opens.
pub fn combo<'a, Message: Clone + 'a>(
    value: Option<&str>,
    placeholder: &str,
    search: Search<'a, Message>,
    note: Option<String>,
    rows: Vec<Row<'a, Message>>,
    foot: Option<(String, Option<Message>)>,
    on_open: Message,
) -> Element<'a, Message> {
    let field = super::field::search(search.hint, search.query)
        .id(search.id)
        .on_input(search.on_query)
        .on_submit(search.on_submit)
        .width(Fill);
    let line = |s: String, h: f32| {
        container(super::text::muted(Role::NOTE, s))
            .padding([0.0, PAD[1]])
            .center_y(h)
            .width(Fill)
            .align_x(super::dir::start())
    };
    let room = COMBO_MAX
        - SEARCH_H
        - MENU_GAP
        - NOTE_H * f32::from(note.is_some())
        - ROW_H
        - FOOT_RULE
        - 1.0;
    let rows_h = (rows.len() as f32 * ROW_H).min(room);
    let rows = (rows.into_iter()).map(|(label, badge, active, message)| {
        option(label, badge, active, (INNER, ROW_H), message)
    });
    let mut menu = column![field]
        .push(Space::new().height(MENU_GAP))
        .push(note.map(|n| line(n, NOTE_H)))
        .push(scroll(column(rows)).height(rows_h));
    if let Some((label, message)) = foot {
        let rule = container(super::divider::divider()).padding([FOOT_RULE / 2.0, PAD[1]]);
        menu = menu.push(rule).push(match message {
            Some(m) => option(label, None, false, (INNER, ROW_H), m),
            None => line(label, ROW_H).into(),
        });
    }
    let menu = super::option_list::frame(menu)
        .padding(MENU_PAD)
        .width(size::CONTROL_COLUMN);
    Element::new(Dropdown {
        opener: opener(value.map(str::to_string), placeholder, true),
        menu: menu.into(),
        on_open: Some(on_open),
        foot: true,
        id: Some(widget::Id::new(search.id)),
    })
}

/// A `combo`'s tallest menu, its search field and its note line.
const COMBO_MAX: f32 = 360.0;
const SEARCH_H: f32 = 32.0;
const NOTE_H: f32 = 24.0;
/// The room of the hairline over the foot (1 px in it).
const FOOT_RULE: f32 = 8.0;

/// The opener: the value (`text`; mono for code) or the placeholder
/// (`text_muted`), and "▾".
fn opener<'a, Message: 'a>(
    shown: Option<String>,
    placeholder: &str,
    mono: bool,
) -> Element<'a, Message> {
    let role = Role::NOTE;
    let face = match mono {
        true => super::text::mono(iced::font::Weight::Normal),
        false => font(role),
    };
    let full = shown.clone().unwrap_or_else(|| placeholder.to_string());
    // one line that stops before the arrow: cut in the middle, whole in a tooltip
    let room = size::CONTROL_COLUMN
        - 2.0 * PAD[1]
        - 2.0
        - super::text::width("▾", super::text::size(role), MONO)
        - 4.0;
    let line = super::text::cut_middle(&full, super::text::size(role), face, room);
    let cut = line != full;
    let value = text(line)
        .size(super::text::size(role))
        .font(face)
        .line_height(LINE)
        .wrapping(text::Wrapping::None)
        .color(match shown {
            Some(_) => c().text,
            None => c().text_muted,
        });
    let handle = text("▾")
        .size(super::text::size(role))
        .font(MONO)
        .line_height(LINE)
        .color(c().text);
    let value: Element<'a, Message> = match cut {
        true => super::tooltip::cut(value, full),
        false => value.into(),
    };
    // right to left: the value at the right, ▾ at the left
    let value = container(value)
        .width(Fill)
        .align_x(super::dir::start())
        .clip(true);
    container(crate::dir_row![value, handle].align_y(Center))
        .padding(PAD)
        .width(size::CONTROL_COLUMN)
        .style(|_| container::Style {
            background: Some(c().track.into()),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

struct Dropdown<'a, Message> {
    opener: Element<'a, Message>,
    menu: Element<'a, Message>,
    /// Sent as the menu opens (a combo focuses its search field).
    on_open: Option<Message>,
    /// The menu's last part is an action that leaves it open.
    foot: bool,
    /// A combo's id (its search field's): `open` finds it by it.
    id: Option<widget::Id>,
}

/// Opens the combo whose search field is `id` (its `on_open` is not sent:
/// the caller focuses the search itself).
pub fn open<T: Send + 'static>(id: &'static str) -> iced::Task<T> {
    iced::advanced::widget::operate(open_op(id))
}

/// `open`'s operation.
pub fn open_op<T>(id: &'static str) -> impl widget::Operation<T> {
    struct Open(widget::Id);
    impl<T> widget::Operation<T> for Open {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation<T>)) {
            operate(self);
        }
        fn custom(&mut self, id: Option<&widget::Id>, _: Rectangle, state: &mut dyn std::any::Any) {
            if let (true, Some(state)) = (id == Some(&self.0), state.downcast_mut::<State>()) {
                state.open = true;
            }
        }
    }
    Open(widget::Id::new(id))
}

#[derive(Default)]
struct State {
    open: bool,
}

impl<'a, Message: Clone + 'a> Widget<Message, iced::Theme, iced::Renderer>
    for Dropdown<'a, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.opener), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.opener, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.opener.as_widget().size()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.custom(self.id.as_ref(), layout.bounds(), state);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        (self.opener.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        (self.opener.as_widget()).draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        let open = tree.state.downcast_ref::<State>().open;
        if open || cursor.is_over(layout.bounds()) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: Border::default()
                        .rounded(radius::TRACK)
                        .width(1)
                        .color(c().accent),
                    ..renderer::Quad::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                state.open = !state.open;
                if let (true, Some(m)) = (state.open, &self.on_open) {
                    shell.publish(m.clone());
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => shell.request_redraw(),
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        match cursor.is_over(layout.bounds()) {
            true => mouse::Interaction::Pointer,
            false => mouse::Interaction::None,
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<State>();
        if !state.open {
            return None;
        }
        let opener = layout.bounds() + translation;
        Some(overlay::Element::new(Box::new(Menu {
            open: &mut state.open,
            tree: &mut children[1],
            menu: &mut self.menu,
            opener,
            foot: self.foot,
        })))
    }
}

/// The open menu, over everything.
struct Menu<'a, 'b, Message> {
    open: &'b mut bool,
    tree: &'b mut Tree,
    menu: &'b mut Element<'a, Message>,
    /// The opener's place in the window.
    opener: Rectangle,
    foot: bool,
}

impl<Message> overlay::Overlay<Message, iced::Theme, iced::Renderer> for Menu<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        // (a combo sizes its own rows: its whole menu, edge and padding in)
        let max = match self.foot {
            true => COMBO_MAX + 2.0 * MENU_PAD + 2.0,
            false => MENU_MAX,
        };
        let limits = layout::Limits::new(Size::ZERO, Size::new(self.opener.width, max));
        let node = (self.menu.as_widget_mut()).layout(self.tree, renderer, &limits);
        let h = node.size().height;
        let below = self.opener.y + self.opener.height + MENU_GAP;
        // below the opener; above it when the window has no room below
        let y = match below + h > bounds.height && self.opener.y - MENU_GAP - h >= 0.0 {
            true => self.opener.y - MENU_GAP - h,
            false => below,
        };
        node.move_to(Point::new(self.opener.x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        (self.menu.as_widget()).draw(self.tree, renderer, theme, style, layout, cursor, &bounds);
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        (self.menu.as_widget_mut()).operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let inside = cursor.is_over(layout.bounds());
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Escape),
                ..
            }) => {
                *self.open = false;
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            // outside: closes (on the opener: only closes, it doesn't reopen)
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !inside => {
                *self.open = false;
                shell.request_redraw();
                if cursor.is_over(self.opener) {
                    shell.capture_event();
                }
                return;
            }
            _ => {}
        }
        let bounds = layout.bounds();
        (self.menu.as_widget_mut()).update(
            self.tree, event, layout, cursor, renderer, clipboard, shell, &bounds,
        );
        // a row picked (not the foot's action) or Enter in the search: closed
        let on_foot = self.foot
            && (layout.children().next())
                .and_then(|parts| parts.children().last())
                .is_some_and(|f| cursor.is_over(f.bounds()));
        let released = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && inside
            && !on_foot;
        let entered = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Enter),
                ..
            })
        );
        if (released || entered) && shell.is_event_captured() {
            *self.open = false;
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        (self.menu.as_widget()).mouse_interaction(self.tree, layout, cursor, &bounds, renderer)
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Size};

    use super::super::testing::shot;

    const LANGS: [&str; 6] = ["Arabic", "English", "Japanese", "العربية", "Hindi", "Wolof"];

    /// Changing a dropdown's value leaves no part of the old text on screen
    /// (#53): each change redrawn only where it differs, as the window does,
    /// equals the same view drawn whole, at 100 % and 150 %.
    #[test]
    fn a_changed_dropdown_leaves_no_ghost_text() {
        for scale in [1.0, 1.5] {
            let view = |i: usize| {
                (i <= 20).then(|| {
                    let field =
                        super::dropdown(&LANGS[..], Some(LANGS[i % LANGS.len()]), "", |_| ());
                    iced::widget::container(field).padding(20).into()
                })
            };
            let size = iced::Size::new(360.0, 80.0);
            let (window, fresh) = shot::redrawn(view, size, scale);
            let differ = window.iter().zip(&fresh).filter(|(a, b)| a != b).count();
            assert_eq!(differ, 0, "{differ} stale bytes at {scale}x");
        }
    }

    /// The open menu as drawn, longer than it can show: the kit's rows, the
    /// kit's scrollbar (the same as every scroll area), in both themes.
    #[test]
    fn the_open_menu_matches_its_golden_image() {
        use super::super::testing::golden;
        use super::super::themes::{self, DAYLIGHT, SAPPHIRE};
        const MANY: [&str; 14] = [
            "Arabic", "Chinese", "Dutch", "English", "French", "German", "Hindi", "Italian",
            "Japanese", "Korean", "Polish", "Russian", "Spanish", "Turkish",
        ];
        if !golden::enabled() {
            return;
        }
        let size = Size::new(340.0, 360.0);
        let at = Point::new(60.0, 36.0);
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let view = || {
                iced::widget::container(super::dropdown(&MANY[..], Some("English"), "", |_| ()))
                    .padding(20)
                    .width(iced::Fill)
                    .height(iced::Fill)
                    .into()
            };
            let px = shot::after_clicks(view, size, &[at]);
            let name = format!("dropdown_open-{}@1x", theme.id);
            if let Err(e) = golden::compare("kit", &name, 340, 360, &px) {
                failed.push(e);
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// A combo as the Model row shows it: a value, a note, rows with the
    /// provider's price tags, the typed id, the foot's action.
    fn combo_view(more: usize) -> iced::Element<'static, &'static str> {
        let mut rows = vec![
            ("gemini-flash-lite-latest".into(), None, true, "a"),
            (
                "gemini-3.8-flash".into(),
                Some("Free tier at Google".to_string()),
                false,
                "b",
            ),
            (
                "gemini-4-argon".into(),
                Some("Paid at Google".to_string()),
                false,
                "c",
            ),
        ];
        rows.extend((0..more).map(|i| (format!("model-{i}"), None, false, "m")));
        let search = super::Search {
            id: "combo-search",
            hint: "Search or type a model id",
            query: "",
            on_query: Box::new(|_| "typed"),
            on_submit: "first",
        };
        let field = super::combo(
            Some("gemini-flash-lite-latest"),
            "",
            search,
            Some("Pricing is set by Google".to_string()),
            rows,
            Some(("Load models from Google Gemini".to_string(), Some("load"))),
            "open",
        );
        iced::widget::container(field)
            .padding(20)
            .width(iced::Fill)
            .height(iced::Fill)
            .into()
    }

    /// The Model row with long OpenRouter ids and tags: the opener's value and
    /// every row on one line, cut in the middle, tags clear of the ids.
    #[test]
    fn the_open_combo_with_long_ids_matches_its_golden_image() {
        use super::super::testing::golden;
        use super::super::themes::{self, DAYLIGHT, SAPPHIRE};
        if !golden::enabled() {
            return;
        }
        let long = "nvidia/nemotron-3-ultra-550b-a55b-instruct-extra-long-id:free";
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let view = || {
                let rows = vec![
                    (
                        "openrouter/free".into(),
                        Some("Free at OpenRouter".to_string()),
                        false,
                        "a",
                    ),
                    (
                        long.into(),
                        Some("Free at OpenRouter".to_string()),
                        true,
                        "b",
                    ),
                    (
                        "anthropic/claude-sonnet-5.5".into(),
                        Some("Paid at OpenRouter".to_string()),
                        false,
                        "c",
                    ),
                    (
                        "a-provider/an-extremely-long-model-name-for-a-vision-chat-model".into(),
                        None,
                        false,
                        "d",
                    ),
                ];
                let search = super::Search {
                    id: "combo-search",
                    hint: "Search or type a model id",
                    query: "",
                    on_query: Box::new(|_| "typed"),
                    on_submit: "first",
                };
                let field = super::combo(Some(long), "", search, None, rows, None, "open");
                iced::widget::container(field)
                    .padding(20)
                    .width(iced::Fill)
                    .height(iced::Fill)
                    .into()
            };
            let px = shot::after_clicks(view, Size::new(400.0, 260.0), &[Point::new(60.0, 36.0)]);
            let name = format!("combo_long_open-{}@1x", theme.id);
            if let Err(e) = golden::compare("kit", &name, 400, 260, &px) {
                failed.push(e);
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// The open combo in both themes.
    #[test]
    fn the_open_combo_matches_its_golden_image() {
        use super::super::testing::golden;
        use super::super::themes::{self, DAYLIGHT, SAPPHIRE};
        if !golden::enabled() {
            return;
        }
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let px = shot::after_clicks(
                || combo_view(0),
                Size::new(400.0, 300.0),
                &[Point::new(60.0, 36.0)],
            );
            let name = format!("combo_open-{}@1x", theme.id);
            if let Err(e) = golden::compare("kit", &name, 400, 300, &px) {
                failed.push(e);
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// Opening says so (the caller focuses the search); the foot's action
    /// leaves the menu open, so a row can still be picked after it.
    #[test]
    fn the_combo_foot_keeps_it_open() {
        let size = Size::new(400.0, 300.0);
        // opener 20..52; menu from 56: 4 in, search 32, gap 4, note 24, rows 30 each
        let (opener, row_b, foot) = (
            Point::new(60.0, 36.0),
            Point::new(60.0, 120.0 + 45.0),
            Point::new(60.0, 120.0 + 90.0 + 9.0 + 15.0),
        );
        let got = shot::clicks(|| combo_view(0), size, &[opener, foot, row_b]);
        assert_eq!(got, ["open", "load", "b"]);
        // a row closes it: the next click there picks nothing
        let got = shot::clicks(|| combo_view(0), size, &[opener, row_b, row_b]);
        assert_eq!(got, ["open", "b"]);
        // a long list scrolls inside; the foot stays at the menu's end (360 in)
        let size = Size::new(400.0, 480.0);
        let foot = Point::new(60.0, 56.0 + 4.0 + 360.0 - 15.0);
        let got = shot::clicks(|| combo_view(60), size, &[opener, foot]);
        assert_eq!(got, ["open", "load"]);
    }

    /// `open` opens a combo by its search's id (an error card's "Choose
    /// another model"): a row can be picked with no click on the opener;
    /// another id opens nothing.
    #[test]
    fn open_opens_the_combo_by_its_id() {
        let size = Size::new(400.0, 300.0);
        let row_b = Point::new(60.0, 120.0 + 45.0);
        let mut op = super::open_op::<()>("combo-search");
        let got = shot::clicks_after(|| combo_view(0), size, Some(&mut op), &[row_b]);
        assert_eq!(got, ["b"]);
        let mut op = super::open_op::<()>("another");
        let got = shot::clicks_after(|| combo_view(0), size, Some(&mut op), &[row_b]);
        assert!(got.is_empty(), "{got:?}");
    }

    /// A click opens the menu (the kit's rows, every choice), a row's click
    /// picks it and closes the menu.
    #[test]
    fn a_click_opens_the_menu_and_a_row_picks() {
        let size = Size::new(400.0, 400.0);
        let view = || {
            iced::widget::container(super::dropdown(&LANGS[..], Some("English"), "", |l| l))
                .padding(20)
                .into()
        };
        let opener = Point::new(60.0, 36.0);
        // closed: only the value
        let texts = shot::texts(view());
        assert!(!texts.iter().any(|t| t == "Wolof"), "{texts:?}");
        let picked = shot::clicks(
            view,
            size,
            &[opener, Point::new(60.0, 60.0 + 30.0 * 2.0 + 15.0)],
        );
        assert_eq!(picked, ["Japanese"]);
    }
}
