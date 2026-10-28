//! Headless renders for tests: an `Element` to RGBA pixels, and to a PNG when
//! `OPENCIRCLE_RENDER_DIR` is set.
use iced::advanced::{graphics::Viewport, mouse, renderer};
use iced::{Color, Element, Pixels, Rectangle, Size, Theme};
use iced_runtime::user_interface::{Cache, UserInterface};

/// A renderer as the app's: its fonts loaded, the UI font by default.
pub fn renderer() -> iced::Renderer {
    super::super::text::load();
    iced::Renderer::new(super::super::text::ui_font(), Pixels(16.0))
}

pub fn render<Message>(view: Element<'_, Message>, width: u32, height: u32) -> Vec<u8> {
    render_twice(view, width, height, None).0
}

/// `view` drawn whole, then (if `damage`) drawn again over it within
/// `damage` only, as a window redraws what changed: (first, second).
pub fn render_twice<Message>(
    view: Element<'_, Message>,
    width: u32,
    height: u32,
    damage: Option<Rectangle>,
) -> (Vec<u8>, Vec<u8>) {
    let mut renderer = renderer();
    let size = Size::new(width as f32, height as f32);
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let style = renderer::Style {
        text_color: Color::WHITE,
    };
    ui.draw(
        &mut renderer,
        &Theme::Dark,
        &style,
        mouse::Cursor::Unavailable,
    );
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    let mut mask = tiny_skia::Mask::new(width, height).unwrap();
    let viewport = Viewport::with_physical_size(Size::new(width, height), 1.0);
    let background = crate::ui::theme::current().window;
    // iced draws B, G, R, A.
    let rgba = |p: &tiny_skia::Pixmap| -> Vec<u8> {
        (p.data().as_chunks::<4>().0.iter())
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    };
    let all = Rectangle::with_size(size);
    renderer.draw(
        &mut pixmap.as_mut(),
        &mut mask,
        &viewport,
        &[all],
        background,
    );
    let first = rgba(&pixmap);
    let Some(damage) = damage else {
        return (first, Vec::new());
    };
    renderer.draw(
        &mut pixmap.as_mut(),
        &mut mask,
        &viewport,
        &[damage],
        background,
    );
    (first, rgba(&pixmap))
}

/// `view` after `events` (a hover, a press, a click that focuses), drawn whole
/// with the cursor at `cursor` and its widgets' status as a window shows it
/// (a redraw request ends the events: buttons take their status from it).
pub fn render_with<Message>(
    view: Element<'_, Message>,
    size: Size,
    cursor: Option<iced::Point>,
    events: &[iced::Event],
) -> Vec<u8> {
    use iced::{Event, window};
    let mut renderer = renderer();
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let cursor = cursor.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
    let mut events = events.to_vec();
    events.push(Event::Window(window::Event::RedrawRequested(
        std::time::Instant::now(),
    )));
    let mut messages = Vec::new();
    ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut iced::advanced::clipboard::Null,
        &mut messages,
    );
    let style = renderer::Style {
        text_color: Color::WHITE,
    };
    ui.draw(&mut renderer, &Theme::Dark, &style, cursor);
    let (w, h) = (size.width as u32, size.height as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    renderer.draw(
        &mut pixmap.as_mut(),
        &mut mask,
        &Viewport::with_physical_size(Size::new(w, h), 1.0),
        &[Rectangle::with_size(size)],
        crate::ui::theme::current().window,
    );
    // iced draws B, G, R, A.
    (pixmap.data().as_chunks::<4>().0.iter())
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect()
}

/// `views` drawn one after another into one window at `scale` (logical
/// `size`), each frame redrawn only where its layers differ from the frame
/// before, as the window compositor does (`iced_tiny_skia::window::compositor`).
/// Returns (the last frame's pixels, the last view drawn whole).
pub fn redrawn<'a, Message: 'a>(
    mut views: impl FnMut(usize) -> Option<Element<'a, Message>>,
    size: Size,
    scale: f32,
) -> (Vec<u8>, Vec<u8>) {
    use iced::advanced::graphics::damage;
    let mut renderer = renderer();
    let (w, h) = (
        (size.width * scale).round() as u32,
        (size.height * scale).round() as u32,
    );
    let viewport = Viewport::with_physical_size(Size::new(w, h), scale);
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    let background = crate::ui::theme::current().window;
    let all = Rectangle::with_size(size);
    let style = renderer::Style {
        text_color: Color::WHITE,
    };
    let rgba = |p: &tiny_skia::Pixmap| -> Vec<u8> {
        (p.data().as_chunks::<4>().0.iter())
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    };
    let (mut window, mut fresh) = (tiny_skia::Pixmap::new(w, h).unwrap(), None);
    let mut last: Option<Vec<iced_tiny_skia::Layer>> = None;
    let mut i = 0;
    while let Some(view) = views(i) {
        let ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
        let mut ui = ui;
        ui.draw(
            &mut renderer,
            &Theme::Dark,
            &style,
            mouse::Cursor::Unavailable,
        );
        let layers = renderer.layers().to_vec();
        let changed = match &last {
            Some(last) => iced_tiny_skia::Layer::frame_damage(last, &layers),
            None => vec![all],
        };
        let changed = damage::group(changed, all);
        if !changed.is_empty() {
            renderer.draw(
                &mut window.as_mut(),
                &mut mask,
                &viewport,
                &changed,
                background,
            );
        }
        let mut whole = tiny_skia::Pixmap::new(w, h).unwrap();
        renderer.draw(
            &mut whole.as_mut(),
            &mut mask,
            &viewport,
            &[all],
            background,
        );
        fresh = Some(rgba(&whole));
        last = Some(layers);
        i += 1;
    }
    (rgba(&window), fresh.unwrap_or_default())
}

/// The messages one left click at `at` produces (press then release) in `view`.
pub fn click<Message>(view: Element<'_, Message>, size: Size, at: iced::Point) -> Vec<Message> {
    use iced::advanced::clipboard;
    use iced::{Event, mouse::Button, mouse::Event as Mouse};
    let mut renderer = renderer();
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    let events = [
        Event::Mouse(Mouse::CursorMoved { position: at }),
        Event::Mouse(Mouse::ButtonPressed(Button::Left)),
        Event::Mouse(Mouse::ButtonReleased(Button::Left)),
    ];
    ui.update(
        &events,
        mouse::Cursor::Available(at),
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    messages
}

/// A clipboard that remembers what was written to it.
#[derive(Default)]
pub struct Clip(pub Vec<String>);

impl iced::advanced::Clipboard for Clip {
    fn read(&self, _: iced::advanced::clipboard::Kind) -> Option<String> {
        None
    }
    fn write(&mut self, _: iced::advanced::clipboard::Kind, text: String) {
        self.0.push(text);
    }
}

/// The events of typing `text` (each character is a key press with its text).
pub fn typing(text: &str) -> Vec<iced::Event> {
    use iced::keyboard::{Key, key::Named};
    text.chars()
        .map(|c| match c {
            ' ' => press(Key::Named(Named::Space), c, false),
            c => press(Key::Character(c.to_string().into()), c, false),
        })
        .collect()
}

/// Ctrl+`c` (the modifier change, the press as the OS sends it - the control
/// character as its text - and the release of the modifier).
pub fn chord(c: char) -> Vec<iced::Event> {
    use iced::keyboard::{Event, Modifiers};
    let control = char::from(c as u8 & 0x1f);
    vec![
        iced::Event::Keyboard(Event::ModifiersChanged(Modifiers::CTRL)),
        press(
            iced::keyboard::Key::Character(c.to_string().into()),
            control,
            true,
        ),
        iced::Event::Keyboard(Event::ModifiersChanged(Modifiers::empty())),
    ]
}

/// A named key (Esc, Tab, Enter, Space); Space also types its character.
pub fn named(key: iced::keyboard::key::Named, shift: bool) -> Vec<iced::Event> {
    use iced::keyboard::{Event, Key, Location, Modifiers, key::Named};
    let mods = if shift {
        Modifiers::SHIFT
    } else {
        Modifiers::empty()
    };
    vec![
        iced::Event::Keyboard(Event::ModifiersChanged(mods)),
        iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: Location::Standard,
            modifiers: mods,
            text: (key == Named::Space).then(|| " ".into()),
            repeat: false,
        }),
        iced::Event::Keyboard(Event::ModifiersChanged(Modifiers::empty())),
    ]
}

fn press(key: iced::keyboard::Key, text: char, ctrl: bool) -> iced::Event {
    use iced::keyboard::{Event, Location, Modifiers, key};
    let code = match key.as_ref() {
        iced::keyboard::Key::Character(s) => match s {
            "c" => key::Code::KeyC,
            "s" => key::Code::KeyS,
            "a" => key::Code::KeyA,
            _ => key::Code::KeyT,
        },
        _ => key::Code::KeyT,
    };
    iced::Event::Keyboard(Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: key::Physical::Code(code),
        location: Location::Standard,
        modifiers: if ctrl {
            Modifiers::CTRL
        } else {
            Modifiers::empty()
        },
        text: Some(text.to_string().into()),
        repeat: false,
    })
}

/// A text field or editor of `view` to give the focus first.
pub enum Focus {
    /// None: nothing is focused.
    Nothing,
    /// The field with this id.
    Id(&'static str),
}

/// Feeds key/mouse `events` to `view` (after `focus`) and returns the messages
/// and what each event did (captured by a widget or not); the clipboard is
/// `clipboard`.
pub fn feed<Message>(
    view: Element<'_, Message>,
    size: Size,
    focus: Focus,
    events: &[iced::Event],
    clipboard: &mut dyn iced::advanced::Clipboard,
) -> (Vec<Message>, Vec<iced::event::Status>) {
    use iced::advanced::widget::{Id, operation::focusable};
    let mut renderer = renderer();
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let mut all = Vec::new();
    let cursor = mouse::Cursor::Unavailable;
    match focus {
        Focus::Nothing => {}
        Focus::Id(id) => ui.operate(&renderer, &mut focusable::focus::<()>(Id::new(id))),
    }
    all.extend_from_slice(events);
    let mut messages = Vec::new();
    let (_, statuses) = ui.update(&all, cursor, &mut renderer, clipboard, &mut messages);
    (messages, statuses)
}

/// Every text a view holds: labels, and what each text field shows or has typed.
pub fn texts<Message>(view: Element<'_, Message>) -> Vec<String> {
    use iced::advanced::widget::{Id, Operation, operation::TextInput};
    struct Collect(Vec<String>);
    impl Operation for Collect {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
            self.0.push(text.to_string());
        }
        fn text_input(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn TextInput) {
            self.0.push(state.text().to_string());
        }
    }
    let mut renderer = renderer();
    let size = Size::new(900.0, 700.0);
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let mut collect = Collect(Vec::new());
    ui.operate(&renderer, &mut collect);
    collect.0
}

/// The bounds of every container of `view` (laid out in `size`) whose id is `id`.
pub fn containers<Message>(
    view: Element<'_, Message>,
    size: Size,
    id: &'static str,
) -> Vec<Rectangle> {
    use iced::advanced::widget::{Id, Operation};
    struct Find(Id, Vec<Rectangle>);
    impl Operation for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&self.0) {
                self.1.push(bounds);
            }
        }
    }
    let mut renderer = renderer();
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let mut find = Find(Id::new(id), Vec::new());
    ui.operate(&renderer, &mut find);
    find.1
}

/// The texts of `view` laid out in `size` that the user can't bring into
/// view: squeezed to nothing, or outside the window and not in a scroll area.
pub fn out_of_reach<Message>(view: Element<'_, Message>, size: Size) -> Vec<String> {
    use iced::advanced::widget::{Id, Operation, operation::Scrollable};
    struct Reach {
        window: Rectangle,
        /// One entry per traversal level: inside a scroll area there.
        scrolled: Vec<bool>,
        next_scrolls: bool,
        lost: Vec<String>,
    }
    impl Operation for Reach {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            let inside = std::mem::take(&mut self.next_scrolls)
                || self.scrolled.last().copied().unwrap_or(false);
            self.scrolled.push(inside);
            operate(self);
            self.scrolled.pop();
        }
        fn scrollable(
            &mut self,
            _: Option<&Id>,
            _: Rectangle,
            _: Rectangle,
            _: iced::Vector,
            _: &mut dyn Scrollable,
        ) {
            self.next_scrolls = true;
        }
        fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
            let scrolled = self.scrolled.last().copied().unwrap_or(false);
            // A column out of room squeezes its texts to nothing.
            let squeezed = bounds.height < 1.0 && !text.trim().is_empty();
            let shown = bounds.shrink(0.5).is_within(&self.window);
            if squeezed || !scrolled && !shown {
                self.lost.push(text.to_string());
            }
        }
    }
    let mut renderer = renderer();
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    let mut reach = Reach {
        window: Rectangle::with_size(size),
        scrolled: Vec::new(),
        next_scrolls: false,
        lost: Vec::new(),
    };
    ui.operate(&renderer, &mut reach);
    reach.lost
}

/// Render and, when `OPENCIRCLE_RENDER_DIR` is set, save `<dir>/<name>.png`.
/// Returns the pixels either way.
pub fn save<Message>(name: &str, view: Element<'_, Message>, size: Size) -> Vec<u8> {
    let (w, h) = (size.width as u32, size.height as u32);
    let rgba = render(view, w, h);
    if let Some(dir) = std::env::var_os("OPENCIRCLE_RENDER_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::actions::write_png(&dir.join(format!("{name}.png")), w, h, &rgba).unwrap();
    }
    rgba
}
