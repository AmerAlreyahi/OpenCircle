//! The editor on a capture: opening it, its messages, the bar swap.

use iced::widget::image;
use iced::{Task, Vector};

use crate::actions::{self, Crop, copy, save};
use crate::editor::{self, Image, Output};
use crate::labs::{self};

use crate::ui::app::message::{Message, Purpose};
use crate::ui::kit::motion;
use crate::ui::screens::editor::Editing;
use crate::ui::snip;

use super::capture::TEST_SHOT;
use super::{App, work_area};

impl App {
    /// Edit in place: the selection's pixels become the editor's canvas, at their
    /// place on screen; the toolbar takes the bar's place. Edits made before
    /// (then Back) are still there.
    pub(in crate::ui) fn open_editor(&mut self) -> Task<Message> {
        if !self.o.labs.has(labs::Feature::Editor) {
            return Task::none();
        }
        self.o.panel = None;
        self.o.view = self.o.view.reset(); // the editor works at 1x
        self.o.zoom = None;
        if let Some(edit) = &mut self.o.edit {
            edit.reopen();
            return Task::none();
        }
        let (
            Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }),
            Some(selection),
        ) = (&self.o.frame, self.o.selection)
        else {
            return Task::none();
        };
        let bounds = actions::crop_bounds(*width, *height, self.o.scale, selection.area);
        let crop = actions::crop(pixels, *width, *height, self.o.scale, selection.area);
        if let (Some(bounds), Some(crop)) = (bounds, crop) {
            // Snip: the area stays live on the whole frozen monitor (N-73).
            let joined = match &self.o.purpose {
                Purpose::Snip(_) => self.desktop_live(selection.area),
                _ => None,
            };
            let mut edit = match &self.o.purpose {
                // across screens: the document is every frame joined
                Purpose::Snip(_) if let Some(edit) = joined => edit,
                Purpose::Snip(_) => {
                    let frame = Image {
                        width: *width,
                        height: *height,
                        rgba: pixels.to_vec(),
                    };
                    Editing::live(frame, bounds, self.o.scale, iced::Point::ORIGIN)
                }
                _ => Editing::in_place(crop, bounds, self.o.scale),
            };
            // Scrolling capture needs the page under the area: Snip, in place.
            edit.editor.scroll = matches!(&self.o.purpose, Purpose::Snip(s) if s.can_scroll());
            if edit.editor.movable {
                (self.o.bar_now, self.o.bar_glide) = (None, None); // it rises from the area
            }
            self.o.edit = Some(edit);
        }
        Task::none()
    }

    pub(in crate::ui) fn on_editor(&mut self, msg: editor::Message) -> Task<Message> {
        // Test hook: the editor's Esc (Back) takes the shot of the editor instead.
        if matches!(msg, editor::Message::Back) && std::env::var_os(TEST_SHOT).is_some() {
            return self.test_shot();
        }
        // Snip's live area: the frame times of each drag on it (the log).
        let live = self.o.edit.as_ref().is_some_and(|e| e.editor.movable);
        match &msg {
            editor::Message::Press(_) if live => self.frame_times.borrow_mut().clear(),
            editor::Message::Release if live => self.log_frames("snip area drag"),
            _ => {}
        }
        // (a press on another screen's part gave its window the focus: back)
        if matches!(msg, editor::Message::Release) && !self.o.screens.is_empty() {
            let focus = iced::window::gain_focus(self.overlay);
            return self.on_editor_here(msg).chain(focus);
        }
        self.on_editor_here(msg)
    }

    fn on_editor_here(&mut self, msg: editor::Message) -> Task<Message> {
        let o = &mut *self.o;
        let Some(edit) = &mut o.edit else {
            return Task::none();
        };
        let out = edit.update(msg);
        // Snip's live area moved or resized: the selection (the overlay's dim,
        // scrolling capture, pin) follows it.
        let live = edit.editor.movable.then(|| edit.canvas_rect());
        // Add capture: the area picked on the frozen frame joins the board.
        if let Some(area) = edit.take_picked()
            && let Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }) = &o.frame
            && let Some(crop) = actions::crop(pixels, *width, *height, o.scale, area)
        {
            edit.add(crop, work_area(o.screen, o.work_inset));
        }
        if let Some(area) = live.filter(|a| self.o.selection.is_none_or(|s| s.area != *a)) {
            self.select(area);
        }
        let o = &mut *self.o;
        let Some(edit) = &mut o.edit else {
            return Task::none();
        };
        match out {
            None => Task::none(),
            // Snip has no action bar: back to picking an area.
            Some(Output::Back) if matches!(o.purpose, Purpose::Snip(_)) => {
                self.o.edit = None;
                self.o.selection = None;
                self.o.stroke.clear();
                Task::none()
            }
            Some(Output::Back) => {
                edit.back();
                self.o.esc_back = true;
                Task::none()
            }
            Some(Output::Copy(crop)) => self.finish(&copy::ACTION, crop),
            Some(Output::Save(crop)) => self.finish(&save::ACTION, crop),
            Some(Output::Pin(_)) if !self.o.labs.has(labs::Feature::Pins) => Task::none(),
            Some(Output::Pin(crop)) => self.pin(crop),
            // Scrolling capture of the area under the editor.
            Some(Output::Scroll) => match self.o.selection {
                Some(selection) => {
                    self.o.edit = None;
                    self.start_scroll(selection.area)
                }
                None => Task::none(),
            },
            Some(Output::AddCapture) => Task::none(), // `edit` picks the area now
            // The share sheet is up: once a target is picked, the editor closes.
            Some(Output::Sharing(chosen)) => Task::future(chosen).then(|picked| match picked {
                Ok(()) => Task::done(Message::Editor(editor::Message::Shared)),
                Err(_) => Task::none(), // the sheet went without a target
            }),
            Some(Output::Dragged | Output::Close) => self.close(),
        }
    }

    /// Pin: the overlay closes and the image floats where the selection was.
    pub(in crate::ui) fn pin(&mut self, crop: Crop) -> Task<Message> {
        let at = (self.o.selection).map(|s| self.o.origin + Vector::new(s.area.x, s.area.y));
        let pinned = self
            .windows
            .pin(crop, at, self.o.scale)
            .map(Message::Windows);
        Task::batch([self.close(), pinned])
    }

    /// The running scrolling capture.
    pub(in crate::ui) fn scroll(&self) -> Option<&snip::Scroll> {
        match &self.o.purpose {
            Purpose::Snip(snip) => snip.scroll.as_ref(),
            _ => None,
        }
    }

    /// The editing toolbar is up (not back at the bar).
    pub(in crate::ui) fn editing(&self) -> bool {
        self.o.edit.as_ref().is_some_and(Editing::is_open)
    }

    /// The bar is the toolbar (1) while editing, else the bar (0).
    pub(in crate::ui) fn swap_target(&self) -> f32 {
        if self.editing() { 1.0 } else { 0.0 }
    }

    /// The bar is turning into the toolbar, or back.
    pub(in crate::ui) fn swapping(&self) -> bool {
        self.o.frame.is_some() && self.o.swap != self.swap_target()
    }

    /// How far the bar has turned into the toolbar (0..1, eased); Snip, which
    /// has no bar, is always there.
    pub(in crate::ui) fn swapped(&self) -> f32 {
        match self.o.purpose {
            // back to the bar: the same curve run backwards, an ease-in
            Purpose::Search => motion::Curve::Out.ease(self.o.swap),
            _ => self.swap_target(),
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Rectangle, Size};

    use crate::actions::share;
    use crate::config::SnipAfter;
    use crate::editor::Message as E;
    use crate::labs::Labs;
    use crate::ui::app::App;
    use crate::ui::app::message::Hiding;
    use crate::ui::app::tests::{open_app, run_task, snipping_on};
    use crate::ui::screens::editor::toolbar;
    use crate::ui::snip;

    /// A snip of an area in its editor (Labs all on for the host; the toolbar
    /// reads the shared config, where Pins is off).
    fn snip_editor() -> App {
        let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
        let mut app = snipping_on(800, 600, area);
        app.o.labs = Labs::all_on();
        app.select(area);
        let crop = app.crop().unwrap();
        let _ = app.snip_routed(crop, true, snip::route(SnipAfter::Editor));
        assert!(app.editing(), "the editor is open");
        app
    }

    /// Circle to Search's Snip button: the editor in place.
    fn in_place_editor() -> App {
        let mut app = open_app(1280, 800, 1.0);
        app.select(Rectangle::new(
            Point::new(200.0, 150.0),
            Size::new(300.0, 200.0),
        ));
        let _ = app.open_editor();
        app.o.swap = 1.0; // the bar has turned into the toolbar
        assert!(app.editing(), "the editor is open");
        app
    }

    fn stops(app: &App) -> Vec<E> {
        toolbar::stops(&app.o.edit.as_ref().unwrap().editor)
    }

    /// The tooltips of what the overlay draws now (its buttons' names).
    fn bar_names(app: &App) -> Vec<String> {
        use crate::ui::kit::tooltip::{SHOWN, tests_support::shown};
        SHOWN.with(|s| s.borrow_mut().clear());
        let _ = crate::ui::kit::testing::shot::texts(app.view(app.overlay));
        shown()
    }

    /// The overlay is closing and draws no toolbar any more.
    fn gone(app: &App) -> bool {
        app.hiding != Hiding::No
            && app.o.edit.is_none()
            && !bar_names(app).iter().any(|n| n.starts_with("Copy"))
    }

    /// N-62: Share hands the capture to the sheet as a PNG; a target picked
    /// closes the snip (the file stays for the target's grace); no target
    /// leaves the editor as it was, and its file goes with the overlay.
    #[test]
    fn share_gives_the_sheet_the_capture_and_a_picked_target_closes_the_snip() {
        let mut app = snip_editor();
        let task = app.on_editor(E::Share);
        let (path, picked) = share::TEST_SHEET.take().expect("the sheet has it");
        assert!(
            path.exists() && app.editing(),
            "the sheet is up over the snip"
        );
        let _ = picked.send(());
        run_task(&mut app, task);
        assert!(gone(&app), "a target took it: closed like a drop");
        assert!(path.exists(), "the target may still read it");
        let _ = std::fs::remove_file(&path);

        // the sheet went without a target: the editor stays, the file goes with it
        let mut app = snip_editor();
        let task = app.on_editor(E::Share);
        let (path, picked) = share::TEST_SHEET.take().unwrap();
        drop(picked);
        run_task(&mut app, task);
        assert!(app.editing() && path.exists());
        let _ = app.close();
        assert!(!path.exists(), "deleted when the overlay closed");
    }

    /// Share is on Snip's toolbar only where there is a share sheet (Hide,
    /// don't delete: Linux has none yet).
    #[test]
    fn share_shows_only_where_the_system_has_a_sheet() {
        let app = snip_editor();
        let shown = bar_names(&app).iter().any(|n| n.starts_with("Share"));
        assert_eq!(shown, share::SUPPORTED);
        assert_eq!(stops(&app).contains(&E::Share), share::SUPPORTED);
    }

    /// With Labs Pins off the toolbar has no Pin button at all (not a dead
    /// one), in Snip and in place, and Tab never lands on it.
    #[test]
    fn no_pin_button_while_labs_pins_is_off() {
        assert!(!crate::labs::has(crate::labs::Feature::Pins), "the default");
        let mut app = snip_editor();
        app.o.labs.set(crate::labs::Feature::Pins, false);
        let names = bar_names(&app);
        assert!(names.iter().any(|n| n.starts_with("Copy")), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("Pin")), "{names:?}");
        assert!(!stops(&app).contains(&E::Pin));
        let app = in_place_editor();
        let names = bar_names(&app);
        assert!(names.iter().any(|n| n.starts_with("Undo")), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("Pin")), "{names:?}");
    }
}
