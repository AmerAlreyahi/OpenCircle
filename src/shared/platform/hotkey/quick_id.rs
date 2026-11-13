//! The id of a quick action, as the config file, the CLI (`opencircle run <id>`) and
//! the Labs switches spell it. One enum instead of strings re-spelled in four places;
//! the names are the file format and never change.
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QuickId {
    // The registry actions (`actions::ACTIONS`, same ids).
    Lens,
    Ai,
    Text,
    Translate,
    Edit,
    Copy,
    Save,
    // Quick actions that are not registry actions.
    CopyText,
    AiExplain,
    Snip,
    SongSearch,
    /// Snip a part of a window into a live pin (Windows, Labs Pins).
    LivePin,
    /// Snip's screen tools (`ui::screens::tools`).
    ColourPicker,
    Ruler,
    /// Any other well-formed id in the config: kept and written back as it was, but
    /// it runs nothing.
    Other(String),
}

impl QuickId {
    /// Every id the app knows.
    pub const KNOWN: [QuickId; 14] = [
        QuickId::Lens,
        QuickId::Ai,
        QuickId::Text,
        QuickId::Translate,
        QuickId::Edit,
        QuickId::Copy,
        QuickId::Save,
        QuickId::CopyText,
        QuickId::AiExplain,
        QuickId::Snip,
        QuickId::SongSearch,
        QuickId::LivePin,
        QuickId::ColourPicker,
        QuickId::Ruler,
    ];

    pub fn as_str(&self) -> &str {
        match self {
            QuickId::Lens => "lens",
            QuickId::Ai => "ai",
            QuickId::Text => "text",
            QuickId::Translate => "translate",
            QuickId::Edit => "edit",
            QuickId::Copy => "copy",
            QuickId::Save => "save",
            QuickId::CopyText => "copy_text",
            QuickId::AiExplain => "ai_explain",
            QuickId::Snip => "snip",
            QuickId::SongSearch => "song_search",
            QuickId::LivePin => "live_pin",
            QuickId::ColourPicker => "colour_picker",
            QuickId::Ruler => "ruler",
            QuickId::Other(name) => name,
        }
    }

    pub fn from_name(name: &str) -> QuickId {
        Self::KNOWN
            .into_iter()
            .find(|id| id.as_str() == name)
            .unwrap_or_else(|| QuickId::Other(name.to_string()))
    }

    /// Does the app know this id (may `opencircle run` start it)?
    pub fn is_known(&self) -> bool {
        !matches!(self, QuickId::Other(_))
    }
}

impl std::ops::Deref for QuickId {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for QuickId {
    fn from(name: &str) -> QuickId {
        QuickId::from_name(name)
    }
}

impl std::fmt::Display for QuickId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for QuickId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl PartialEq<str> for QuickId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for QuickId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_ones_the_config_always_used() {
        let names = [
            "lens",
            "ai",
            "text",
            "translate",
            "edit",
            "copy",
            "save",
            "copy_text",
            "ai_explain",
            "snip",
            "song_search",
            "live_pin",
        ];
        for (id, name) in QuickId::KNOWN.iter().zip(names) {
            assert_eq!(id.as_str(), name);
            assert_eq!(&QuickId::from_name(name), id);
        }
        // An id from a newer or hand-edited config survives unchanged.
        let other = QuickId::from_name("future_thing");
        assert_eq!(other, QuickId::Other("future_thing".into()));
        assert_eq!(other.to_string(), "future_thing");
        assert!(!other.is_known());
    }

    #[test]
    fn the_registry_and_the_enum_agree() {
        for a in crate::actions::ACTIONS {
            assert!(QuickId::from_name(a.id).is_known(), "{}", a.id);
        }
    }
}
