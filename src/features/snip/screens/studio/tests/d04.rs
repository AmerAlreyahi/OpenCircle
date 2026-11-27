//! D-04: Studio round 6 with the "Group chips" tool rail.
//!
//! AC1 no status bar: the stage runs to the window's bottom edge.
//! AC2 two floating pills over the stage: the size chip at its bottom start, zoom + Save +
//!     Copy at its bottom end.
//! AC3 the rail starts below the top band and each tool group is its own tile (separate
//!     rounded tiles with a gap, no container round them).
use super::*;
use iced::advanced::widget::{Id, Operation, operation};
use iced_runtime::user_interface::{Cache, UserInterface};

struct Bounds(Vec<(String, Rectangle)>);

impl Operation for Bounds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if let Some(id) = id {
            self.0.push((format!("{id:?}"), bounds));
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        _: Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        if let Some(id) = id {
            self.0.push((format!("{id:?}"), bounds));
        }
    }
}

/// The bounds of every identified container / scrollable of the editor window.
pub(super) fn bounds(s: &Studio) -> Vec<(String, Rectangle)> {
    crate::shared::kit::text::load();
    let mut renderer = crate::shared::kit::testing::shot::renderer();
    let mut ui = UserInterface::build(view(s, &[], false), SIZE, Cache::default(), &mut renderer);
    let mut op = Bounds(Vec::new());
    ui.operate(&renderer, &mut op);
    op.0
}

pub(super) fn find(all: &[(String, Rectangle)], id: &str) -> Rectangle {
    all.iter()
        .find(|(n, _)| n.contains(id))
        .unwrap_or_else(|| panic!("{id}"))
        .1
}

fn editor() -> Studio {
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(crop(300, 200)));
    s.settle_open();
    s
}

#[test]
fn d04_ac1_no_status_bar_the_stage_reaches_the_window_bottom() {
    let s = editor();
    let all = bounds(&s);
    let canvas = find(&all, super::CANVAS);
    // the window's 1 px rim below the stage (a status bar would leave 45 px)
    assert!(
        (canvas.y + canvas.height - (SIZE.height - 1.0)).abs() < 3.0,
        "{canvas:?}"
    );
    // the room a fitted picture is centred in: above the pills' band
    assert_eq!(
        s.stage().height,
        SIZE.height - 2.0 - super::HEADER - (super::OPTIONS + 1.0) - super::BAND
    );
}

#[test]
fn d04_ac2_pills_float_over_the_stage_corners() {
    let s = editor();
    let all = bounds(&s);
    let stage = find(&all, super::CANVAS);
    let (size, actions) = (
        find(&all, super::pills::SIZE_PILL),
        find(&all, super::pills::ACTIONS_PILL),
    );
    let inside = |r: Rectangle| {
        r.x >= stage.x
            && r.y >= stage.y
            && r.x + r.width <= stage.x + stage.width + 1.0
            && r.y + r.height <= stage.y + stage.height + 1.0
    };
    assert!(
        inside(size) && inside(actions),
        "{size:?} {actions:?} over {stage:?}"
    );
    // bottom start / bottom end, on the same line near the bottom edge
    assert!(size.x - stage.x < 24.0, "size chip at the start: {size:?}");
    assert!(
        stage.x + stage.width - (actions.x + actions.width) < 24.0,
        "{actions:?}"
    );
    assert!(stage.y + stage.height - (size.y + size.height) < 24.0);
    assert!((size.y + size.height - (actions.y + actions.height)).abs() < 8.0);
    assert!(size.x + size.width < actions.x, "two separate pills");
}

#[test]
fn d04_ac3_rail_tiles_start_below_the_band_and_are_separate() {
    let s = editor();
    let all = bounds(&s);
    let tiles: Vec<Rectangle> = super::view::TILES.iter().map(|t| find(&all, t)).collect();
    let band_bottom = super::HEADER + super::OPTIONS;
    assert!(
        tiles[0].y >= band_bottom,
        "rail starts below the top band: {:?}",
        tiles[0]
    );
    for w in tiles.windows(2) {
        let gap = w[1].y - (w[0].y + w[0].height);
        assert!(gap >= 4.0, "tiles are separate chips, gap {gap}");
    }
    // all of them fit the default window without scrolling the rail
    assert!(
        tiles[5].y + tiles[5].height <= SIZE.height - 1.0,
        "{:?}",
        tiles[5]
    );
    // every tile is as wide as the others (one column of chips, no outer container)
    assert!(tiles.iter().all(|t| (t.width - tiles[0].width).abs() < 0.5));
    assert!(
        tiles[0].width < super::RAIL,
        "a tile is narrower than the rail's room"
    );
}

/// Fit (and the first open) keeps the whole picture, any shape, above the pills.
#[test]
fn d04_fit_keeps_the_picture_clear_of_the_pills() {
    for (w, h) in [(300, 200), (3000, 2000), (800, 1500), (1600, 300)] {
        let mut s = Studio::new(StudioConfig::default());
        s.open(Picture::Crop(crop(w, h)));
        s.settle_open();
        let _ = s.update(Msg::Fit);
        let all = bounds(&s);
        let (size, actions) = (
            find(&all, super::pills::SIZE_PILL),
            find(&all, super::pills::ACTIONS_PILL),
        );
        let e = s.editor.as_ref().unwrap();
        let shown = e.size();
        let top = 1.0 + super::HEADER + super::OPTIONS + 1.0;
        let bottom = top + super::stage_corner(s.stage(), shown).y - s.offset.y + shown.height;
        assert!(
            bottom <= size.y.min(actions.y),
            "{w}x{h}: picture bottom {bottom} under the pills at {}",
            size.y.min(actions.y)
        );
    }
}
