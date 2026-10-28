//! A dropdown: the design's language selector (`.oc-input`, as wide as a
//! settings row's control column; the value and "▾", the edge `accent` under
//! the cursor and while open) and its menu (`.oc-dropdown-menu`: the
//! `dropdown` material, a `rim` edge, radius 14), built from kit parts: one
//! `option_list::option` row per choice in the one `scroll` area (its
//! scrollbar is every other one's). It opens below (above when there is no
//! room); a pick, a click outside or Esc closes it.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard::{self, key::Named};
use iced::widget::{column, container, row, text};
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
    let role = Role::NOTE;
    let shown = chosen.as_ref().map(ToString::to_string);
    let value = text(shown.clone().unwrap_or_else(|| placeholder.to_string()))
        .size(role.size)
        .font(font(role))
        .line_height(LINE)
        .color(match shown {
            Some(_) => c().text,
            None => c().text_muted,
        });
    let handle = text("▾")
        .size(role.size)
        .font(MONO)
        .line_height(LINE)
        .color(c().text);
    let opener = container(row![container(value).width(Fill), handle].align_y(Center))
        .padding(PAD)
        .width(size::CONTROL_COLUMN)
        .style(|_| container::Style {
            background: Some(c().track.into()),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        });
    let inner = size::CONTROL_COLUMN - 2.0 * MENU_PAD - super::scroll::WIDTH - 2.0;
    let rows = options.iter().map(|o| {
        let active = chosen.as_ref() == Some(o);
        option(
            o.to_string(),
            None,
            active,
            (inner, ROW_H),
            on_pick(o.clone()),
        )
    });
    let menu = container(scroll(column(rows)).height(Length::Shrink))
        .padding(MENU_PAD)
        .width(size::CONTROL_COLUMN)
        .max_height(MENU_MAX)
        .style(|_| container::Style {
            background: Some(c().dropdown.into()),
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        });
    Element::new(Dropdown {
        opener: opener.into(),
        menu: menu.into(),
    })
}

struct Dropdown<'a, Message> {
    opener: Element<'a, Message>,
    menu: Element<'a, Message>,
}

#[derive(Default)]
struct State {
    open: bool,
}

impl<'a, Message: 'a> Widget<Message, iced::Theme, iced::Renderer> for Dropdown<'a, Message> {
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
}

impl<Message> overlay::Overlay<Message, iced::Theme, iced::Renderer> for Menu<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, Size::new(self.opener.width, MENU_MAX));
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
        // a row picked: closed
        let picked = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && inside
            && shell.is_event_captured();
        if picked {
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
