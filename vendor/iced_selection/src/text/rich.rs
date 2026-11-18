use iced_widget::graphics::text::Paragraph;
use iced_widget::graphics::text::cosmic_text;

use crate::click;
use crate::core::alignment;
use crate::core::clipboard;
use crate::core::keyboard;
use crate::core::keyboard::key;
use crate::core::layout;
use crate::core::mouse;
use crate::core::renderer;
use crate::core::text::{Paragraph as _, Span};
use crate::core::time::Duration;
use crate::core::touch;
use crate::core::widget::text::{Alignment, LineHeight, Shaping, Wrapping};
use crate::core::widget::tree::{self, Tree};
use crate::core::{
    self, Clipboard, Element, Event, Font, Layout, Length, Pixels, Point,
    Rectangle, Shell, Size, Vector, Widget,
};
use crate::selection::{Selection, SelectionEnd};
use crate::text::{Catalog, Dragging, Style, StyleFn};

/// A bunch of [`Rich`] text.
pub struct Rich<
    'a,
    Link,
    Message,
    Theme = iced_widget::Theme,
    Renderer = iced_widget::Renderer,
> where
    Link: Clone + 'static,
    Theme: Catalog,
    Renderer: core::text::Renderer,
{
    spans: Box<dyn AsRef<[Span<'a, Link, Renderer::Font>]> + 'a>,
    size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    height: Length,
    font: Option<Renderer::Font>,
    align_x: Alignment,
    align_y: alignment::Vertical,
    wrapping: Wrapping,
    click_interval: Option<Duration>,
    class: Theme::Class<'a>,
    on_link_click: Option<Box<dyn Fn(Link) -> Message + 'a>>,
    on_link_hover: Option<Box<dyn Fn(Link) -> Message + 'a>>,
    on_hover_lost: Option<Box<dyn Fn() -> Message + 'a>>,
}

impl<'a, Link, Message, Theme, Renderer>
    Rich<'a, Link, Message, Theme, Renderer>
where
    Link: Clone + 'static,
    Theme: Catalog,
    Renderer: core::text::Renderer,
    Renderer::Font: 'a,
{
    /// Creates a new empty [`Rich`] text.
    pub fn new() -> Self {
        Self {
            spans: Box::new([]),
            size: None,
            line_height: LineHeight::default(),
            width: Length::Shrink,
            height: Length::Shrink,
            font: None,
            align_x: Alignment::Default,
            align_y: alignment::Vertical::Top,
            wrapping: Wrapping::default(),
            click_interval: None,
            class: Theme::default(),
            on_link_click: None,
            on_link_hover: None,
            on_hover_lost: None,
        }
    }

    /// Creates a new [`Rich`] text with the given text spans.
    pub fn with_spans(
        spans: impl AsRef<[Span<'a, Link, Renderer::Font>]> + 'a,
    ) -> Self {
        Self {
            spans: Box::new(spans),
            ..Self::new()
        }
    }

    /// Sets the default size of the [`Rich`] text.
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    /// Sets the default [`LineHeight`] of the [`Rich`] text.
    pub fn line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    /// Sets the default font of the [`Rich`] text.
    pub fn font(mut self, font: impl Into<Renderer::Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Sets the width of the [`Rich`] text boundaries.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height of the [`Rich`] text boundaries.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Centers the [`Rich`] text, both horizontally and vertically.
    pub fn center(self) -> Self {
        self.align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
    }

    /// Sets the [`alignment::Horizontal`] of the [`Rich`] text.
    pub fn align_x(mut self, alignment: impl Into<Alignment>) -> Self {
        self.align_x = alignment.into();
        self
    }

    /// Sets the [`alignment::Vertical`] of the [`Rich`] text.
    pub fn align_y(
        mut self,
        alignment: impl Into<alignment::Vertical>,
    ) -> Self {
        self.align_y = alignment.into();
        self
    }

    /// Sets the [`Wrapping`] strategy of the [`Rich`] text.
    pub fn wrapping(mut self, wrapping: Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    /// The maximum delay required for two consecutive clicks to be interpreted as a double click
    /// (also applies to triple clicks).
    ///
    /// Defaults to 300ms.
    pub fn click_interval(mut self, click_interval: Duration) -> Self {
        self.click_interval = Some(click_interval);
        self
    }

    /// Sets the message that will be produced when a link of the [`Rich`] text
    /// is clicked.
    ///
    /// If the spans of the [`Rich`] text contain no links, you may need to call
    /// this method with `on_link_click(never)` in order for the compiler to infer
    /// the proper `Link` generic type.
    pub fn on_link_click(
        mut self,
        on_link_click: impl Fn(Link) -> Message + 'a,
    ) -> Self {
        self.on_link_click = Some(Box::new(on_link_click));
        self
    }

    /// Sets the message that will be produced when a link of the [`Rich`] text
    /// is hovered.
    pub fn on_link_hover(
        mut self,
        on_link_hovered: impl Fn(Link) -> Message + 'a,
    ) -> Self {
        self.on_link_hover = Some(Box::new(on_link_hovered));
        self
    }

    /// Sets the message that will be produced when a link of the [`Rich`] text
    /// is no longer hovered.
    pub fn on_hover_lost(mut self, on_hover_lost: Message) -> Self
    where
        Message: Clone + 'a,
    {
        self.on_hover_lost = Some(Box::new(move || on_hover_lost.clone()));
        self
    }

    /// Sets the message that will be produced when a link of the [`Rich`] text
    /// is no longer hovered.
    pub fn on_hover_lost_with(
        mut self,
        on_hover_lost: impl Fn() -> Message + 'a,
    ) -> Self {
        self.on_hover_lost = Some(Box::new(on_hover_lost));
        self
    }

    /// Sets the default style of the [`Rich`] text.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the default style class of the [`Rich`] text.
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }
}

impl<'a, Link, Message, Theme, Renderer> Default
    for Rich<'a, Link, Message, Theme, Renderer>
where
    Link: Clone + 'a,
    Theme: Catalog,
    Renderer: core::text::Renderer<Paragraph = Paragraph, Font = Font>,
    Renderer::Font: 'a,
{
    fn default() -> Self {
        Self::new()
    }
}

struct State<Link> {
    spans: Vec<Span<'static, Link, Font>>,
    span_pressed: Option<usize>,
    hovered_link: Option<usize>,
    paragraph: Paragraph,
    is_hovered: bool,
    selection: Selection,
    dragging: Option<Dragging>,
    last_click: Option<click::Click>,
    keyboard_modifiers: keyboard::Modifiers,
    visual_lines_bounds: Vec<core::Rectangle>,
}

impl<Link> State<Link> {
    fn grapheme_line_and_index(
        &self,
        point: Point,
        bounds: core::Rectangle,
    ) -> Option<(usize, usize)> {
        let bounded_x = if point.y < bounds.y {
            bounds.x
        } else if point.y > bounds.y + bounds.height {
            bounds.x + bounds.width
        } else {
            point.x.max(bounds.x).min(bounds.x + bounds.width)
        };
        let bounded_y = point.y.max(bounds.y).min(bounds.y + bounds.height);
        let bounded_point = Point::new(bounded_x, bounded_y);
        let mut relative_point =
            bounded_point - core::Vector::new(bounds.x, bounds.y);

        let buffer = self.paragraph.buffer();
        let line_height = buffer.metrics().line_height;
        let visual_line = (relative_point.y / line_height).floor() as usize;
        let visual_line_start_offset = self
            .visual_lines_bounds
            .get(visual_line)
            .map(|r| r.x)
            .unwrap_or_default();
        let visual_line_end = self
            .visual_lines_bounds
            .get(visual_line)
            .map(|r| r.x + r.width)
            .unwrap_or_default();

        if relative_point.x < visual_line_start_offset {
            relative_point.x = visual_line_start_offset;
        }

        if relative_point.x > visual_line_end {
            relative_point.x = visual_line_end;
        }

        let cursor = buffer.hit(relative_point.x, relative_point.y)?;
        let value = buffer.lines[cursor.line].text();

        Some((
            cursor.line,
            unicode_segmentation::UnicodeSegmentation::graphemes(
                &value[..cursor.index.min(value.len())],
                true,
            )
            .count(),
        ))
    }

    /// OpenCircle patch (`group`): the selection's end for a drag end at
    /// `point`: above the glyphs = the text's start, below = its end (its
    /// logical ends, whatever its direction), else the grapheme there.
    fn end_at(&self, point: Point, text: core::Rectangle) -> SelectionEnd {
        if point.y < text.y {
            return SelectionEnd::new(0, 0);
        }
        let lines = &self.paragraph.buffer().lines;
        if point.y > text.y + text.height || lines.is_empty() {
            let line = lines.len().saturating_sub(1);
            let index = lines.get(line).map_or(0, |l| {
                iced_widget::text_input::Value::new(l.text()).len()
            });
            return SelectionEnd::new(line, index);
        }
        let (line, index) =
            self.grapheme_line_and_index(point, text).unwrap_or((0, 0));
        SelectionEnd::new(line, index)
    }

    fn selection(&self) -> Vec<core::Rectangle> {
        let Selection { start, end, .. } = self.selection;
        // OpenCircle patch: the shared, bidi-aware `selected_spans` (see text.rs).
        crate::text::selected_rects(
            self.paragraph.buffer(),
            (start.line, start.index),
            (end.line, end.index),
            visual_lines_offset(start.line, self.paragraph.buffer()),
        )
    }

    fn update_visual_bounds(&mut self) {
        // OpenCircle patch: each visual line's whole extent, whatever its direction.
        self.visual_lines_bounds =
            crate::text::line_bounds(self.paragraph.buffer());
    }
}

impl<Link, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Rich<'_, Link, Message, Theme, Renderer>
where
    Link: Clone + 'static,
    Theme: Catalog,
    Renderer: core::text::Renderer<Paragraph = Paragraph, Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Link>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Link> {
            spans: Vec::new(),
            span_pressed: None,
            hovered_link: None,
            paragraph: Paragraph::default(),
            is_hovered: false,
            selection: Selection::default(),
            dragging: None,
            last_click: None,
            keyboard_modifiers: keyboard::Modifiers::default(),
            visual_lines_bounds: Vec::new(),
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout(
            tree.state.downcast_mut::<State<Link>>(),
            renderer,
            limits,
            self.width,
            self.height,
            self.spans.as_ref().as_ref(),
            self.line_height,
            self.size,
            self.font,
            self.align_x,
            self.align_y,
            self.wrapping,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }

        let state = tree.state.downcast_ref::<State<Link>>();

        let style = theme.style(&self.class);

        for (index, span) in self.spans.as_ref().as_ref().iter().enumerate() {
            let is_hovered_link = self.on_link_click.is_some()
                && Some(index) == state.hovered_link;

            if span.highlight.is_some()
                || span.underline
                || span.strikethrough
                || is_hovered_link
            {
                let translation =
                    origin(&state.paragraph, layout.bounds()) - Point::ORIGIN;
                let regions = state.paragraph.span_bounds(index);

                if let Some(highlight) = span.highlight {
                    for bounds in &regions {
                        let bounds = Rectangle::new(
                            bounds.position()
                                - Vector::new(
                                    span.padding.left,
                                    span.padding.top,
                                ),
                            bounds.size()
                                + Size::new(span.padding.x(), span.padding.y()),
                        );

                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: bounds + translation,
                                border: highlight.border,
                                ..Default::default()
                            },
                            highlight.background,
                        );
                    }
                }

                if span.underline || span.strikethrough || is_hovered_link {
                    let size = span
                        .size
                        .or(self.size)
                        .unwrap_or(renderer.default_size());

                    let line_height = span
                        .line_height
                        .unwrap_or(self.line_height)
                        .to_absolute(size);

                    let color = span
                        .color
                        .or(style.color)
                        .unwrap_or(defaults.text_color);

                    let baseline = translation
                        + Vector::new(
                            0.0,
                            size.0 + (line_height.0 - size.0) / 2.0,
                        );

                    if span.underline || is_hovered_link {
                        for bounds in &regions {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle::new(
                                        bounds.position() + baseline
                                            - Vector::new(0.0, size.0 * 0.08),
                                        Size::new(bounds.width, 1.0),
                                    ),
                                    ..Default::default()
                                },
                                color,
                            );
                        }
                    }

                    if span.strikethrough {
                        for bounds in &regions {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle::new(
                                        bounds.position() + baseline
                                            - Vector::new(0.0, size.0 / 2.0),
                                        Size::new(bounds.width, 1.0),
                                    ),
                                    ..Default::default()
                                },
                                color,
                            );
                        }
                    }
                }
            }
        }

        if !state.selection.is_empty() {
            let bounds = layout.bounds();
            let translation = origin(&state.paragraph, bounds) - Point::ORIGIN;
            let ranges = state.selection();

            for range in ranges
                .into_iter()
                .filter_map(|range| bounds.intersection(&(range + translation)))
            {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: range,
                        ..renderer::Quad::default()
                    },
                    style.selection,
                );
            }
        }

        crate::text::draw(
            renderer,
            defaults,
            layout.bounds(),
            &state.paragraph,
            style,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State<Link>>();

        let bounds = layout.bounds();
        let click_position = cursor.position_in(bounds);
        // OpenCircle patch: the glyphs' box (an aligned paragraph is drawn at
        // its anchor, not at the widget's left edge).
        let text = Rectangle::new(
            origin(&state.paragraph, bounds),
            state.paragraph.min_bounds(),
        );

        let group = crate::group::anchor(); // OpenCircle patch
        if viewport.intersection(&bounds).is_none()
            && state.selection.is_empty()
            && state.dragging.is_none()
            && group.is_none()
        {
            return;
        }

        let was_hovered = state.is_hovered;
        let hovered_link_before = state.hovered_link;
        let selection_before = state.selection;

        state.is_hovered = click_position.is_some();

        if let Some(position) = click_position {
            let position = position + (bounds.position() - text.position());
            state.hovered_link =
                state.paragraph.hit_span(position).and_then(|span| {
                    if self.spans.as_ref().as_ref().get(span)?.link.is_some() {
                        Some(span)
                    } else {
                        None
                    }
                });
        } else {
            state.hovered_link = None;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if state.hovered_link.is_some() {
                    state.span_pressed = state.hovered_link;
                    shell.capture_event();
                }

                if let Some(position) = cursor.position_over(bounds) {
                    let click = click::Click::new(
                        position,
                        mouse::Button::Left,
                        state.last_click,
                        self.click_interval,
                    );

                    let (line, index) = state
                        .grapheme_line_and_index(position, text)
                        .unwrap_or((0, 0));

                    match click.kind() {
                        click::Kind::Single => {
                            let new_end = SelectionEnd { line, index };

                            if state.keyboard_modifiers.shift() {
                                state.selection.change_selection(new_end);
                            } else {
                                state.selection.select_range(new_end, new_end);
                                // OpenCircle patch: the drag may go on into
                                // the other texts of its group
                                let _ = crate::group::start(position);
                            }

                            state.dragging = Some(Dragging::Grapheme);
                        }
                        click::Kind::Double => {
                            state.selection.select_word(
                                line,
                                index,
                                &state.paragraph,
                            );
                            state.dragging = Some(Dragging::Word);
                        }
                        click::Kind::Triple => {
                            state.selection.select_line(line, &state.paragraph);
                            state.dragging = Some(Dragging::Line);
                        }
                    }

                    state.last_click = Some(click);

                    shell.capture_event();
                } else {
                    state.selection = Selection::default();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                state.dragging = None;
                if !matches!(
                    event,
                    Event::Touch(touch::Event::FingerLost { .. })
                ) && state.selection.is_empty()
                {
                    match state.span_pressed {
                        Some(span) if Some(span) == state.hovered_link => {
                            if let Some((link, on_link_clicked)) = self
                                .spans
                                .as_ref()
                                .as_ref()
                                .get(span)
                                .and_then(|span| span.link.clone())
                                .zip(self.on_link_click.as_deref())
                            {
                                shell.publish(on_link_clicked(link));
                            }
                        }
                        _ => {}
                    }

                    state.span_pressed = None;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                // OpenCircle patch: a drag in the group: this text's part of it
                if let Some(anchor) = group
                    && let Some(position) = cursor.land().position()
                {
                    let (from, to) =
                        (state.end_at(anchor, text), state.end_at(position, text));
                    state.selection.select_range(from, to);
                } else if let Some(position) = cursor.land().position()
                    && let Some(dragging) = state.dragging
                {
                    let (line, index) = state
                        .grapheme_line_and_index(position, text)
                        .unwrap_or((0, 0));

                    match dragging {
                        Dragging::Grapheme => {
                            let new_end = SelectionEnd { line, index };

                            state.selection.change_selection(new_end);
                        }
                        Dragging::Word => {
                            let new_end = SelectionEnd { line, index };

                            state.selection.change_selection_by_word(
                                new_end,
                                &state.paragraph,
                            );
                        }
                        Dragging::Line => {
                            state.selection.change_selection_by_line(
                                line,
                                &state.paragraph,
                            );
                        }
                    };
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                match key.as_ref() {
                    keyboard::Key::Character("c")
                        if state.keyboard_modifiers.command()
                            && !state.selection.is_empty() =>
                    {
                        // OpenCircle patch: in a group, one text for all
                        let copied = state.selection.text(&state.paragraph);
                        if let Err(copied) = crate::group::copy(copied) {
                            clipboard.write(clipboard::Kind::Standard, copied);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Character("a")
                        if state.keyboard_modifiers.command()
                            && !state.selection.is_empty() =>
                    {
                        state.selection.select_all(&state.paragraph);

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::Home)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.jump() {
                            state.selection.select_beginning();
                        } else {
                            state.selection.select_line_beginning();
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::End)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.jump() {
                            state.selection.select_end(&state.paragraph);
                        } else {
                            state.selection.select_line_end(&state.paragraph);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::ArrowLeft)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.macos_command() {
                            state.selection.select_line_beginning();
                        } else if state.keyboard_modifiers.jump() {
                            state
                                .selection
                                .select_left_by_words(&state.paragraph);
                        } else {
                            state.selection.select_left(&state.paragraph);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::ArrowRight)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.macos_command() {
                            state.selection.select_line_end(&state.paragraph);
                        } else if state.keyboard_modifiers.jump() {
                            state
                                .selection
                                .select_right_by_words(&state.paragraph);
                        } else {
                            state.selection.select_right(&state.paragraph);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::ArrowUp)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.macos_command() {
                            state.selection.select_beginning();
                        } else if state.keyboard_modifiers.jump() {
                            state.selection.select_line_beginning();
                        } else {
                            state.selection.select_up(&state.paragraph);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::ArrowDown)
                        if state.keyboard_modifiers.shift()
                            && !state.selection.is_empty() =>
                    {
                        if state.keyboard_modifiers.macos_command() {
                            state.selection.select_end(&state.paragraph);
                        } else if state.keyboard_modifiers.jump() {
                            state.selection.select_line_end(&state.paragraph);
                        } else {
                            state.selection.select_down(&state.paragraph);
                        }

                        shell.capture_event();
                    }
                    keyboard::Key::Named(key::Named::Escape) => {
                        state.dragging = None;
                        state.selection = Selection::default();

                        state.keyboard_modifiers =
                            keyboard::Modifiers::default();

                        if state.selection != selection_before {
                            shell.capture_event();
                        }
                    }
                    _ => {}
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.keyboard_modifiers = *modifiers;
            }
            _ => {}
        }

        if state.is_hovered != was_hovered
            || state.selection != selection_before
            || state.hovered_link != hovered_link_before
        {
            if let Some(span) = state.hovered_link {
                if let Some((link, on_link_hovered)) = self
                    .spans
                    .as_ref()
                    .as_ref()
                    .get(span)
                    .and_then(|span| span.link.clone())
                    .zip(self.on_link_hover.as_deref())
                {
                    shell.publish(on_link_hovered(link));
                }
            } else if let Some(on_hover_lost) = self.on_hover_lost.as_deref() {
                shell.publish(on_hover_lost());
            }

            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State<Link>>();

        if state.hovered_link.is_some() {
            mouse::Interaction::Pointer
        } else if cursor.is_over(layout.bounds()) || state.dragging.is_some() {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn layout<Link, Renderer>(
    state: &mut State<Link>,
    renderer: &Renderer,
    limits: &layout::Limits,
    width: Length,
    height: Length,
    spans: &[Span<'_, Link, Renderer::Font>],
    line_height: LineHeight,
    size: Option<Pixels>,
    font: Option<Renderer::Font>,
    align_x: Alignment,
    align_y: alignment::Vertical,
    wrapping: Wrapping,
) -> layout::Node
where
    Link: Clone,
    Renderer: core::text::Renderer<Paragraph = Paragraph, Font = Font>,
{
    layout::sized(limits, width, height, |limits| {
        let bounds = limits.max();

        let size = size.unwrap_or_else(|| renderer.default_size());
        let font = font.unwrap_or_else(|| renderer.default_font());

        let text_with_spans = || core::Text {
            content: spans,
            bounds,
            size,
            line_height,
            font,
            align_x,
            align_y,
            shaping: Shaping::Advanced,
            wrapping,
        };

        if state.spans != spans {
            state.paragraph =
                Renderer::Paragraph::with_spans(text_with_spans());
            state.spans = spans.iter().cloned().map(Span::to_static).collect();
            state.update_visual_bounds();
        } else {
            match state.paragraph.compare(core::Text {
                content: (),
                bounds,
                size,
                line_height,
                font,
                align_x,
                align_y,
                shaping: Shaping::Advanced,
                wrapping,
            }) {
                core::text::Difference::None => {}
                core::text::Difference::Bounds => {
                    state.paragraph.resize(bounds);
                    state.update_visual_bounds();
                }
                core::text::Difference::Shape => {
                    state.paragraph =
                        Renderer::Paragraph::with_spans(text_with_spans());
                    state.update_visual_bounds();
                }
            }
        }

        state.paragraph.min_bounds()
    })
}

impl<'a, Link, Message, Theme, Renderer>
    FromIterator<Span<'a, Link, Renderer::Font>>
    for Rich<'a, Link, Message, Theme, Renderer>
where
    Link: Clone + 'a,
    Theme: Catalog,
    Renderer: core::text::Renderer<Paragraph = Paragraph, Font = Font>,
    Renderer::Font: 'a,
{
    fn from_iter<T: IntoIterator<Item = Span<'a, Link, Renderer::Font>>>(
        spans: T,
    ) -> Self {
        Self::with_spans(spans.into_iter().collect::<Vec<_>>())
    }
}

impl<'a, Link, Message, Theme, Renderer>
    From<Rich<'a, Link, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Link: Clone + 'a,
    Theme: Catalog + 'a,
    Renderer: core::text::Renderer<Paragraph = Paragraph, Font = Font> + 'a,
{
    fn from(
        text: Rich<'a, Link, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(text)
    }
}


fn visual_lines_offset(line: usize, buffer: &cosmic_text::Buffer) -> i32 {
    let scroll = buffer.scroll();

    let start = scroll.line.min(line);
    let end = scroll.line.max(line);

    let visual_lines_offset: usize = buffer.lines[start..]
        .iter()
        .take(end - start)
        .map(|line| line.layout_opt().map(Vec::len).unwrap_or_default())
        .sum();

    visual_lines_offset as i32 * if scroll.line < line { 1 } else { -1 }
}

/// OpenCircle patch: where `paragraph` is drawn in `bounds` (by its
/// alignment): the origin of its glyphs' coordinates. Upstream drew the
/// selection, span highlights and hit tests from the widget's left edge, so
/// on right-aligned (Arabic) or centred text they sat beside the text.
fn origin(paragraph: &Paragraph, bounds: Rectangle) -> Point {
    bounds.anchor(
        paragraph.min_bounds(),
        paragraph.align_x(),
        paragraph.align_y(),
    )
}
