//! R-18: the Studio view moves ONLY when the user moves it on purpose, plus the two removals
//! (no extra slider toggle in the options bar, no Picture info).
use super::*;
use crate::shared::kit::motion;
use iced::advanced::widget::operation::scrollable::scroll_to;
use iced::advanced::widget::{Id, Operation, operation};
use iced::widget::scrollable::AbsoluteOffset;
use iced_runtime::user_interface::{Cache, UserInterface};
use std::sync::Arc;

#[test]
fn r18_right_click_keeps_view_and_zoom() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    h.s.pointer.set(Point::new(200.0, 150.0));
    let _ = h.send(Msg::ContextOpen);
    assert!(h.s.context.is_some());
    h.assert_centre_and_zoom(centre, zoom, "right-click open");
    let _ = h.send(Msg::Cancel);
    assert!(h.s.context.is_none());
    h.assert_centre_and_zoom(centre, zoom, "right-click close");
}

#[test]
fn r18_pin_steps_keeps_centre_and_zoom() {
    // Zoomed & panned
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::Library);
    h.assert_centre_and_zoom(centre, zoom, "open slider before pin");
    let _ = h.send(Msg::PinSteps);
    assert!(h.s.sidebar_docked());
    h.assert_centre_and_zoom(centre, zoom, "pin Steps slider while zoomed");

    // Fitted (not zoomed by hand yet): pinning must NOT re-fit (zoom stays identical)
    let mut f = Harness::fitted(3840, 2160);
    let (fc, fz) = (f.visible_centre(), f.zoom());
    let _ = f.send(Msg::Library);
    let _ = f.send(Msg::PinSteps);
    f.assert_centre_and_zoom(fc, fz, "pin Steps slider while fitted");
}

#[test]
fn r18_unpin_steps_keeps_centre_and_zoom() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let _ = h.send(Msg::Library);
    let _ = h.send(Msg::PinSteps);
    h.apply_scroll(AbsoluteOffset { x: 400.0, y: 200.0 });
    let _ = h.s.update(Msg::Scrolled(Vector::new(400.0, 200.0)));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::PinSteps);
    assert!(!h.s.sidebar_docked());
    h.assert_centre_and_zoom(centre, zoom, "unpin Steps slider while zoomed");
}

#[test]
fn r18_preferences_open_and_close_keeps_view_and_zoom() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::OpenPrefs);
    assert!(h.s.prefs.is_some());
    h.assert_centre_and_zoom(centre, zoom, "Preferences open");
    let _ = h.send(Msg::Prefs(preferences::Msg::Close));
    assert!(h.s.prefs.is_none());
    h.assert_centre_and_zoom(centre, zoom, "Preferences close");
}

#[test]
fn r18_steps_overlay_open_and_close_keeps_view_and_zoom() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::Library);
    h.assert_centre_and_zoom(centre, zoom, "Steps overlay open");
    let _ = h.send(Msg::Library);
    h.assert_centre_and_zoom(centre, zoom, "Steps overlay close");
}

#[test]
fn r18_steps_docked_open_and_close_keeps_centre_and_zoom() {
    // English (right edge)
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let _ = h.send(Msg::Library);
    let _ = h.send(Msg::PinSteps);
    h.apply_scroll(AbsoluteOffset { x: 400.0, y: 200.0 });
    let _ = h.s.update(Msg::Scrolled(Vector::new(400.0, 200.0)));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::Library); // close while pinned
    h.assert_centre_and_zoom(centre, zoom, "docked Steps close (LTR)");
    let _ = h.send(Msg::Library); // open while pinned
    h.assert_centre_and_zoom(centre, zoom, "docked Steps open (LTR)");

    // Arabic (left edge: sidebar is before the stage visually)
    let _ar = crate::shared::kit::testing::arabic();
    let mut ha = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let _ = ha.send(Msg::Library);
    let _ = ha.send(Msg::PinSteps);
    ha.apply_scroll(AbsoluteOffset { x: 400.0, y: 200.0 });
    let _ = ha.s.update(Msg::Scrolled(Vector::new(400.0, 200.0)));
    let (ac, az) = (ha.visible_centre(), ha.zoom());
    let _ = ha.send(Msg::Library);
    ha.assert_centre_and_zoom(ac, az, "docked Steps close (RTL)");
    let _ = ha.send(Msg::Library);
    ha.assert_centre_and_zoom(ac, az, "docked Steps open (RTL)");
}

#[test]
fn r18_popovers_menus_and_sheets_keep_view_and_zoom() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(320.0, 210.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());

    let _ = h.send(Msg::Menu(Some(menu_bar::Menu::File)));
    h.assert_centre_and_zoom(centre, zoom, "File menu open");
    let _ = h.send(Msg::Menu(None));
    h.assert_centre_and_zoom(centre, zoom, "File menu close");

    let _ = h.send(Msg::Pop(Some(popovers::Pop::Actions)));
    h.assert_centre_and_zoom(centre, zoom, "Actions popover open");
    let _ = h.send(Msg::Pop(None));
    h.assert_centre_and_zoom(centre, zoom, "Actions popover close");

    let _ = h.send(Msg::Export);
    h.assert_centre_and_zoom(centre, zoom, "Export slide-over open");
    let _ = h.send(Msg::WalkCancel);
    h.assert_centre_and_zoom(centre, zoom, "Export slide-over close");

    let _ = h.send(Msg::Shortcuts(true));
    h.assert_centre_and_zoom(centre, zoom, "Shortcuts modal open");
    let _ = h.send(Msg::Shortcuts(false));
    h.assert_centre_and_zoom(centre, zoom, "Shortcuts modal close");

    let _ = h.send(Msg::Space(true));
    h.assert_centre_and_zoom(centre, zoom, "Space pan start");
    let _ = h.send(Msg::Space(false));
    h.assert_centre_and_zoom(centre, zoom, "Space pan end");
}

#[test]
fn r18_dpi_change_keeps_zoom_in_range() {
    let mut h = Harness::zoomed(3000, 2000, Vector::new(400.0, 300.0));
    h.s.editor.as_mut().unwrap().zoom = ZOOM_RANGE.end() / h.s.scale;
    let _ = h.send(Msg::Scale(h.s.scale / 4.0));
    let z = h.zoom();
    assert!(
        z <= ZOOM_RANGE.end() / h.s.scale + 1e-4,
        "zoom {z} over the range"
    );
}

#[test]
fn r18_window_resize_keeps_centre_and_zoom() {
    // Zoomed & scrolled
    let mut h = Harness::zoomed(3000, 2000, Vector::new(400.0, 300.0));
    let (centre, zoom) = (h.visible_centre(), h.zoom());
    let _ = h.send(Msg::Resized(Size::new(960.0, 640.0)));
    h.assert_centre_and_zoom(centre, zoom, "window shrink while zoomed");
    let _ = h.send(Msg::Resized(Size::new(1600.0, 1000.0)));
    h.assert_centre_and_zoom(centre, zoom, "window grow while zoomed");

    // Fitted (never zoomed by hand): resize must NOT re-fit
    let mut f = Harness::fitted(3840, 2160);
    let (fc, fz) = (f.visible_centre(), f.zoom());
    let _ = f.send(Msg::Resized(Size::new(960.0, 640.0)));
    f.assert_centre_and_zoom(fc, fz, "window shrink while fitted");
}

#[test]
fn r18_options_bar_has_no_extra_slider_button() {
    let s = studio_on(640, 400);
    let e = s.editor.as_ref().unwrap();
    let size = Size::new(SIZE.width - 54.0, 48.0);
    let hits = (0..12)
        .filter(|i| {
            let at = Point::new(size.width - 10.0 - 4.0 * *i as f32, 24.0);
            crate::shared::kit::testing::shot::click(options_bar::view(&s, e), size, at)
                .iter()
                .any(|m| matches!(m, Msg::Library))
        })
        .count();
    assert_eq!(
        hits, 0,
        "options bar must not contain a Steps toggle button"
    );

    let empty = Studio::new(StudioConfig::default());
    let empty_hits = (0..12)
        .filter(|i| {
            let at = Point::new(
                SIZE.width - 10.0 - 4.0 * *i as f32,
                super::HEADER + super::OPTIONS / 2.0,
            );
            click_view(&empty, (at.x, at.y))
                .iter()
                .any(|m| matches!(m, Msg::Library))
        })
        .count();
    assert_eq!(
        empty_hits, 0,
        "empty Studio options bar must not contain a Steps toggle button"
    );
}

#[test]
fn r18_picture_info_is_removed() {
    crate::shared::kit::text::load();
    let s = studio_on(640, 400);
    let texts = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    assert!(
        texts.iter().any(|t| t.contains("640") && t.contains("400")),
        "size chip stays: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Picture info"),
        "Picture info button must be removed: {texts:?}"
    );
}

struct ReadScroll {
    id: Id,
    offset: Option<Vector>,
}

impl Operation for ReadScroll {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&self.id) {
            self.offset = Some(translation);
        }
    }
}

/// Persistent iced UserInterface harness: keeps widget Tree state across frames just like a
/// real window so any widget-tree shape change that resets scrollable state is caught immediately.
struct Harness {
    s: Studio,
    steps: Vec<Arc<Step>>,
    size: Size,
    renderer: iced::Renderer,
    cache: Cache,
}

impl Harness {
    fn zoomed(w: u32, h: u32, scroll: Vector) -> Self {
        motion::set_reduced(true);
        crate::shared::kit::text::load();
        let mut s = Studio::new(StudioConfig::default());
        s.open(Picture::Crop(crop(w, h)));
        s.settle_open();
        let _ = s.update(Msg::ZoomIn);
        let _ = s.update(Msg::ZoomIn);
        let _ = s.update(Msg::ZoomIn);
        let _ = s.update(Msg::ZoomIn);
        let steps: Vec<Arc<Step>> = [(300, 200), (200, 300)]
            .map(|(sw, sh)| Arc::new(Step::new(&crop(sw, sh)).unwrap()))
            .into();
        let mut h = Harness {
            s,
            steps,
            size: SIZE,
            renderer: crate::shared::kit::testing::shot::renderer(),
            cache: Cache::default(),
        };
        h.apply_scroll(AbsoluteOffset {
            x: scroll.x,
            y: scroll.y,
        });
        let _ = h.s.update(Msg::Scrolled(scroll));
        let got = h.read_scroll();
        assert!(
            (got.x - scroll.x).abs() < 1.0 && (got.y - scroll.y).abs() < 1.0,
            "initial scroll failed to take: want {scroll:?}, got {got:?}"
        );
        h
    }

    fn fitted(w: u32, h: u32) -> Self {
        motion::set_reduced(true);
        crate::shared::kit::text::load();
        let mut s = Studio::new(StudioConfig::default());
        s.open(Picture::Crop(crop(w, h)));
        s.settle_open();
        let _ = s.take_scroll();
        let steps: Vec<Arc<Step>> = [(300, 200)]
            .map(|(sw, sh)| Arc::new(Step::new(&crop(sw, sh)).unwrap()))
            .into();
        let mut h = Harness {
            s,
            steps,
            size: SIZE,
            renderer: crate::shared::kit::testing::shot::renderer(),
            cache: Cache::default(),
        };
        let _ = h.read_scroll();
        h
    }

    fn apply_scroll(&mut self, to: AbsoluteOffset) {
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(
            view(&self.s, &self.steps, false),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut op = scroll_to(
            Id::new(super::CANVAS),
            AbsoluteOffset {
                x: Some(to.x),
                y: Some(to.y),
            },
        );
        ui.operate(&self.renderer, &mut op);
        self.cache = ui.into_cache();
    }

    /// Rebuilds the UI with the persistent Cache (just like iced on every frame) and reads the
    /// real scrollable offset back out of the widget tree.
    fn read_scroll(&mut self) -> Vector {
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(
            view(&self.s, &self.steps, false),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut op = ReadScroll {
            id: Id::new(super::CANVAS),
            offset: None,
        };
        ui.operate(&self.renderer, &mut op);
        self.cache = ui.into_cache();
        op.offset.expect("CANVAS scrollable exists")
    }

    fn send(&mut self, msg: Msg) -> Outcome {
        if let Msg::Resized(sz) = msg {
            self.size = sz;
        }
        let out = self.s.update(msg);
        if let Outcome::ScrollTo(to) = out {
            self.apply_scroll(to);
        }
        if let Some(to) = self.s.take_scroll() {
            self.apply_scroll(to);
        }
        out
    }

    fn zoom(&self) -> f32 {
        self.s.editor.as_ref().unwrap().zoom
    }

    /// The image point currently at the centre of the visible stage, measured from the REAL
    /// scrollable widget state in iced's Tree.
    fn visible_centre(&mut self) -> Point {
        let off = self.read_scroll();
        let e = self.s.editor.as_ref().unwrap();
        let (room, shown) = (self.s.stage(), e.size());
        let corner = Vector::new(
            ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
            ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
        );
        let d = (Vector::new(room.width / 2.0, room.height / 2.0) + off - corner) * (1.0 / e.zoom);
        let origin = e.doc.view().position();
        Point::new(origin.x + d.x, origin.y + d.y)
    }

    fn assert_centre_and_zoom(&mut self, want_centre: Point, want_zoom: f32, label: &str) {
        let got_zoom = self.zoom();
        assert!(
            (got_zoom - want_zoom).abs() < 1e-4,
            "{label}: zoom changed {want_zoom} -> {got_zoom}"
        );
        let got_centre = self.visible_centre();
        assert!(
            (got_centre.x - want_centre.x).abs() < 1.5
                && (got_centre.y - want_centre.y).abs() < 1.5,
            "{label}: visible centre jumped {want_centre:?} -> {got_centre:?} (scroll={:?})",
            self.read_scroll()
        );
    }
}
