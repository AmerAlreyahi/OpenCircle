//! The bottom bar: size, dock, glide, keys and its view.

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::{container, opaque, operation, space};
use iced::{Element, Point, Rectangle, Size, Task};

use crate::actions::{self, lens, text};
use crate::config::{self};
use crate::labs::{self, BarButton};

use crate::ui::app::background::background;
use crate::ui::app::keys;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::edit::Editing;
use crate::ui::{bar, song, widgets};

use super::{ASK_INPUT, ASK_PLACEHOLDERS, App, LIVE, Reader};

impl App {
    /// The bottom bar shows in main mode from the first frame, through drawing
    /// and selecting; a result card, a translation, the song card or a toast
    /// take its place.
    pub(in crate::ui) fn bottom_bar_shown(&self) -> bool {
        matches!(self.o.purpose, Purpose::Search)
            && self.o.panel.is_none()
            && self.o.translate.is_none()
            && self.o.song.is_none()
            && self.o.toast.is_none()
    }

    /// The bottom bar's field width and size: the same in every state (no
    /// selection, a selection, after a card), so it never jumps.
    pub(in crate::ui) fn bar_size(&self) -> (f32, Size) {
        let icons = labs::bar(&self.o.labs)
            .iter()
            .filter(|b| !b.is_pill())
            .count();
        bar::bottom_size(self.o.screen.width, icons as f32)
    }

    /// The size of what is docked at the bar's spot now: the bar, the editing
    /// toolbar, or the shape between them while one turns into the other.
    pub(in crate::ui) fn dock_size(&self) -> Size {
        let (_, bar) = self.bar_size();
        let tool = Editing::toolbar_size(self.o.screen.width);
        let t = self.swapped();
        Size::new(
            bar.width + (tool.width - bar.width) * t,
            bar.height + (tool.height - bar.height) * t,
        )
    }

    /// Where the bar is drawn now (top-left): on its way to `bar_target`.
    pub(in crate::ui) fn bar_at(&self) -> Point {
        self.o.bar_now.unwrap_or_else(|| self.bar_target())
    }

    /// Where the bar (or the toolbar in its place) goes: following the pointer
    /// while held, else its spot, or the nearest free one while the selection
    /// (the editor's canvas) is where it would be.
    pub(in crate::ui) fn bar_target(&self) -> Point {
        if let Some((_, at)) = self.o.bar_drag {
            return at;
        }
        let size = self.dock_size();
        let canvas = self.o.edit.as_ref().map(Editing::canvas_rect);
        let blocked: Vec<Rectangle> = (self.o.selection.iter())
            .chain(self.o.others.iter().map(|(s, _)| s))
            .map(|s| self.o.view.rect_to_screen(s.area))
            .chain(canvas)
            .collect();
        let spot = bar::free(
            self.o.bar_anchor,
            &blocked,
            self.o.screen,
            size,
            self.o.work_inset,
        );
        bar::anchor_at(spot, self.o.screen, size, self.o.work_inset)
    }

    /// The bar is on its way somewhere.
    pub(in crate::ui) fn bar_gliding(&self) -> bool {
        self.o.frame.is_some() && self.o.bar_now.is_some_and(|at| at != self.bar_target())
    }

    /// The bar was dropped: its place is remembered (config `[bar]`).
    pub(in crate::ui) fn remember_bar(&self) -> Task<Message> {
        let anchor = self.o.bar_anchor;
        if cfg!(test) {
            let mut cfg = config::current();
            cfg.bar.anchor = anchor;
            config::set_current(cfg);
            return Task::none(); // never the user's file
        }
        background(move |_: &UnboundedSender<()>| {
            if let Err(e) = config::update(|c| c.bar.anchor = anchor) {
                crate::log!("[opencircle] the bar's place was not saved: {e}");
            }
        })
        .discard()
    }

    /// Something is selected for the bar's pills to act on.
    pub(in crate::ui) fn selected_for_bar(&self) -> bool {
        self.o.selection.is_some() && !self.settling()
    }

    /// What a bar button does (also what Enter / Space do on it once Tab
    /// focused it); none = dimmed: Search in browser and Copy text need a
    /// selection, Translate takes the whole screen without one.
    pub(in crate::ui) fn bar_message(&self, which: BarButton) -> Option<Message> {
        let selected = self.selected_for_bar();
        match which {
            BarButton::Search | BarButton::CopyText => selected.then_some(Message::Pill(which)),
            BarButton::Translate => Some(Message::Pill(which)),
            BarButton::Live => Some(Message::LiveToggle),
            BarButton::Song => Some(Message::SongStart),
            BarButton::Edit if selected => Some(Message::Run(&actions::edit::ACTION)),
            BarButton::Edit => Some(Message::EditScreen),
        }
    }

    /// Tab / Shift+Tab: the focus moves on through the bar (`bar::stops`).
    /// The AI field takes the text input's focus, any other stop lets go of it.
    pub(in crate::ui) fn tab_bar(&mut self, back: bool) -> Task<Message> {
        let stops = bar::stops(self.selected_for_bar(), &labs::bar(&self.o.labs));
        self.o.bar_focus = bar::tab(&stops, self.o.bar_focus, back);
        match self.o.bar_focus {
            Some(bar::Stop::Field) => operation::focus(ASK_INPUT),
            _ => operation::focus("overlay-none"), // focusing nothing unfocuses the field
        }
    }

    /// Enter / Space on the stop Tab focused.
    pub(in crate::ui) fn activate(&mut self, stop: bar::Stop) -> Task<Message> {
        match stop {
            bar::Stop::Logo => self.update(Message::AskButton),
            bar::Stop::Badge => self.update(Message::AskSubmit),
            bar::Stop::Button(which) => match self.bar_message(which) {
                Some(message) => self.update(message),
                None => Task::none(),
            },
            bar::Stop::Field => Task::none(), // the field's own Enter submits
        }
    }

    /// The bottom bar, the one action hub, as the new design's search bar: a
    /// handle to drag it, the AI field ("Ask OpenCircle AI about your
    /// screen…": Enter, the logo or the "AI ↵" badge ask about the selection,
    /// else the whole screen; empty, they open Ask AI), the pills Search in
    /// browser (highlighted until another is pressed), Copy text and
    /// Translate, then the round icon buttons. Search in browser and Copy text
    /// are dimmed until something is selected; Translate without a selection
    /// takes the whole screen. Always the same size (`bar_size`). Its field
    /// takes the keys only once clicked: until then letters are the action
    /// shortcuts (L, A, T still search, ask and translate).
    pub(in crate::ui) fn bottom_bar(&self) -> Element<'_, Message> {
        let (width, _) = self.bar_size();
        let input = widgets::input(
            ASK_INPUT,
            ask_placeholder(width),
            &self.o.ask,
            Message::AskInput,
            Message::AskSubmit,
        );
        let kbd = |stop| self.o.bar_focus == Some(stop);
        let field = widgets::ai_field(
            input,
            width,
            (Message::AskButton, kbd(bar::Stop::Logo)),
            (Message::AskSubmit, kbd(bar::Stop::Badge)),
        );
        let field = keys::guard(field, Message::Field);
        let keys = &self.o.bar_keys;
        let selected = self.selected_for_bar();
        let shown = labs::bar(&self.o.labs);
        let icons = shown.iter().filter(|b| !b.is_pill()).count() as f32;
        let widths = bar::pill_widths(self.o.screen.width, icons);
        let pills = [
            (
                BarButton::Search,
                lens::ACTION.icon,
                lens::ACTION.label,
                &keys.search,
            ),
            (
                BarButton::CopyText,
                text::ACTION.icon,
                "Copy text",
                &keys.copy_text,
            ),
            (
                BarButton::Translate,
                actions::translate::ACTION.icon,
                actions::translate::ACTION.label,
                &keys.translate,
            ),
        ]
        .into_iter()
        .zip(widths)
        .map(|((which, glyph, label, keys), width)| {
            let pill = widgets::bar_pill(
                glyph,
                label,
                width,
                self.o.pill == which,
                self.bar_message(which),
                kbd(bar::Stop::Button(which)),
            );
            // Its label is written on it; an icon-only pill (narrow screens)
            // names itself and its shortcut in a tooltip.
            if width > widgets::BUBBLE {
                return pill.into();
            }
            let why = match keys.is_empty() {
                true => widgets::Why::Icon,
                false => widgets::Why::Keys(keys),
            };
            widgets::hint(pill, label, why, iced::widget::tooltip::Position::Top)
        });
        let icons = [
            (
                BarButton::Live,
                LIVE,
                "Live translate (Beta)",
                &keys.live,
                self.live.is_some(),
            ),
            (
                BarButton::Song,
                song::MUSIC,
                "Song search",
                &keys.song,
                false,
            ),
            (
                BarButton::Edit,
                actions::edit::ACTION.icon,
                if selected {
                    "Edit image"
                } else {
                    "Edit the screen"
                },
                &keys.edit,
                false,
            ),
        ]
        .into_iter()
        .filter(|(which, ..)| shown.contains(which))
        .map(|(which, glyph, label, keys, active)| {
            let message = self.bar_message(which);
            bar_button(
                glyph,
                label,
                keys,
                active,
                message,
                kbd(bar::Stop::Button(which)),
            )
        });
        let handle = container(widgets::icon(widgets::DRAG))
            .center_x(bar::HANDLE)
            .center_y(widgets::CONTROL);
        let mut row = iced::widget::row![handle, field, widgets::toolbar_divider()]
            .extend(pills)
            .spacing(bar::SPACING)
            .align_y(iced::Center);
        let icons: Vec<_> = icons.collect();
        if !icons.is_empty() {
            row = row.push(widgets::toolbar_divider()).extend(icons);
        }
        // A press on the handle or the padding (not taken by the field or a
        // button) drags the bar; opaque: a press on it never starts a selection.
        let bar = iced::widget::mouse_area(widgets::surface(row))
            .on_press(Message::BarGrab)
            .interaction(iced::mouse::Interaction::Grab);
        opaque(bar)
    }

    /// The note on the text reader (`Reader`): its download offered with its
    /// size, its progress, a failure to try again, or "Reading text…".
    pub(in crate::ui) fn reader_note(&self) -> Option<Element<'_, Message>> {
        // what it says, then its button or progress bar
        let (say, then): (Element<'_, Message>, Element<'_, Message>) = match &self.o.reader {
            Reader::Idle => return None,
            Reader::Missing(bytes) => {
                let mb = bytes.div_ceil(1_000_000).max(1);
                let say = format!("Text selection needs the text reader (~{mb} MB).");
                let download = widgets::chip("Download", Message::ReaderDownload);
                (widgets::body(say).into(), download.into())
            }
            Reader::Downloading(done) => {
                let percent = (done * 100.0).round();
                let say = widgets::body(format!("Downloading the text reader… {percent}%"));
                (
                    say.into(),
                    container(widgets::progress(*done)).width(120).into(),
                )
            }
            Reader::Failed(e) => {
                let say = widgets::error(format!("The text reader didn't download: {e}"));
                let again = widgets::chip("Try again", Message::ReaderDownload);
                (say.into(), again.into())
            }
        };
        let row = iced::widget::row![say, then]
            .spacing(12)
            .padding([0, 10])
            .height(widgets::CONTROL)
            .align_y(iced::Center);
        Some(widgets::surface(row).into())
    }

    /// `note` centred on the bar at `at`: above it, or below it when the bar
    /// is at the top; opaque, so a press on it never starts a selection.
    pub(in crate::ui) fn beside_bar<'a>(
        &self,
        note: Element<'a, Message>,
        at: Point,
    ) -> Element<'a, Message> {
        let (_, size) = self.bar_size();
        let height = widgets::CONTROL + 2.0 * widgets::SURFACE_PADDING;
        let (y, align) = match at.y > self.o.screen.height / 2.0 {
            true => (at.y - bar::SPACING - height, iced::Bottom),
            false => (at.y + size.height + bar::SPACING, iced::Top),
        };
        let note = container(opaque(note))
            .center_x(size.width)
            .height(height)
            .align_y(align);
        iced::widget::pin(note).position(Point::new(at.x, y)).into()
    }

    /// The bar turning into the editing toolbar (or back) at `at`: its
    /// surface only, sized between the two (`dock_size`); opaque, so a press
    /// on it never starts a selection.
    // ponytail: a plain size morph; the full motion polish is v0.1.1.
    pub(in crate::ui) fn morphing_bar(&self, at: Point) -> Element<'_, Message> {
        let size = self.dock_size();
        // (the surface pads its content: the content takes the rest)
        let inner = size - Size::new(2.0, 2.0) * widgets::SURFACE_PADDING;
        let shape = widgets::surface(space().width(inner.width).height(inner.height));
        iced::widget::pin(opaque(shape)).position(at).into()
    }
}

/// The placeholder that fits a field `width` wide, in the UI font.
pub(in crate::ui) fn ask_placeholder(width: f32) -> &'static str {
    let room = width - widgets::AI_FIELD_LEFT - widgets::AI_FIELD_RIGHT;
    widgets::fitting(&ASK_PLACEHOLDERS, 14.0, room)
}

/// A round button of the bottom bar, named `label` (its tooltip adds `keys`);
/// `message: None` = dimmed (nothing to act on yet).
pub(in crate::ui) fn bar_button<'a>(
    glyph: char,
    label: &'a str,
    keys: &'a str,
    active: bool,
    message: Option<Message>,
    kbd: bool,
) -> Element<'a, Message> {
    let why = match keys.is_empty() {
        true => widgets::Why::Icon,
        false => widgets::Why::Keys(keys),
    };
    let button = widgets::round_button(glyph, active, message, kbd);
    widgets::hint(button, label, why, iced::widget::tooltip::Position::Top)
}
