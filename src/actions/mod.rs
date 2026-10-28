//! The action registry: ONE list of what can be done with a selection. The action
//! bar renders it in order; the first entry is the default (Enter).
//! To add an action: see ARCHITECTURE.md ("Add an action").
pub mod ask;
pub mod copy;
pub mod crop;
pub mod edit;
pub mod entities;
pub mod lens;
pub mod qr;
pub mod save;
pub mod text;
pub mod translate;

/// Every action, in action-bar order. The first one runs on Enter.
pub const ACTIONS: &[Action] = &[
    lens::ACTION,
    ask::ACTION,
    text::ACTION,
    translate::ACTION,
    edit::ACTION,
    copy::ACTION,
    save::ACTION,
];

#[derive(Debug)]
pub struct Action {
    /// Stable name (logs, later the config's quick actions).
    pub id: &'static str,
    /// Button text.
    pub label: &'static str,
    pub key: Key,
    /// Glyph in `assets/lucide/lucide-subset.ttf` (must be listed in its `subset.sh`).
    pub icon: char,
    pub run: Run,
}

/// What running an action does.
#[derive(Debug, Clone, Copy)]
pub enum Run {
    /// Close the overlay, then do this with the crop (Copy, Save). Ok = the
    /// confirmation pill's text ("Copied", "Saved to ..."); Err = its error text.
    Finish(fn(Crop) -> Result<String, String>),
    /// Open this result panel beside the selection; the overlay stays.
    Panel(Panel),
    /// Turn the action bar into the editing toolbar over the selection.
    Edit,
    /// Replace the selection's text by its translation, in place.
    Translate,
}

/// The result panels (`ui/panel.rs` draws them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Lens,
    Text,
    Ask,
}

/// The action's key while the overlay is open: a Latin letter (layout-independent),
/// alone or with Ctrl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub ctrl: bool,
    pub letter: char,
}

/// The action bound to `letter` (lower-case Latin) with/without Ctrl; none for an
/// action whose Labs feature is off.
pub fn for_key(labs: &crate::labs::Labs, letter: char, ctrl: bool) -> Option<&'static Action> {
    let key = Key { ctrl, letter };
    (ACTIONS.iter())
        .find(|action| action.key == key)
        .filter(|action| {
            crate::labs::action_on(labs, &crate::hotkey::QuickId::from_name(action.id))
        })
}

/// The action Enter runs: the config's `general.default_action` (the first
/// registry entry if the registry ever lacks it).
pub fn default_for(cfg: &crate::config::Config) -> &'static Action {
    let id = cfg.general.default_action.id();
    ACTIONS.iter().find(|a| a.id == id).unwrap_or(&ACTIONS[0])
}

pub use crop::{Crop, crop, crop_bounds, encode, fit_long_edge, stack_crops};

/// Writes an RGBA image as an 8-bit PNG.
pub fn write_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    std::fs::write(path, encode_png(width, height, rgba)?).map_err(|e| e.to_string())
}

/// An RGBA image as 8-bit PNG bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keys_and_labels_are_unique() {
        for (i, a) in ACTIONS.iter().enumerate() {
            for b in &ACTIONS[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.key, b.key, "{} and {} share a key", a.id, b.id);
                assert_ne!(a.label, b.label);
            }
            assert!(a.key.letter.is_ascii_lowercase(), "{}", a.id);
            let on = crate::labs::Labs::all_on();
            assert_eq!(
                for_key(&on, a.key.letter, a.key.ctrl).map(|f| f.id),
                Some(a.id)
            );
        }
    }

    #[test]
    fn a_hidden_feature_has_no_key() {
        let (off, on) = (crate::labs::Labs::default(), crate::labs::Labs::all_on());
        let e = edit::ACTION.key;
        assert!(
            for_key(&off, e.letter, e.ctrl).is_none(),
            "E with the editor off"
        );
        assert_eq!(for_key(&on, e.letter, e.ctrl).map(|a| a.id), Some("edit"));
        let l = lens::ACTION.key;
        assert!(for_key(&off, l.letter, l.ctrl).is_some());
    }

    #[test]
    fn enter_runs_the_configured_default_action() {
        use crate::config::{Config, DefaultAction};
        let mut cfg = Config::default();
        assert_eq!(default_for(&cfg).id, "lens", "Lens out of the box");
        for (choice, id) in [
            (DefaultAction::Ai, "ai"),
            (DefaultAction::Text, "text"),
            (DefaultAction::Copy, "copy"),
            (DefaultAction::Lens, "lens"),
        ] {
            cfg.general.default_action = choice;
            assert_eq!(default_for(&cfg).id, id);
        }
        // A saved choice survives the config file.
        let (back, _) = Config::parse("[general]\ndefault_action = \"text\"\n");
        assert_eq!(default_for(&back).id, "text");
    }

    #[test]
    fn every_icon_is_in_the_subset_font() {
        let script = include_str!("../../assets/lucide/subset.sh");
        for a in ACTIONS {
            let code = format!("U+{:04X}", a.icon as u32);
            assert!(script.contains(&code), "{}: add {code} to subset.sh", a.id);
        }
    }
}
