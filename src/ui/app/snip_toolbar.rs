//! The Snip toolbar on a capture: opening it, its messages, the bar swap.

use iced::widget::image;
use iced::{Point, Rectangle, Task};

use crate::actions::{self, Crop, copy, save};
use crate::snip::annotate::{self, Image, Output};

use crate::shared::kit::motion;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::screens::snip_toolbar::SnipToolbar;

use super::App;
use super::capture::TEST_SHOT;
use super::screens::to_desktop;

impl App {
    /// Edit in place: the selection's pixels become the editor's canvas, at their
    /// place on screen; the toolbar takes the bar's place. Edits made before
    /// (then Back) are still there.
    pub(crate) fn open_snip_toolbar(&mut self) -> Task<Message> {
        self.o.ai_chat = None;
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
        // Snip: the area stays live on the whole frozen monitor (N-73). With
        // several screens the document is every frame joined, so an area wholly
        // on another screen counts too, not only one meeting the starting one's
        // frame (owner bug 2026-11-03: a pick there opened nothing).
        let joined = match &self.o.purpose {
            Purpose::Snip(_) => self.desktop_live(selection.area),
            _ => None,
        };
        let edit = joined.or_else(|| {
            let bounds = actions::crop_bounds(*width, *height, self.o.scale, selection.area)?;
            let frame = Image {
                width: *width,
                height: *height,
                rgba: pixels.to_vec(),
            };
            Some(SnipToolbar::live(
                frame,
                bounds,
                self.o.scale,
                iced::Point::ORIGIN,
            ))
        });
        let Some(mut edit) = edit else {
            return Task::none();
        };
        // Scrolling capture needs the page under the area: Snip, in place.
        edit.annotator.scroll = matches!(&self.o.purpose, Purpose::Snip(s) if s.can_scroll());
        if edit.annotator.movable {
            (self.o.bar_now, self.o.bar_glide, self.o.bar_left) = (None, None, None); // it rises from the area
        }
        edit.annotator.steps = self.windows.steps();
        if let Purpose::Snip(s) = &self.o.purpose {
            // window edges for the picked area to snap to, in the editor's image px
            let place = self.o.place();
            let origin = match self.desk_bounds(self.o.desk()) {
                Some([x0, y0, ..]) if !self.o.screens.is_empty() => (x0 as f32, y0 as f32),
                _ => (place.origin.0 as f32, place.origin.1 as f32),
            };
            edit.annotator.edges = (s.edge_rects().into_iter())
                .map(|r| {
                    let at = to_desktop(place, r.position());
                    Rectangle::new(
                        Point::new(at.x - origin.0, at.y - origin.1),
                        iced::Size::new(r.width * place.scale, r.height * place.scale),
                    )
                })
                .collect();
        }
        self.more_reset();
        self.o.edit = Some(edit);
        Task::none()
    }

    pub(crate) fn on_snip_toolbar(&mut self, msg: annotate::Message) -> Task<Message> {
        // Test hook: the editor's Esc (Back) takes the shot of the editor instead.
        if matches!(msg, annotate::Message::Back)
            && crate::shared::platform::sys::test_hook(TEST_SHOT).is_some()
        {
            return self.test_shot();
        }
        // The toolbar's grip (its own press, so the toolbar's `grab` under it
        // never hears it): drags the toolbar as the bar's grip does.
        if matches!(msg, annotate::Message::Grab) {
            return self.update(Message::BarGrab);
        }
        // Esc while the toolbar is dragged: it glides back to where it was.
        if matches!(msg, annotate::Message::Back)
            && let Some(was) = self.o.bar_was.take()
            && self.o.bar_drag.take().is_some()
        {
            self.o.bar_left = was;
            self.o.bar_eased = None;
            if let Some(edit) = &mut self.o.edit {
                edit.annotator.held = false;
            }
            return Task::none();
        }
        // The AI chat asked from More is a quick question: Esc closes both.
        if matches!(msg, annotate::Message::Back) && self.o.ai_chat.is_some() {
            return self.close();
        }
        // Snip's live area: the frame times of each drag on it (the log).
        let live = self.o.edit.as_ref().is_some_and(|e| e.annotator.movable);
        match &msg {
            annotate::Message::Press(_) if live => self.frame_times.borrow_mut().clear(),
            annotate::Message::Release if live => self.log_frames("snip area drag"),
            _ => {}
        }
        // (a press on another screen's part gave its window the focus: back)
        if matches!(msg, annotate::Message::Release) && !self.o.screens.is_empty() {
            let focus = iced::window::gain_focus(self.overlay);
            return self.on_snip_toolbar_here(msg).chain(focus);
        }
        self.on_snip_toolbar_here(msg)
    }

    fn on_snip_toolbar_here(&mut self, msg: annotate::Message) -> Task<Message> {
        // A long capture running: the toolbar's finishing buttons end it, on
        // the stitched image; nothing else reaches the editor meanwhile.
        if self.scroll().is_some() {
            use crate::ui::screens::long_capture::Finish;
            // More stays usable meanwhile (Copy text and Ask AI finish the
            // capture, Pin and Share as their buttons do; the rest is dimmed)
            let edit = self.o.edit.as_mut();
            match (msg.clone(), edit) {
                (
                    annotate::Message::Menu(annotate::Menu::More)
                    | annotate::Message::MenuStep(_)
                    | annotate::Message::MenuEdge(_),
                    Some(edit),
                ) => {
                    let _ = edit.update(msg);
                    return Task::none();
                }
                (annotate::Message::Activate, Some(edit)) => {
                    return match edit.take_row() {
                        Some(row) => self.on_snip_toolbar_here(row),
                        None => Task::none(),
                    };
                }
                _ => {}
            }
            let how = match msg {
                annotate::Message::Copy => Finish::Copy,
                annotate::Message::Save => Finish::Save,
                annotate::Message::Pin => Finish::Pin,
                annotate::Message::Share => Finish::Share,
                annotate::Message::CopyText => Finish::CopyText,
                annotate::Message::AskAi => Finish::AskAi,
                _ => return Task::none(),
            };
            return self.on_snip(crate::ui::snip::Msg::Finish(how));
        }
        let o = &mut *self.o;
        let Some(edit) = &mut o.edit else {
            return Task::none();
        };
        let opened_more = matches!(msg, annotate::Message::Menu(annotate::Menu::More));
        let out = edit.update(msg);
        let opened_more = opened_more && edit.annotator.menu == Some(annotate::Menu::More);
        // Snip's live area moved or resized: the selection (the overlay's dim,
        // scrolling capture, pin) follows it.
        let live = edit.annotator.movable.then(|| edit.canvas_rect());
        if let Some(area) = live.filter(|a| self.o.selection.is_none_or(|s| s.area != *a)) {
            self.select(area);
        }
        let o = &mut *self.o;
        let Some(edit) = &mut o.edit else {
            return Task::none();
        };
        match out {
            None if opened_more => self.more_opened(),
            None => Task::none(),
            // a step of the Steps strip edited: Esc leaves it as it was
            Some(Output::Back) if edit.annotator.updating.is_some() => self.close(),
            // Snip has no action bar: back to picking an area.
            Some(Output::Back) if matches!(o.purpose, Purpose::Snip(_)) => {
                self.more_reset();
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
            Some(Output::CopyText) => self.more_copy_text(),
            Some(Output::AskAi(crop)) => self.open_chat(crop, None),
            Some(Output::Translate) => self.more_translate(),
            Some(Output::Copy(crop)) => self.finish(&copy::ACTION, crop),
            Some(Output::Save(crop)) => self.finish(&save::ACTION, crop),
            Some(Output::Pin(crop)) => self.pin(crop),
            Some(Output::LivePin) => match self.o.selection {
                Some(selection) => self.live_pin(selection.area),
                None => Task::none(),
            },
            // Scrolling capture of the area under the editor (its toolbar stays).
            Some(Output::Scroll) => match self.o.selection {
                Some(selection) => self.start_scroll(selection.area),
                None => Task::none(),
            },
            Some(Output::AddStep(crop)) => {
                let update = edit.annotator.updating;
                self.add_step(crop, update)
            }
            // the colour picker, the ruler: Esc there comes back to this
            Some(Output::ScreenTool(tool)) => self.start_tool(tool),
            // The share sheet is up: once a target is picked, the editor closes.
            Some(Output::Sharing(chosen)) => Task::future(chosen).then(|picked| match picked {
                Ok(()) => Task::done(Message::Annotate(annotate::Message::Shared)),
                Err(_) => Task::none(), // the sheet went without a target
            }),
            Some(Output::Dragged) => self.close(),
        }
    }

    /// Pin: the overlay closes and the image floats where the selection was,
    /// on whichever screen holds the larger part of it (at that screen's scale).
    pub(crate) fn pin(&mut self, crop: Crop) -> Task<Message> {
        let place = (self.o.selection).and_then(|s| self.pin_place(s.area));
        let at = place.map(|(_, (x, y, ..))| (x, y));
        let scale = place.map_or(self.o.scale, |(on, _)| on.place.scale);
        let pinned = self.windows.pin(crop, at, scale).map(Message::Windows);
        Task::batch([self.close(), pinned])
    }

    /// Add to steps / Add step N / Update step N: the overlay closes and the
    /// capture goes to the Steps strip (opened on this monitor by its first
    /// step) as its next step, or in place of the `update`th.
    pub(crate) fn add_step(&mut self, crop: Crop, update: Option<usize>) -> Task<Message> {
        let (place, inset) = (self.place(), self.o.work_inset);
        let added = self.windows.add_step(crop, update, place, inset);
        Task::batch([self.close(), added.map(Message::Windows)])
    }

    /// A double-click on a step: Snip opens (freezing the screen) with that
    /// step picked, its toolbar saying "Update step N" (`capture::open`).
    pub(crate) fn edit_step(&mut self, i: usize) -> Task<Message> {
        let Some(crop) = self.windows.step_crop(i) else {
            return Task::none();
        };
        self.picked_next = Some((Some(i), crop));
        self.next = Purpose::Search;
        self.capture(
            std::time::Instant::now(),
            Some(crate::ui::snip::Kind::Region),
        )
    }

    /// Live pin (Snip, Labs Pins): the overlay closes and `area`'s part of the
    /// window under it floats where it was, kept updating (`Windows::live_pin`).
    /// Any screen: its part on the screen holding the larger part of it, from
    /// that screen's own frozen pixels, at that screen's scale.
    pub(crate) fn live_pin(&mut self, area: iced::Rectangle) -> Task<Message> {
        let window = match &self.o.purpose {
            Purpose::Snip(snip) => snip.window_under(area),
            _ => None,
        };
        let place = self.pin_place(area);
        let part = place.and_then(|(on, _)| area.intersection(&self.monitor_of(&on)));
        let first = part.and_then(|part| self.crop_of(part));
        let (Some(window), Some(first), Some((on, region))) = (window, first, place) else {
            return self.close();
        };
        let at = Some((region.0, region.1));
        let pinned = (self.windows)
            .live_pin(window, region, first, at, on.place.scale)
            .map(Message::Windows);
        Task::batch([self.close(), pinned])
    }

    /// Where a pin of `area` (image px) goes: the screen holding the larger
    /// part of it and that part in physical desktop px.
    pub(crate) fn pin_place(
        &self,
        area: iced::Rectangle,
    ) -> Option<(
        super::screens::On,
        crate::shared::platform::capture::window_rect::WinRect,
    )> {
        let on = self.on_area(area);
        self.region_on(on, area).map(|(_, region)| (on, region))
    }

    /// The running long capture.
    pub(crate) fn scroll(&self) -> Option<&crate::ui::screens::long_capture::Long> {
        match &self.o.purpose {
            Purpose::Snip(snip) => snip.scroll.as_ref(),
            _ => None,
        }
    }

    /// The editing toolbar is up (not back at the bar).
    pub(crate) fn editing(&self) -> bool {
        self.o.edit.as_ref().is_some_and(SnipToolbar::is_open)
    }

    /// The bar is the toolbar (1) while editing, else the bar (0).
    pub(crate) fn swap_target(&self) -> f32 {
        if self.editing() { 1.0 } else { 0.0 }
    }

    /// The bar is turning into the toolbar, or back.
    pub(crate) fn swapping(&self) -> bool {
        self.o.frame.is_some() && self.o.swap != self.swap_target()
    }

    /// How far the bar has turned into the toolbar (0..1, eased); Snip, which
    /// has no bar, is always there.
    pub(crate) fn swapped(&self) -> f32 {
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
    use crate::labs::Labs;
    use crate::snip::annotate::{Message as E, dragout};
    use crate::ui::app::App;
    use crate::ui::app::message::Hiding;
    use crate::ui::app::tests::{run_task, snipping_on};
    use crate::ui::screens::snip_toolbar::toolbar;
    use crate::ui::windows::Windows;

    /// Add to steps (More, owner 2026-11-02): the overlay closes and the
    /// capture is the strip's step 1; the next snip's primary is "Add step
    /// 2" (Copy moves into More) and adds it; at 12 it says "Steps full" and
    /// adds nothing. A step edited: "Update step N" replaces it, Esc leaves.
    #[test]
    fn add_to_steps_then_each_next_snip_adds_the_next_step() {
        let mut app = snip_toolbar_open();
        assert!(more(&app).contains(&Some(E::AddStep)), "More: Add to steps");
        let task = app.on_snip_toolbar(E::AddStep);
        run_task(&mut app, task);
        assert!(app.hiding != Hiding::No, "the overlay closes");
        assert_eq!(app.windows.steps(), Some(1));
        let mut windows = std::mem::take(&mut app.windows);
        for n in 1..crate::snip::annotate::STEPS_MAX {
            let mut app = snip_toolbar_open_with(windows);
            let editor = &app.o.edit.as_ref().unwrap().annotator;
            assert_eq!(editor.steps, Some(n));
            assert!(toolbar::stops(editor).contains(&E::AddStep), "Enter adds");
            assert!(more(&app).contains(&Some(E::Copy)), "Copy moved to More");
            assert!(
                bar_names(&app)
                    .iter()
                    .any(|t| t.starts_with("Add as the next step"))
            );
            let task = app.on_snip_toolbar(E::AddStep);
            run_task(&mut app, task);
            assert_eq!(app.windows.steps(), Some(n + 1));
            windows = std::mem::take(&mut app.windows);
        }
        // full: dimmed, nothing added, the overlay stays
        let mut app = snip_toolbar_open_with(windows);
        // Copy all: ONE image of the 12 (300 x 200) stacked and numbered
        use crate::ui::app::message::Message;
        use crate::ui::screens::steps::Msg as Steps;
        let all = Message::Windows(crate::ui::windows::Msg::Steps(Steps::CopyAll));
        let task = app.update(all);
        run_task(&mut app, task);
        let copied = crate::actions::copy::copied::image();
        assert_eq!(copied, Some((440, 40 + 12 * 200 + 11 * 32 + 40)));
        assert!(
            bar_names(&app)
                .iter()
                .any(|t| t.starts_with("Steps full (12 of 12)"))
        );
        let task = app.on_snip_toolbar(E::AddStep);
        run_task(&mut app, task);
        assert!(app.editing() && app.hiding == Hiding::No);
        // a step edited: Update step 1 replaces it (still 12)
        app.o.edit.as_mut().unwrap().annotator.updating = Some(0);
        assert!(
            bar_names(&app)
                .iter()
                .any(|t| t.starts_with("Update the step"))
        );
        let task = app.on_snip_toolbar(E::AddStep);
        run_task(&mut app, task);
        assert!(app.hiding != Hiding::No);
        assert_eq!(app.windows.steps(), Some(12));
        // Esc on an edited step leaves it as it was
        let mut app = snip_toolbar_open_with(std::mem::take(&mut app.windows));
        app.o.edit.as_mut().unwrap().annotator.updating = Some(0);
        let _ = app.on_snip_toolbar(E::Back);
        assert!(app.hiding != Hiding::No, "closed, nothing changed");
    }

    /// A snip of an area in its editor (Labs all on for the host; the toolbar
    /// reads the shared config, where Pins is off).
    fn snip_toolbar_open() -> App {
        snip_toolbar_open_with(Windows::default())
    }

    /// `snip_editor` with these windows (a Steps strip from an earlier snip).
    fn snip_toolbar_open_with(windows: Windows) -> App {
        let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
        let mut app = snipping_on(800, 600, area);
        app.windows = windows;
        app.o.labs = Labs::all_on();
        app.select(area);
        let crop = app.crop().unwrap();
        let _ = app.snip_routed(crop, true, false);
        assert!(app.editing(), "the editor is open");
        app
    }

    /// A keyboard nudge after a pause: the toolbar's glide logs only its own
    /// frames, not the idle time before it (the VM's "bar glide ... max
    /// 2001 ms apart" was the wait between two nudges); docked by OPEN.
    #[test]
    fn the_toolbar_glide_after_a_nudge_times_only_its_own_frames() {
        use crate::shared::kit::motion;
        use crate::ui::app::message::Message;
        use crate::ui::app::tests::settle_bar;
        use std::time::{Duration, Instant};
        let mut app = snip_toolbar_open();
        settle_bar(&mut app);
        let start = Instant::now();
        // a frame drawn 2 s before the nudge (the last glide's)
        app.frame_times
            .borrow_mut()
            .push(start - Duration::from_secs(2));
        let _ = app.on_snip_toolbar(E::Nudge((0, 1), (false, true, false)));
        let to = app.bar_target();
        assert_ne!(app.bar_at(), to, "the area went 10 px down");
        let _ = app.update(Message::Frame(start));
        assert!(app.o.bar_glide.is_some(), "it glides to the moved area");
        assert!(app.frame_times.borrow().is_empty(), "nothing from before");
        let _ = app.update(Message::Frame(start + motion::OPEN));
        assert_eq!(app.bar_at(), to, "there by OPEN");
    }

    /// What More offers now (each row's message).
    fn more(app: &App) -> Vec<Option<E>> {
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        toolbar::more_rows(editor)
            .into_iter()
            .map(|r| r.message)
            .collect()
    }

    /// The tooltips of what the overlay draws now (its buttons' names).
    fn bar_names(app: &App) -> Vec<String> {
        use crate::shared::kit::tooltip::{SHOWN, tests_support::shown};
        SHOWN.with(|s| s.borrow_mut().clear());
        let _ = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
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
        let mut app = snip_toolbar_open();
        let task = app.on_snip_toolbar(E::Share);
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
        let mut app = snip_toolbar_open();
        let task = app.on_snip_toolbar(E::Share);
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
        let mut app = snip_toolbar_open();
        let _ = app.on_snip_toolbar(E::Menu(crate::snip::annotate::Menu::More));
        let texts = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
        assert_eq!(texts.iter().any(|n| n == "Share"), share::SUPPORTED);
        assert_eq!(more(&app).contains(&Some(E::Share)), share::SUPPORTED);
    }

    /// Still pins left Labs (owner, 2026-10-31): with Labs Pins off the
    /// toolbar has Pin (and Tab lands on it) but no Live pin.
    #[test]
    fn pin_shows_with_labs_pins_off_live_pin_does_not() {
        assert!(!crate::labs::has(crate::labs::Feature::Pins), "the default");
        let mut app = snip_toolbar_open();
        app.o.labs.set(crate::labs::Feature::Pins, false);
        let names = bar_names(&app);
        assert!(names.iter().any(|n| n.starts_with("Copy")), "{names:?}");
        // More's rows
        let _ = app.on_snip_toolbar(E::Menu(crate::snip::annotate::Menu::More));
        let names = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
        assert!(
            names.iter().any(|n| n.starts_with("Pin to screen")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.starts_with("Long capture")),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("Live pin")),
            "{names:?}"
        );
        assert!(more(&app).contains(&Some(E::Pin)));
        // a pin with every Labs switch off
        let _ = app.on_snip_toolbar(E::Pin);
        assert!(app.hiding != Hiding::No, "pinned: the overlay closes");
    }

    /// More (owner, 2026-11-02): Long capture, Add to steps, Pin, Share (where
    /// there is a sheet), the screen tools. The arrow keys walk its live
    /// rows, Enter runs one.
    #[test]
    fn more_holds_the_rest_and_the_keys_walk_it() {
        use crate::snip::annotate::Menu;
        let mut app = snip_toolbar_open();
        let _ = app.on_snip_toolbar(E::Menu(Menu::More));
        let texts = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
        for row in [
            "Long capture",
            "Add to steps",
            "Pin to screen",
            "Colour picker",
            "Screen ruler",
        ] {
            assert!(texts.iter().any(|t| t == row), "{row}: {texts:?}");
        }
        assert_eq!(texts.iter().any(|t| t == "Share"), share::SUPPORTED);
        // Down x6: the sixth live row (Copy text, Ask AI, Translate, Long
        // capture, Add to steps, Pin); Enter pins (the overlay closes)
        for _ in 0..6 {
            let _ = app.on_snip_toolbar(E::MenuStep(false));
        }
        let editor = &app.o.edit.as_ref().unwrap().annotator;
        let rows = toolbar::more_rows(editor);
        assert_eq!(rows[editor.row.unwrap()].message, Some(E::Pin));
        let _ = app.on_snip_toolbar(E::Activate);
        assert!(app.hiding != Hiding::No, "pinned: the overlay closes");
    }

    /// #37: a drag-out an app took closes the overlay with its toolbar (none
    /// left floating over the desktop); one nothing took keeps the editor.
    #[test]
    fn after_a_drag_out_drop_no_toolbar_remains() {
        {
            let mut app = snip_toolbar_open();
            dragout::TEST_DROP.set(Some(false));
            let _ = app.on_snip_toolbar(E::DragOut);
            assert!(app.editing() && app.hiding == Hiding::No, "nothing took it");
            assert!(
                bar_names(&app).iter().any(|n| n.starts_with("Copy")),
                "toolbar back"
            );
            dragout::TEST_DROP.set(Some(true));
            let task = app.on_snip_toolbar(E::DragOut);
            run_task(&mut app, task);
            assert!(gone(&app), "dropped: no toolbar left");
        }
    }
}
