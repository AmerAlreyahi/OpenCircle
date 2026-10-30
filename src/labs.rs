//! Labs: one switch per unfinished feature, all off by default (`[labs]` in the
//! config; Settings > Labs). `Labs::has` is the one source of truth: the tray, the
//! shortcuts, the CLI, the bottom bar and Settings all ask it, so a hidden feature
//! has no shortcut, tray item, bar button or Settings row. Its own settings stay in
//! the file untouched; turning it on brings them back.
//!
//! Live translate is hidden here until 0.3.0 (owner, 2026-10-27).
use serde::Serialize;

use crate::hotkey::QuickId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// Snip mode: its shortcuts (Print Screen ...), tray item and Settings page.
    Snip,
    /// The annotation editor: the bar's Snip button (E), and the editor after a capture.
    Editor,
    /// Song search: bar button, shortcut and Settings page.
    Song,
    /// The local history of searches (`general.search_history`).
    History,
    /// Pinning an image to the screen from the editor.
    Pins,
    /// Live translate: bar button, tray item, shortcut and its Settings rows.
    Live,
}

impl Feature {
    pub const ALL: [Feature; 6] = [
        Feature::Snip,
        Feature::Editor,
        Feature::Song,
        Feature::History,
        Feature::Pins,
        Feature::Live,
    ];

    /// The switch's name in Settings and in messages.
    pub fn title(self) -> &'static str {
        match self {
            Feature::Snip => "Snip",
            Feature::Editor => "Snip editor",
            Feature::Song => "Song search",
            Feature::History => "Search history",
            Feature::Pins => "Pins",
            Feature::Live => "Live translate",
        }
    }

    /// One honest line: what it is, and that it is unfinished.
    pub fn blurb(self) -> &'static str {
        match self {
            Feature::Snip => {
                "Screenshots with Print Screen, window and region capture. Unfinished; may change"
            }
            Feature::Editor => "Draw on a capture: arrows, text, blur. Unfinished; may change",
            Feature::Song => "Find the song playing on your PC. Unfinished; may change",
            Feature::History => "Keep a list of your searches on this PC. Unfinished; may change",
            Feature::Pins => "Float a capture above your windows. Unfinished; may change",
            Feature::Live => "Keep the screen translated as it changes. Unfinished; may change",
        }
    }
}

/// `[labs]`: which unfinished features are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Labs {
    pub snip: bool,
    pub editor: bool,
    pub song: bool,
    pub history: bool,
    pub pins: bool,
    pub live: bool,
}

impl Labs {
    pub fn has(&self, f: Feature) -> bool {
        match f {
            Feature::Snip => self.snip,
            Feature::Editor => self.editor,
            Feature::Song => self.song,
            Feature::History => self.history,
            Feature::Pins => self.pins,
            Feature::Live => self.live,
        }
    }

    pub fn set(&mut self, f: Feature, on: bool) {
        match f {
            Feature::Snip => self.snip = on,
            Feature::Editor => self.editor = on,
            Feature::Song => self.song = on,
            Feature::History => self.history = on,
            Feature::Pins => self.pins = on,
            Feature::Live => self.live = on,
        }
    }

    /// Every switch on (the ids a shortcut had before it could be hidden).
    pub fn all_on() -> Labs {
        Labs {
            snip: true,
            editor: true,
            song: true,
            history: true,
            pins: true,
            live: true,
        }
    }
}

/// Is `f` on in the running app's config?
pub fn has(f: Feature) -> bool {
    crate::config::current().labs.has(f)
}

/// The Labs feature an action id (registry, quick action or `opencircle run`) belongs to.
pub fn action_feature(id: &QuickId) -> Option<Feature> {
    match id {
        QuickId::Snip => Some(Feature::Snip),
        QuickId::Edit => Some(Feature::Editor),
        QuickId::SongSearch => Some(Feature::Song),
        QuickId::LivePin => Some(Feature::Pins),
        _ => None,
    }
}

/// Is the action `id` available with these switches? A live pin also needs Windows.
pub fn action_on(labs: &Labs, id: &QuickId) -> bool {
    action_feature(id).is_none_or(|f| labs.has(f))
        && (*id != QuickId::LivePin || crate::capture::live::SUPPORTED)
}

/// Is this quick action available? Live translate is its own Labs feature.
pub fn quick_on(labs: &Labs, q: &crate::config::QuickAction) -> bool {
    action_on(labs, &q.action) && (!q.live || labs.live)
}

/// The CLI's answer for a hidden feature's target (`opencircle run snip`): this
/// message on stderr and exit status 2, the same as any refused command.
pub fn command_allowed(labs: &Labs, action: &str) -> Result<(), String> {
    match action_feature(&QuickId::from_name(action)).filter(|f| !labs.has(*f)) {
        Some(f) => Err(format!(
            "\"{action}\" is off: turn on {} in Settings > Labs",
            f.title()
        )),
        None => Ok(()),
    }
}

/// The overlay's bottom bar after its AI field, in order (the new design's
/// search bar, owner 2026-10-26): three labelled pills, then icon buttons. A
/// button of a hidden feature is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarButton {
    /// Pill: the selection to Google Lens (or its text to a search) in the browser.
    Search,
    /// Pill: the selection's text to the clipboard.
    CopyText,
    /// Pill: translate the selection (or the screen) in place.
    Translate,
    /// Icon: live translate (Beta).
    Live,
    Song,
    Edit,
}

impl BarButton {
    const ALL: [BarButton; 6] = [
        BarButton::Search,
        BarButton::CopyText,
        BarButton::Translate,
        BarButton::Live,
        BarButton::Song,
        BarButton::Edit,
    ];

    /// A labelled pill (else a round icon button).
    pub fn is_pill(self) -> bool {
        matches!(
            self,
            BarButton::Search | BarButton::CopyText | BarButton::Translate
        )
    }

    fn feature(self) -> Option<Feature> {
        match self {
            BarButton::Song => Some(Feature::Song),
            BarButton::Edit => Some(Feature::Editor),
            BarButton::Live => Some(Feature::Live),
            _ => None,
        }
    }
}

/// The bar's buttons with these switches.
pub fn bar(labs: &Labs) -> Vec<BarButton> {
    (BarButton::ALL.into_iter())
        .filter(|b| b.feature().is_none_or(|f| labs.has(f)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_by_default() {
        for f in Feature::ALL {
            assert!(!Labs::default().has(f), "{f:?}");
        }
    }

    #[test]
    fn set_and_has_agree_for_every_feature() {
        for f in Feature::ALL {
            let mut l = Labs::default();
            l.set(f, true);
            assert!(l.has(f));
            assert_eq!(Feature::ALL.iter().filter(|g| l.has(**g)).count(), 1);
        }
    }

    #[test]
    fn the_live_pin_quick_action_is_a_pins_feature_and_windows_only() {
        let id = QuickId::LivePin;
        assert_eq!(action_feature(&id), Some(Feature::Pins));
        assert!(!action_on(&Labs::default(), &id));
        assert_eq!(action_on(&Labs::all_on(), &id), cfg!(windows));
        assert!(command_allowed(&Labs::default(), "live_pin").is_err());
    }

    #[test]
    fn hidden_features_have_no_bar_button() {
        use BarButton::*;
        assert_eq!(
            bar(&Labs::default()),
            [Search, CopyText, Translate],
            "the design's bar with Labs off"
        );
        let mut on = Labs::default();
        on.set(Feature::Live, true);
        assert_eq!(bar(&on), [Search, CopyText, Translate, Live]);
        on.set(Feature::Song, true);
        assert_eq!(bar(&on), [Search, CopyText, Translate, Live, Song]);
        on.set(Feature::Editor, true);
        assert_eq!(bar(&on), [Search, CopyText, Translate, Live, Song, Edit]);
        assert_eq!(bar(&on).iter().filter(|b| b.is_pill()).count(), 3);
    }

    #[test]
    fn the_cli_says_why_a_hidden_target_is_refused() {
        let off = Labs::default();
        let msg = command_allowed(&off, "snip").unwrap_err();
        assert_eq!(msg, "\"snip\" is off: turn on Snip in Settings > Labs");
        assert!(command_allowed(&off, "song_search").is_err());
        assert!(command_allowed(&off, "edit").is_err());
        assert!(command_allowed(&off, "translate").is_ok());
        assert!(command_allowed(&Labs::all_on(), "snip").is_ok());
    }
}
