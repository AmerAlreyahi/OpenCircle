//! Keyboard focus: the ONE mechanism every focusable kit control uses (iced
//! 0.14's buttons take no focus). A control is a `Stop` round what it draws
//! (`press` for a button: its mouse press and its keyboard press are the same
//! message); a window's view puts its content in a `scope`, which walks the
//! stops in tree order on Tab / Shift+Tab, wrapping at the ends. Text fields
//! keep iced's own focus and join the same walk (iced's `focusable`
//! operation counts both). Tree order is reading order in both directions:
//! `dir::row` keeps its children in reading order and only mirrors their
//! places, so right to left the walk goes right to left by itself.
//!
//! Keys on the focused stop: Enter / Space press it; the arrows step a value
//! (`Stop::steps`: segmented, slider; left / right as seen, so mirrored in
//! RTL). The focused control shows a soft background (`shade`: the theme's
//! `accent_soft` over it, in its shape; never a ring or a border, owner's
//! rule), only while the keyboard leads: Tab or a key on a stop turns it on,
//! any mouse press off and unfocuses every stop (a click never leaves it). A
//! new scope (a window opened again) starts with nothing focused and the
//! mouse leading. A dropdown is its own stop (`dropdown.rs`) on these same
//! parts (`State`, `key`, `shade`). Mouse-only presses (window drags,
//! resize edges, the caption buttons) say so with `// mouse only` (the
//! architecture test `every_kit_press_is_a_focus_stop`).
use std::cell::Cell;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Id, Operation, Tree, operation, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard::{self, key::Named};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Border, Element, Event, Length, Rectangle, Size, Vector};

use super::themes::c;
use super::tokens::focus::MARGIN;

type R = iced::Renderer;
type T = iced::Theme;

/// A question's Cancel (`button::cancel`, `setting_row::cancel`): the
/// screen focuses it by this id when the question opens.
pub const CANCEL: &str = "kit-focus-cancel";

thread_local! {
    /// The keyboard leads: the focus shows (the UI thread's; a new scope resets it).
    static KEYBOARD: Cell<bool> = const { Cell::new(false) };
}

/// The focus shows now (the last input was a key, not the mouse).
pub fn keyboard() -> bool {
    KEYBOARD.get()
}

pub(super) fn lead(keyboard: bool) {
    KEYBOARD.set(keyboard);
}

/// A step of a value by the arrows, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Back,
    On,
}

/// What a key does to a focused control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Enter / Space.
    Press,
    Step(Step),
}

/// The key `event` is for a focused control (none with Ctrl / Alt / Logo):
/// left / right as seen (`rtl`: left goes on); up goes back, unless
/// `up_is_on` (a slider: up = more).
pub fn key(event: &Event, rtl: bool, up_is_on: bool) -> Option<Key> {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    let keyboard::Key::Named(named) = key else {
        return None;
    };
    let ahead = |on: bool| Key::Step(if on { Step::On } else { Step::Back });
    Some(match named {
        Named::Enter | Named::Space => Key::Press,
        Named::ArrowRight => ahead(!rtl),
        Named::ArrowLeft => ahead(rtl),
        Named::ArrowDown => ahead(!up_is_on),
        Named::ArrowUp => ahead(up_is_on),
        _ => return None,
    })
}

/// A control's focus (iced's `Focusable`, so the walk and `focus(id)` reach it).
#[derive(Debug, Default)]
pub struct State {
    focused: bool,
}

impl State {
    pub fn focused(&self) -> bool {
        self.focused
    }
    /// A mouse press anywhere: the focus goes (the mouse leads now).
    pub fn on_mouse(&mut self, event: &Event, shell: &mut Shell<'_, impl Sized>) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            lead(false);
            if std::mem::take(&mut self.focused) {
                shell.request_redraw();
            }
        }
    }
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

/// The soft background over `bounds` (a control of corner `radius`), while
/// the keyboard leads: the theme's `accent_soft`, a little stronger than hover
/// (the overlay bar's Tab focus looks the same). On a control filled with a
/// colour (`Stop::on_fill`: a call to action) the accent would not show: its
/// text colour at `accent_soft`'s strength instead.
pub fn shade(renderer: &mut R, bounds: Rectangle, radius: f32, fill: Option<iced::Color>) {
    use iced::advanced::Renderer as _;
    if !keyboard() {
        return;
    }
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: Border::default().rounded(radius),
            ..renderer::Quad::default()
        },
        match fill {
            Some(f) => iced::Color {
                a: c().accent_soft.a,
                ..super::themes::on(f)
            },
            None => c().accent_soft,
        },
    );
}

/// A focusable control: `content` and what its keys send.
pub struct Stop<'a, Message> {
    content: Element<'a, Message>,
    radius: f32,
    press: Option<Message>,
    steps: Option<Box<dyn Fn(Step) -> Option<Message> + 'a>>,
    up_is_on: bool,
    rtl: bool,
    id: Option<Id>,
    fill: Option<iced::Color>,
}

/// `content` as a stop of corner `radius`; Enter / Space send `press` (none
/// and no `steps`: it can't be focused, as a dimmed control).
pub fn stop<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    radius: f32,
    press: Option<Message>,
) -> Stop<'a, Message> {
    Stop {
        content: content.into(),
        radius,
        press,
        steps: None,
        up_is_on: false,
        rtl: super::dir::rtl(),
        id: None,
        fill: None,
    }
}

/// A kit button as a stop: the mouse and the keyboard press it alike.
pub fn press<'a, Message: Clone + 'a>(
    button: iced::widget::Button<'a, Message>,
    radius: f32,
    on_press: Option<Message>,
) -> Stop<'a, Message> {
    stop(button.on_press_maybe(on_press.clone()), radius, on_press)
}

impl<'a, Message: 'a> Stop<'a, Message> {
    /// The arrows step its value: what each step sends (none: at the end).
    pub fn steps(mut self, steps: impl Fn(Step) -> Option<Message> + 'a) -> Self {
        self.steps = Some(Box::new(steps));
        self
    }
    /// Up is more (a slider), not back.
    pub fn up_is_on(mut self) -> Self {
        self.up_is_on = true;
        self
    }
    /// Its id (`iced::widget::operation::focus` finds it: a confirm's Cancel).
    /// The control is filled with `fill` (a call to action): the focus shade
    /// is drawn to show on it.
    pub fn on_fill(mut self, fill: iced::Color) -> Self {
        self.fill = Some(fill);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(Id::new(id));
        self
    }
    fn live(&self) -> bool {
        self.press.is_some() || self.steps.is_some()
    }
}

impl<'a, Message: Clone + 'a> From<Stop<'a, Message>> for Element<'a, Message> {
    fn from(stop: Stop<'a, Message>) -> Self {
        Element::new(stop)
    }
}

impl<Message: Clone> Widget<Message, T, R> for Stop<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        (self.content.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &T,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        (self.content.as_widget()).draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if tree.state.downcast_ref::<State>().focused {
            shade(renderer, layout.bounds(), self.radius, self.fill);
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        if self.live() {
            let state = tree.state.downcast_mut::<State>();
            operation.focusable(self.id.as_ref(), layout.bounds(), state);
        }
        let content = &mut self.content;
        let child = &mut tree.children[0];
        operation.traverse(&mut |op| {
            content.as_widget_mut().operate(child, layout, renderer, op);
        });
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &R,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        (self.content.as_widget_mut()).update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let state = tree.state.downcast_mut::<State>();
        state.on_mouse(event, shell);
        if !state.focused || shell.is_event_captured() {
            return;
        }
        let sent = match key(event, self.rtl, self.up_is_on) {
            Some(Key::Press) => self.press.clone(),
            Some(Key::Step(s)) => match &self.steps {
                Some(steps) => steps(s),
                None => return,
            },
            None => return,
        };
        lead(true);
        if let Some(m) = sent {
            shell.publish(m);
        }
        shell.capture_event();
        shell.request_redraw();
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &R,
    ) -> mouse::Interaction {
        (self.content.as_widget()).mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, T, R>> {
        (self.content.as_widget_mut()).overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// A window's content with Tab / Shift+Tab walking its stops (and fields).
pub fn scope<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Scope {
        content: content.into(),
    })
}

struct Scope<'a, Message> {
    content: Element<'a, Message>,
}

/// Runs `op` (and what it chains) over `content`.
fn run<Message>(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &R,
    op: Box<dyn Operation>,
) {
    let mut op = op;
    loop {
        content
            .as_widget_mut()
            .operate(tree, layout, renderer, op.as_mut());
        match op.finish() {
            operation::Outcome::Chain(next) => op = next,
            _ => return,
        }
    }
}

/// Walks the focus one stop on (`back`: one back) over `content`, wrapping:
/// past the last stop iced's walk focuses nothing, so it walks once more.
pub fn walk<Message>(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &R,
    back: bool,
) {
    for _ in 0..2 {
        let op: Box<dyn Operation> = match back {
            true => Box::new(operation::focusable::focus_previous()),
            false => Box::new(operation::focusable::focus_next()),
        };
        run(content, tree, layout, renderer, op);
        let mut n = Count::default();
        content
            .as_widget_mut()
            .operate(tree, layout, renderer, &mut n);
        if n.focused.is_some() || n.total == 0 {
            return;
        }
    }
}

/// Focuses the `n`th stop (or field) and no other (a menu opened by the keyboard: its choice).
pub fn nth(n: usize) -> impl Operation {
    struct Nth(usize, usize);
    impl Operation for Nth {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn focusable(
            &mut self,
            _: Option<&Id>,
            _: Rectangle,
            state: &mut dyn operation::Focusable,
        ) {
            match self.1 == self.0 {
                true => state.focus(),
                false => state.unfocus(),
            }
            self.1 += 1;
        }
    }
    Nth(n, 0)
}

/// How many stops (and fields) there are, and whether one is focused.
#[derive(Default)]
struct Count {
    total: usize,
    focused: Option<usize>,
}

impl Operation for Count {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn operation::Focusable) {
        if state.is_focused() {
            self.focused = Some(self.total);
        }
        self.total += 1;
    }
}

/// How far a scroll area showing `view` (scrolled by `translation`) must
/// scroll so `target` (laid out unscrolled) is in it, `MARGIN` clear of its
/// edges: 0 when it is.
pub fn reveal(target: Rectangle, view: Rectangle, translation: Vector) -> f32 {
    let top = target.y - translation.y;
    let bottom = top + target.height;
    if top < view.y + MARGIN {
        top - view.y - MARGIN
    } else if bottom > view.y + view.height - MARGIN {
        (bottom - (view.y + view.height - MARGIN)).min(top - view.y - MARGIN)
    } else {
        0.0
    }
}

/// Scrolls the scroll area holding the focused control so it shows.
// ponytail: the innermost area only (Settings has one); nested areas each
// scrolled by their own share when a screen needs it.
fn scroll_to_focus<Message>(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &R,
) -> bool {
    /// A scroll area: its index in the walk, its view, how far it is scrolled.
    type Area = (usize, Rectangle, Vector);
    /// The focused control and the area round it.
    #[derive(Default)]
    struct Find {
        areas: usize,
        next: Option<Area>,
        open: Vec<Area>,
        found: Option<(Rectangle, Option<Area>)>,
    }
    impl Operation for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            // an area's own `scrollable` call comes right before its children
            let area = self.next.take();
            if let Some(a) = area {
                self.open.push(a);
            }
            operate(self);
            if area.is_some() {
                self.open.pop();
            }
        }
        fn scrollable(
            &mut self,
            _: Option<&Id>,
            bounds: Rectangle,
            _: Rectangle,
            translation: Vector,
            _: &mut dyn operation::Scrollable,
        ) {
            self.next = Some((self.areas, bounds, translation));
            self.areas += 1;
        }
        fn focusable(
            &mut self,
            _: Option<&Id>,
            bounds: Rectangle,
            state: &mut dyn operation::Focusable,
        ) {
            if state.is_focused() && self.found.is_none() {
                self.found = Some((bounds, self.open.last().copied()));
            }
        }
    }
    struct By(usize, f32, usize);
    impl Operation for By {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            _: Vector,
            state: &mut dyn operation::Scrollable,
        ) {
            if self.2 == self.0 {
                state.scroll_by(AbsoluteOffset { x: 0.0, y: self.1 }, bounds, content);
            }
            self.2 += 1;
        }
    }
    let mut find = Find::default();
    content
        .as_widget_mut()
        .operate(tree, layout, renderer, &mut find);
    let Some((target, Some((area, view, translation)))) = find.found else {
        return false;
    };
    let dy = reveal(target, view, translation);
    if dy == 0.0 {
        return false;
    }
    content
        .as_widget_mut()
        .operate(tree, layout, renderer, &mut By(area, dy, 0));
    true
}

/// A scope's tree state: none; making it marks a new scope (a window opened).
struct Fresh;

impl<Message> Widget<Message, T, R> for Scope<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Fresh>()
    }
    /// Made once per window (a reopened Settings is a new tree): nothing of the
    /// last session's focus carries over, the mouse leads until a key.
    fn state(&self) -> tree::State {
        lead(false);
        tree::State::new(Fresh)
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) -> layout::Node {
        (self.content.as_widget_mut()).layout(&mut tree.children[0], renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &T,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        (self.content.as_widget()).draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        (self.content.as_widget_mut()).operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &R,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            lead(false);
        }
        (self.content.as_widget_mut()).update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Tab),
            modifiers,
            ..
        }) = event
        else {
            return;
        };
        if shell.is_event_captured() || modifiers.control() || modifiers.alt() {
            return;
        }
        let child = &mut tree.children[0];
        walk(
            &mut self.content,
            child,
            layout,
            renderer,
            modifiers.shift(),
        );
        if scroll_to_focus(&mut self.content, child, layout, renderer) {
            shell.invalidate_layout();
        }
        lead(true);
        shell.capture_event();
        shell.request_redraw();
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &R,
    ) -> mouse::Interaction {
        (self.content.as_widget()).mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, T, R>> {
        (self.content.as_widget_mut()).overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Named;
    use iced::widget::{column, text};
    use iced::{Element, Event, Rectangle, Size, Vector};

    use super::super::testing::shot;
    use super::super::{button, dir, segmented};

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum M {
        A,
        B,
        Pick(bool),
    }

    fn key(k: Named, shift: bool) -> Event {
        shot::named(k, shift)[1].clone()
    }

    /// Two buttons side by side, a dimmed one (no stop), then Off / On.
    fn view(rtl: bool) -> Element<'static, M> {
        let _dir = dir::scope(rtl);
        let b = |s, m| button::button(text(s), button::Kind::Elevated, m);
        super::scope(column![
            crate::dir_row![b("A", Some(M::A)), b("B", Some(M::B))].spacing(20),
            b("dimmed", None),
            segmented::boolean(["Off", "On"], false, false, 1.0, M::Pick),
        ])
    }

    const SIZE: Size = Size::new(400.0, 200.0);

    fn walk(rtl: bool, events: &[Event]) -> (Vec<M>, Vec<Option<Rectangle>>) {
        shot::focus_walk(|| view(rtl), SIZE, events)
    }

    /// Tab walks in reading order and wraps; right to left the row's first
    /// stop is at the right; Shift+Tab from nothing goes to the last.
    #[test]
    fn tab_walks_in_reading_order_both_ways() {
        let tab = key(Named::Tab, false);
        for rtl in [false, true] {
            let (_, at) = walk(rtl, &[tab.clone(), tab.clone(), tab.clone(), tab.clone()]);
            let at: Vec<_> = at.into_iter().map(Option::unwrap).collect();
            let (a, b, track) = (at[0], at[1], at[2]);
            assert_eq!(at[3], a, "it wraps to the first");
            match rtl {
                false => assert!(a.x < b.x, "{a:?} {b:?}"),
                true => assert!(a.x > b.x, "right to left: A at the right {a:?} {b:?}"),
            }
            assert!(track.y > a.y, "the dimmed button is skipped: {track:?}");
        }
        let (_, at) = walk(false, &[key(Named::Tab, true)]);
        let (_, all) = walk(
            false,
            &[
                key(Named::Tab, false),
                key(Named::Tab, false),
                key(Named::Tab, false),
            ],
        );
        assert_eq!(at[0], all[2], "Shift+Tab from nothing: the last");
    }

    /// Enter and Space press the focused control; the arrows step a track
    /// (left goes on right to left); nothing happens without focus.
    #[test]
    fn enter_space_and_arrows_act_on_the_focused_control() {
        let tab = || key(Named::Tab, false);
        let (m, _) = walk(false, &[key(Named::Enter, false), key(Named::Space, false)]);
        assert!(m.is_empty(), "no focus, no press: {m:?}");
        let (m, _) = walk(
            false,
            &[
                tab(),
                key(Named::Enter, false),
                tab(),
                key(Named::Space, false),
            ],
        );
        assert_eq!(m, [M::A, M::B]);
        for (rtl, arrow, want) in [
            (false, Named::ArrowRight, vec![M::Pick(true)]),
            (false, Named::ArrowLeft, vec![]),
            (true, Named::ArrowLeft, vec![M::Pick(true)]),
            (false, Named::Space, vec![M::Pick(true)]),
        ] {
            let events = [tab(), tab(), tab(), key(arrow, false)];
            assert_eq!(walk(rtl, &events).0, want, "rtl {rtl} {arrow:?}");
        }
    }

    /// The focus shows after Tab, not after a click (and a click clears it).
    #[test]
    fn the_focus_shows_only_after_the_keyboard() {
        use iced::mouse;
        let size = Size::new(200.0, 120.0);
        let rest = shot::render_with(view(false), size, None, &[]);
        let tab = shot::render_with(view(false), size, None, &[key(Named::Tab, false)]);
        assert_ne!(rest, tab, "Tab shades the control");
        let click = [
            key(Named::Tab, false),
            Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::new(190.0, 110.0),
            }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ];
        let clicked = shot::render_with(view(false), size, None, &click);
        assert_eq!(rest, clicked, "a click: no focus shown");
        let (_, at) = walk(false, &click);
        assert_eq!(at[2], None, "a click: no focus");
        let (_, at) = walk(false, &click[1..]);
        assert_eq!(at[1], None, "a click never focuses");
    }

    /// A window opened again (a new scope, a new tree) starts clean: nothing
    /// focused, the mouse leading (Tab focus once carried over to the next
    /// session).
    #[test]
    fn a_new_scope_starts_with_no_focus() {
        let (_, at) = walk(false, &[key(Named::Tab, false)]);
        assert!(at[0].is_some() && super::keyboard(), "Tab focused one");
        let (sent, at) = walk(false, &[key(Named::Enter, false)]);
        assert!(sent.is_empty(), "nothing pressed: {sent:?}");
        assert_eq!(at[0], None, "nothing focused");
        assert!(!super::keyboard(), "the mouse leads again");
    }

    #[test]
    fn a_focused_control_is_scrolled_into_view() {
        let view = Rectangle::new(iced::Point::new(0.0, 100.0), Size::new(300.0, 200.0));
        let at = |y| Rectangle::new(iced::Point::new(0.0, y), Size::new(100.0, 30.0));
        let none = Vector::ZERO;
        assert_eq!(super::reveal(at(150.0), view, none), 0.0);
        assert_eq!(
            super::reveal(at(290.0), view, none),
            32.0,
            "below: up by its overhang"
        );
        assert_eq!(super::reveal(at(400.0), view, Vector::new(0.0, 150.0)), 0.0);
        assert_eq!(super::reveal(at(90.0), view, none), -22.0, "above: back");
    }
}
