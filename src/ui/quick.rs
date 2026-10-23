//! Quick actions (DESIGN.md "Quick actions"): what a quick action id does, and
//! which area it runs on for its target.
use iced::{Rectangle, Size};

use crate::actions::{self, Action};
use crate::config::Target;

/// What a quick action runs once it has its area.
#[derive(Debug, Clone, Copy)]
pub enum Quick {
    /// A registry action, as if its button was pressed (`opencircle run copy`).
    Action(&'static Action),
    /// Read the text and put it on the clipboard (a toast says so).
    CopyText,
    /// Ask AI, with "Explain" already sent.
    Explain,
}

impl Quick {
    /// `None` = nothing to run yet (translate, wired later) or an unknown id.
    pub fn for_id(id: &str) -> Option<Quick> {
        match id {
            "copy_text" => Some(Quick::CopyText),
            "ai_explain" => Some(Quick::Explain),
            id => actions::ACTIONS
                .iter()
                .find(|a| a.id == id)
                .map(Quick::Action),
        }
    }
}

/// Where a quick action starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Start {
    /// Show the overlay; the first selection runs the action (no action bar).
    Select,
    /// Run at once on this area (logical px of the monitor).
    Area(Rectangle),
}

/// The start for `target` on a `screen`-sized monitor; `last` = the previous
/// selection on this monitor. No previous selection: the user draws one.
pub fn start(target: Target, screen: Size, last: Option<Rectangle>) -> Start {
    let whole = Rectangle::with_size(screen);
    match target {
        Target::FullScreen => Start::Area(whole),
        Target::Circle => Start::Select,
        // A previous region now (partly) off screen (resolution change) is clipped.
        Target::LastRegion => last
            .and_then(|r| r.intersection(&whole))
            .map_or(Start::Select, Start::Area),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Point;

    const SCREEN: Size = Size::new(1920.0, 1080.0);

    #[test]
    fn targets_resolve_to_an_area_or_a_selection() {
        let whole = Rectangle::with_size(SCREEN);
        let last = Rectangle::new(Point::new(100.0, 900.0), Size::new(800.0, 120.0));
        assert_eq!(start(Target::FullScreen, SCREEN, None), Start::Area(whole));
        assert_eq!(
            start(Target::FullScreen, SCREEN, Some(last)),
            Start::Area(whole)
        );
        assert_eq!(start(Target::Circle, SCREEN, Some(last)), Start::Select);
        assert_eq!(
            start(Target::LastRegion, SCREEN, Some(last)),
            Start::Area(last)
        );
        // No previous region: draw one.
        assert_eq!(start(Target::LastRegion, SCREEN, None), Start::Select);
        // Remembered on a bigger screen: clipped, or drawn anew when fully off.
        let wide = Rectangle::new(Point::new(1800.0, 1000.0), Size::new(400.0, 200.0));
        assert_eq!(
            start(Target::LastRegion, SCREEN, Some(wide)),
            Start::Area(Rectangle::new(
                Point::new(1800.0, 1000.0),
                Size::new(120.0, 80.0)
            ))
        );
        let gone = Rectangle::new(Point::new(2500.0, 100.0), Size::new(100.0, 100.0));
        assert_eq!(start(Target::LastRegion, SCREEN, Some(gone)), Start::Select);
    }

    #[test]
    fn quick_ids_map_to_what_they_run() {
        assert!(matches!(Quick::for_id("copy_text"), Some(Quick::CopyText)));
        assert!(matches!(Quick::for_id("ai_explain"), Some(Quick::Explain)));
        for id in ["lens", "ai", "text", "edit", "copy", "save"] {
            assert!(
                matches!(Quick::for_id(id), Some(Quick::Action(a)) if a.id == id),
                "{id}"
            );
        }
        // Translate is wired by the translate pipeline; snip is its own mode.
        assert!(Quick::for_id("translate").is_none());
        assert!(Quick::for_id("snip").is_none());
        assert!(Quick::for_id("").is_none());
    }
}
