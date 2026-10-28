//! Settings' tests: the state's logic and the pages as drawn.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::{Color, Element, Length, Size};

use super::pages::SONG_PRIVACY;
use super::pages::{self, PAGES};
use super::*;
use crate::config::{
    self, After, BarStyle, ColourSet, Config, DefaultAction, EdgeMode, EdgeStyle, Engine, Format,
    LookTheme, Provider, Rgb, SearchWith, SelectStyle, SnipAfter, SongSource, TextReader, Theme,
};
use crate::hotkey::{DesktopKey, managed};
use crate::labs::{Feature, Labs};
use crate::music::{self};
use crate::secrets::{KeyStore, Secret, SecretsError};
use crate::translate::lang::{self, LANGUAGES};
use crate::ui::kit::header::Caption;
use crate::ui::kit::motion;
use crate::ui::theme;

/// The window as drawn, not maximised.
fn view(s: &State) -> Element<'_, Message> {
    super::view(s, false)
}
use crate::secrets::MemoryStore;

/// `update` with its disk write done at once (the shell does it on a thread),
/// and its motion over (the shots show the window at rest).
fn update(state: &mut State, message: Message) -> Outcome {
    let out = moving_update(state, message);
    (state.moved, state.picked) = (None, None);
    out
}

fn moving_update(state: &mut State, message: Message) -> Outcome {
    let mut out = super::update(state, message);
    loop {
        // The shell runs these off the UI thread; here, at once.
        let jobs = state.take_key_jobs();
        let ran = !jobs.is_empty();
        for (song, job) in jobs {
            let k = job.run();
            let answer = if song {
                Message::SongKey(k)
            } else {
                Message::ApiKey(k)
            };
            out = super::update(state, answer);
        }
        if let Outcome::Write(w) = out {
            out = super::update(state, Message::Written(w.run()));
        } else if !ran {
            return out;
        }
    }
}
use crate::ui::widgets::tests_support::{assert_tips_add_something, shown};
use crate::ui::widgets::{TIPS, shot};

#[test]
fn general_has_the_start_with_the_os_switch_on_by_default() {
    let path = tmp("autostart");
    let (mut s, _) = state(Some(path.clone()));
    assert!(s.draft.general.autostart);
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == START_LABEL), "{texts:?}");
    update(&mut s, Message::Autostart(false));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.problems);
    };
    assert!(!cfg.general.autostart);
    assert!(!Config::load_from(&path).0.general.autostart);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_browser_provider_says_it_opens_in_the_browser() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    update(&mut s, Message::Provider(Provider::Browser));
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t.contains("in your browser")),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t.contains("results card")));
}

#[test]
fn song_search_section_saves_a_write_only_token_and_its_choices() {
    let path = tmp("song");
    let (mut s, _) = state(Some(path.clone()));
    assert!(
        s.song_key.is_none(),
        "no keyring call before the section opens"
    );
    update(&mut s, Message::Section(Section::Song));
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == SONG_PRIVACY), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("dashboard.audd.io")));
    // Write-only: once saved, the token is never shown again.
    update(
        &mut s,
        Message::SongKey(KeyMessage::Input(Secret::new("FAKE-audd-1"))),
    );
    update(&mut s, Message::SongKey(KeyMessage::Save));
    assert_eq!(s.song_key.as_ref().unwrap().status, KeyStatus::Saved);
    assert_eq!(music::key_known(), Some(true), "the music button shows");
    let texts = shot::texts(view(&s));
    assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Saved in your keyring"));
    shot::save("settings-song-saved", view(&s), WINDOW_SIZE);
    update(&mut s, Message::SongKey(KeyMessage::Remove));
    assert_eq!(music::key_known(), Some(false), "and hides again");
    update(&mut s, Message::SongSource(SongSource::Microphone));
    update(&mut s, Message::SongHistory(true));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.problems);
    };
    assert_eq!(cfg.song.source, SongSource::Microphone);
    assert!(cfg.song.history);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Shares a `MemoryStore` between the test and the state.
struct Shared(Arc<MemoryStore>);
impl KeyStore for Shared {
    fn is_set(&self, p: Provider) -> Result<bool, SecretsError> {
        self.0.is_set(p)
    }
    fn set(&self, p: Provider, k: &Secret) -> Result<(), SecretsError> {
        self.0.set(p, k)
    }
    fn delete(&self, p: Provider) -> Result<(), SecretsError> {
        self.0.delete(p)
    }
}

pub fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oc-settings-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d.join("config.toml")
}

fn state(path: Option<PathBuf>) -> (State, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let s = State::new(
        Config::default(),
        vec![],
        path,
        Box::new(Shared(store.clone())),
    );
    (s, store)
}

fn chord(ctrl: bool, shift: bool, key: &str) -> Chord {
    Chord {
        ctrl,
        alt: false,
        shift,
        logo: false,
        key: key.into(),
    }
}

fn press(s: &mut State, c: Chord) {
    update(s, Message::Key(c));
}

#[test]
fn recording_validates_and_assigns() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Record(Slot::Quick(2)));
    assert_eq!(s.recording, Some(Slot::Quick(2)));
    // A lone modifier keeps waiting; nothing changes.
    press(&mut s, chord(true, false, "ControlLeft"));
    assert_eq!(s.recording, Some(Slot::Quick(2)));
    assert!(s.shortcut_error.is_none());
    press(&mut s, chord(true, true, "KeyK"));
    assert_eq!(s.draft.quick_actions[2].shortcut, "Ctrl+Shift+K");
    assert_eq!(s.recording, None);
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    // Digits and F-keys.
    update(&mut s, Message::Record(Slot::Capture));
    press(&mut s, chord(true, false, "Digit1"));
    assert_eq!(s.draft.shortcut, "Ctrl+1");
    update(&mut s, Message::Record(Slot::Quick(4)));
    press(&mut s, chord(false, false, "F9"));
    assert_eq!(s.draft.quick_actions[4].shortcut, "F9");
}

#[test]
fn invalid_keys_are_rejected_inline_and_escape_cancels() {
    let (mut s, _) = state(None);
    let before = s.draft.clone();
    update(&mut s, Message::Record(Slot::Capture));
    press(&mut s, chord(false, false, "KeyA"));
    let (slot, why) = s.shortcut_error.clone().unwrap();
    assert_eq!(slot, Slot::Capture);
    assert!(why.contains("modifier"), "{why}");
    assert_eq!(s.recording, Some(Slot::Capture), "stays open to retry");
    assert_eq!(s.draft, before);
    press(&mut s, chord(false, false, "Escape"));
    assert_eq!(s.recording, None);
    assert!(s.shortcut_error.is_none());
    assert_eq!(s.draft, before);
    // A key the parser does not know.
    update(&mut s, Message::Record(Slot::Capture));
    press(&mut s, chord(true, false, "NotARealKey"));
    assert!(s.shortcut_error.as_ref().unwrap().1.contains("unknown key"));
}

#[test]
fn conflicts_show_inline_and_do_not_assign() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    // Quick action 0 (Ctrl+Shift+T) vs the capture shortcut.
    update(&mut s, Message::Record(Slot::Quick(0)));
    press(&mut s, chord(true, true, "Space"));
    assert_eq!(
        s.shortcut_error,
        Some((Slot::Quick(0), "Already used by Open OpenCircle".into()))
    );
    assert_eq!(s.draft.quick_actions[0].shortcut, "Ctrl+Shift+T");
    // Quick vs quick: "Translate (live, Beta)" owns Ctrl+Shift+L.
    press(&mut s, chord(true, true, "KeyL"));
    assert_eq!(
        s.shortcut_error.as_ref().unwrap().1,
        "Already used by Translate (live, Beta)"
    );
    // Re-pressing its own combination is fine.
    press(&mut s, chord(true, true, "KeyT"));
    assert_eq!(s.recording, None);
}

#[test]
fn delete_unbinds_quick_actions_only() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Record(Slot::Quick(3)));
    press(&mut s, chord(false, false, "Delete"));
    assert_eq!(s.draft.quick_actions[3].shortcut, "");
    assert_eq!(s.recording, None);
    update(&mut s, Message::Record(Slot::Capture));
    press(&mut s, chord(false, false, "Delete"));
    assert_eq!(s.draft.shortcut, "Ctrl+Shift+Space");
    assert_eq!(s.recording, Some(Slot::Capture));
}

#[test]
fn keycaps_one_per_key() {
    assert_eq!(key_labels("Ctrl+Shift+Space"), ["Ctrl", "Shift", "Space"]);
    assert_eq!(key_labels("ctrl+shift+t"), ["Ctrl", "Shift", "T"]);
    assert_eq!(key_labels("Alt+PrintScreen"), ["Alt", "PrtSc"]);
    assert_eq!(key_labels("Super+KeyA"), ["Super", "A"]);
    assert!(key_labels("").is_empty());
}

fn page(s: &mut State, section: Section) -> Vec<String> {
    update(s, Message::Section(section));
    shot::texts(view(s))
}

fn sidebar(s: &State) -> Vec<&'static str> {
    pages::visible(&s.draft.labs).map(|p| p.title).collect()
}

#[test]
fn opens_at_a_size_that_fits_the_sidebar_and_the_content() {
    // Work area in logical px: the screen / scale, minus a 40 px taskbar.
    let work = |w: f32, h: f32, scale: f32| Size::new(w / scale, h / scale - 40.0);
    for (screen, scale) in [
        ((1920.0, 1080.0), 1.0),
        ((1920.0, 1080.0), 1.5),
        ((1280.0, 720.0), 1.0),
    ] {
        let w = work(screen.0, screen.1, scale);
        let (size, min) = window_limits(w);
        assert!(
            size.width <= w.width && size.height <= w.height,
            "{screen:?} {scale}: {size:?}"
        );
        assert!(min.width <= size.width && min.height <= size.height);
        assert!(
            size.height >= FIT_HEIGHT,
            "{screen:?} {scale}: {size:?} < {FIT_HEIGHT}"
        );
        assert!(size.width >= MIN_WIDTH, "{screen:?} {scale}: {size:?}");
    }
    // A huge screen does not make a huge window.
    assert_eq!(window_limits(work(3440.0, 1440.0, 1.0)).0, WINDOW_SIZE);
    // The header and every sidebar page are what FIT_HEIGHT counts.
    const { assert!(FIT_HEIGHT > 400.0 && FIT_HEIGHT <= WINDOW_SIZE.height) };
}

#[test]
fn hidden_features_have_no_settings_page_row_or_shortcut() {
    let (mut s, _) = state(None);
    let side = sidebar(&s);
    assert!(
        !side.contains(&"Snip") && !side.contains(&"Song search"),
        "{side:?}"
    );
    assert!(side.contains(&"Labs"));
    // No snip shortcut fields, no search-history row.
    let shortcuts = page(&mut s, Section::Shortcuts);
    assert!(
        !shortcuts
            .iter()
            .any(|t| t.contains("Snip") || t.contains("song")),
        "{shortcuts:?}"
    );
    let general = page(&mut s, Section::General);
    assert!(
        !general.iter().any(|t| t == "Search history"),
        "{general:?}"
    );
    // The labs page: one row per feature, all off.
    let labs = page(&mut s, Section::Labs);
    for f in Feature::ALL {
        assert!(labs.iter().any(|t| t == f.title()), "{labs:?}");
        assert!(labs.iter().any(|t| t == f.blurb()), "{labs:?}");
    }
    // The entries are still in the draft (and so in the file): hidden, not deleted.
    assert!(s.draft.quick_actions.iter().any(|q| q.action == "snip"));
}

#[test]
fn a_failed_write_keeps_the_queued_switches_and_the_unsaved_edit() {
    let path = tmp("write-fails");
    // A folder where the file should go: every write fails.
    std::fs::create_dir_all(&path).unwrap();
    let (mut s, _) = state(Some(path.clone()));
    s.auto = true;
    let Outcome::Write(first) = super::update(&mut s, Message::Theme(Theme::Light)) else {
        panic!("no write")
    };
    super::update(&mut s, Message::Labs(Feature::Snip, true));
    let out = super::update(&mut s, Message::Written(first.run()));
    assert!(
        matches!(out, Outcome::Write(_)),
        "the switch was dropped: {out:?}"
    );
    assert!(
        s.notice
            .as_deref()
            .is_some_and(|n| n.contains("Could not save"))
    );
    assert!(s.dirty(), "the edit stays unsaved, so auto-save retries it");
}

#[test]
fn writes_go_one_at_a_time_and_a_switch_during_a_write_waits_its_turn() {
    let path = tmp("write-queue");
    let (mut s, _) = state(Some(path.clone()));
    s.auto = true;
    // The save is handed out, not done: the UI thread never waits on the disk.
    let Outcome::Write(first) = super::update(&mut s, Message::Theme(Theme::Light)) else {
        panic!("no write")
    };
    assert!(!path.exists());
    // More edits while it is on its way: held, nothing else starts.
    assert_eq!(
        super::update(&mut s, Message::Labs(Feature::Snip, true)),
        Outcome::None
    );
    super::update(&mut s, Message::Autostart(false));
    let Outcome::Write(second) = super::update(&mut s, Message::Written(first.run())) else {
        panic!("the queued switch was not written")
    };
    let Outcome::Write(third) = super::update(&mut s, Message::Written(second.run())) else {
        panic!("the edit made during the write was not written")
    };
    let Outcome::Saved(cfg) = super::update(&mut s, Message::Written(third.run())) else {
        panic!("not saved: {:?}", s.notice)
    };
    let (back, _) = Config::load_from(&path);
    assert_eq!(back, cfg);
    assert!(back.theme == Theme::Light && back.labs.snip && !back.general.autostart);
}

#[test]
fn a_labs_switch_saves_and_applies_at_once_and_brings_its_rows_back() {
    let path = tmp("labs-switch");
    let (mut s, _) = state(Some(path.clone()));
    // An unsaved edit elsewhere is not saved by a Labs switch.
    update(&mut s, Message::Theme(Theme::Light));
    let Outcome::Saved(cfg) = update(&mut s, Message::Labs(Feature::Snip, true)) else {
        panic!("not saved: {:?}", s.notice)
    };
    assert!(cfg.labs.snip && !cfg.labs.song && cfg.theme == Theme::System);
    let (back, w) = Config::load_from(&path);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(back.labs, cfg.labs);
    assert_eq!(back.theme, Theme::System, "other draft edits stay unsaved");
    assert_eq!(s.draft.theme, Theme::Light);
    // Snip's page and its three shortcuts are back.
    assert!(sidebar(&s).contains(&"Snip"));
    let shortcuts = page(&mut s, Section::Shortcuts);
    for want in ["Snip a region", "Snip a window", "Snip the full screen"] {
        assert!(shortcuts.iter().any(|t| t == want), "{want}: {shortcuts:?}");
    }
    // History and Song likewise.
    update(&mut s, Message::Labs(Feature::History, true));
    update(&mut s, Message::Labs(Feature::Song, true));
    assert!(
        page(&mut s, Section::General)
            .iter()
            .any(|t| t == "Search history")
    );
    assert!(sidebar(&s).contains(&"Song search"));
    // Off again: gone again, and the page it was on is left.
    update(&mut s, Message::Section(Section::Snip));
    update(&mut s, Message::Labs(Feature::Snip, false));
    assert_eq!(s.section, Section::Labs);
    assert!(!sidebar(&s).contains(&"Snip"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_labs_write_changes_nothing() {
    let blocker = std::env::temp_dir().join(format!("oc-labs-blocker-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&blocker);
    std::fs::write(&blocker, "a file where the folder should be").unwrap();
    let (mut s, _) = state(Some(blocker.join("c.toml")));
    let out = update(&mut s, Message::Labs(Feature::Pins, true));
    assert!(matches!(out, Outcome::None));
    assert!(!s.draft.labs.pins);
    assert!(
        s.notice
            .as_deref()
            .is_some_and(|n| n.starts_with("Could not save"))
    );
    let _ = std::fs::remove_file(blocker);
}

/// #136: closing with unsaved changes asks; Save writes then leaves, Discard
/// leaves without writing, Keep editing stays; a clean window just closes.
#[test]
fn closing_with_unsaved_changes_asks_first() {
    let path = tmp("close-ask");
    let (mut s, _) = state(Some(path.clone()));
    assert!(s.close_requested(), "nothing changed: close");

    update(&mut s, Message::Theme(Theme::Light));
    assert!(!s.close_requested(), "unsaved changes: ask, don't close");
    assert!(s.asking_close && !s.leaving());
    update(&mut s, Message::CloseKeep);
    assert!(!s.asking_close && !s.leaving() && s.dirty());

    s.close_requested();
    update(&mut s, Message::Section(Section::About));
    assert!(!s.asking_close, "any other action is keep editing");

    s.close_requested();
    update(&mut s, Message::CloseDiscard);
    assert!(s.leaving() && !path.exists(), "discard writes nothing");

    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Theme(Theme::Light));
    s.close_requested();
    assert!(matches!(
        update(&mut s, Message::CloseSave),
        Outcome::Saved(_)
    ));
    assert!(s.leaving());
    assert_eq!(Config::load_from(&path).0.theme, Theme::Light);

    // A save refused for a problem stays open, still asking.
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    s.close_requested();
    assert_eq!(update(&mut s, Message::CloseSave), Outcome::None);
    assert!(!s.leaving() && s.asking_close);
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_file(&path);
}

/// Something else changes the saved config while Settings is open (the bar is
/// dragged): saving a theme change must keep it.
#[test]
fn saving_keeps_what_changed_elsewhere_meanwhile() {
    let path = tmp("keep-elsewhere");
    let (mut s, _) = state(Some(path.clone()));
    config::update_at(Some(&path), |c| c.bar.anchor = config::BarAnchor::Top).unwrap();
    update(&mut s, Message::Theme(Theme::Light));
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    let on_disk = Config::load_from(&path).0;
    assert_eq!(on_disk.theme, Theme::Light);
    assert_eq!(on_disk.bar.anchor, config::BarAnchor::Top);
    assert!(!s.dirty(), "{:?}", s.notice);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// #136: in the real window a valid change is on disk at once; one with a
/// problem is held back until it is fixed, and the pill's "Saved" fades.
#[test]
fn a_valid_change_saves_itself() {
    let path = tmp("autosave");
    let (mut s, _) = state(Some(path.clone()));
    s.auto = true;
    let Outcome::Saved(cfg) = update(&mut s, Message::Theme(Theme::Light)) else {
        panic!("not saved: {:?}", s.notice)
    };
    assert_eq!(cfg.theme, Theme::Light);
    assert_eq!(Config::load_from(&path).0.theme, Theme::Light);
    assert!(!s.dirty() && s.saved_at.is_some());
    assert!(
        s.close_requested(),
        "nothing unsaved: closes without asking"
    );

    // A change with a problem waits (and closing asks); fixing it saves.
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    assert!(s.dirty() && !s.problems.is_empty());
    assert_ne!(
        Config::load_from(&path).0.ai.provider,
        Provider::OpenaiCompatible
    );
    assert!(!s.close_requested());
    update(&mut s, Message::Preset(PRESETS[3]));
    update(&mut s, Message::Model("llama3:8b".into()));
    assert!(!s.dirty(), "{:?}", s.problems);
    assert_eq!(Config::load_from(&path).0.ai.model, "llama3:8b");

    // The pill's "Saved" ends after its moment.
    s.saved_at = Some(Instant::now() - SAVED_FOR);
    update(&mut s, Message::Tick);
    assert!(s.saved_at.is_none());
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_round_trips_through_a_temp_config() {
    let path = tmp("roundtrip");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Theme(Theme::Light));
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    update(&mut s, Message::Preset(PRESETS[3]));
    update(&mut s, Message::Model("llama3:8b".into()));
    update(&mut s, Message::Target(LANGUAGES[1]));
    update(&mut s, Message::Source(SOURCES[2]));
    update(&mut s, Message::DefaultAction(DefaultAction::Copy));
    update(&mut s, Message::Folder(Some(std::env::temp_dir())));
    update(&mut s, Message::LiveInterval("2500".into()));
    update(&mut s, Message::Engine(Engine::Offline));
    update(&mut s, Message::Record(Slot::Quick(4)));
    press(&mut s, chord(true, true, "KeyY"));
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    let (back, warnings) = Config::load_from(&path);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, cfg);
    assert_eq!(back.theme, Theme::Light);
    assert_eq!(back.ai.base_url, "http://localhost:11434/v1");
    assert_eq!(back.ai.model, "llama3:8b");
    assert_eq!(back.translate.target, "en");
    assert_eq!(back.translate.source, SOURCES[2].code);
    assert_eq!(back.translate.engine, Engine::Offline);
    assert_eq!(back.general.default_action, DefaultAction::Copy);
    assert_eq!(
        back.general.save_folder,
        std::env::temp_dir().to_string_lossy()
    );
    assert_eq!(back.translate.live_interval_ms, 2500);
    assert_eq!(back.quick_actions[4].shortcut, "Ctrl+Shift+Y");
    assert!(!s.dirty());
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn invalid_values_block_save_with_a_reason() {
    let path = tmp("invalid");
    let (mut s, _) = state(Some(path.clone()));
    // OpenAI-compatible with nothing filled in.
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    assert_eq!(
        s.draft.ai.model, "",
        "the Claude model id is not carried over"
    );
    assert!(issue(&s.problems, "ai.base_url").unwrap().contains("URL"));
    assert!(issue(&s.problems, "ai.model").is_some());
    assert_eq!(update(&mut s, Message::Save), Outcome::None);
    assert!(!path.exists());
    // Bad scheme: the config validator's own reason.
    update(&mut s, Message::BaseUrl("ftp://x".into()));
    update(&mut s, Message::Model("m".into()));
    assert!(
        issue(&s.problems, "ai.base_url")
            .unwrap()
            .contains("http(s)")
    );
    // Plain http to a remote host while a key is saved: the ai module's rule.
    update(&mut s, Message::BaseUrl("http://example.com/v1".into()));
    assert!(s.problems.is_empty(), "no key saved: {:?}", s.problems);
    update(&mut s, Message::ApiKey(KeyMessage::Input(Secret::new("k"))));
    update(&mut s, Message::ApiKey(KeyMessage::Save));
    assert!(
        issue(&s.problems, "ai.base_url")
            .unwrap()
            .contains("https://")
    );
    assert_eq!(update(&mut s, Message::Save), Outcome::None);
    assert!(!path.exists());
    update(&mut s, Message::BaseUrl("https://example.com/v1".into()));
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    assert!(path.exists());
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn switching_provider_keeps_the_model_sensible() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    update(&mut s, Message::Preset(PRESETS[0]));
    assert_eq!(
        s.draft.ai.model, "gpt-4o-mini",
        "preset fills an empty model"
    );
    update(&mut s, Message::Provider(Provider::Anthropic));
    assert_eq!(s.draft.ai.model, crate::ai::DEFAULT_ANTHROPIC_MODEL);
    assert_eq!(s.draft.ai.base_url, "", "Anthropic uses its own endpoint");
}

const FAKE_KEY: &str = "sk-ant-FAKE-0123456789";

#[test]
fn the_browser_provider_needs_no_key_model_or_server() {
    let (mut s, store) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    assert_eq!(s.draft.ai.provider, Provider::Browser, "the default");
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Provider"), "{texts:?}");
    for hidden in ["API key", "Model", "Server URL", "Preset"] {
        assert!(!texts.iter().any(|t| t == hidden), "{hidden}: {texts:?}");
    }
    // Back from a half-filled server form: hidden fields do not block Save.
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    update(&mut s, Message::Provider(Provider::Browser));
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(store.is_empty(), "no keyring entry for it");
}

#[test]
fn an_unknown_provider_blocks_save_until_one_is_picked() {
    let path = tmp("bad-provider");
    let (config, warnings) = Config::parse("[ai]\nprovider = \"antropic\"\n");
    let store = Box::new(MemoryStore::default());
    let mut s = State::new(config, warnings, Some(path.clone()), store);
    update(&mut s, Message::Section(Section::Ai));
    let texts = shot::texts(view(&s));
    assert!(
        texts
            .iter()
            .any(|t| t.contains("\"antropic\" is not browser")),
        "{texts:?}"
    );
    for hidden in ["API key", "Model", "Server URL"] {
        assert!(!texts.iter().any(|t| t == hidden), "{hidden}: {texts:?}");
    }
    // Save never writes "browser" (or anything) over it on its own.
    update(&mut s, Message::Save);
    assert!(!path.exists(), "saved with an unknown provider");
    update(&mut s, Message::Provider(Provider::Anthropic));
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("provider = \"anthropic\""), "{text}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_unreadable_file_keeps_ai_off_until_a_provider_is_picked() {
    let path = tmp("unreadable");
    let (config, warnings) = Config::parse("theme = = =");
    let store = Box::new(MemoryStore::default());
    let mut s = State::new(config, warnings, Some(path.clone()), store);
    let notice = s.notice.clone().unwrap();
    assert!(
        !notice.contains("reset") && notice.contains("AI is off"),
        "{notice}"
    );
    update(&mut s, Message::Section(Section::Ai));
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t.contains("couldn't be read")),
        "{texts:?}"
    );
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    update(&mut s, Message::Save);
    assert!(!path.exists(), "saved a guessed provider");
    update(&mut s, Message::Provider(Provider::Browser));
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("provider = \"browser\""), "{text}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A keyring that blocks `set` until released (or fails it).
struct Slow {
    open: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    fail: bool,
}
impl KeyStore for Slow {
    fn is_set(&self, _: Provider) -> Result<bool, SecretsError> {
        Ok(false)
    }
    fn set(&self, _: Provider, _: &Secret) -> Result<(), SecretsError> {
        let (lock, cv) = &*self.open;
        let (_open, timeout) = cv
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(10), |open| !*open)
            .unwrap();
        assert!(!timeout.timed_out(), "never released");
        match self.fail {
            true => Err(SecretsError::Failed("keyring refused".into())),
            false => Ok(()),
        }
    }
    fn delete(&self, _: Provider) -> Result<(), SecretsError> {
        Ok(())
    }
}

/// A state on the AI page with `Slow` keys and a fake key typed (built at run time).
fn slow_state(
    fail: bool,
) -> (
    State,
    Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    String,
) {
    let open: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> = Arc::default();
    let store = Slow {
        open: open.clone(),
        fail,
    };
    let mut s = State::new(Config::default(), vec![], None, Box::new(store));
    update(&mut s, Message::Provider(Provider::Anthropic));
    update(&mut s, Message::Section(Section::Ai));
    let fake = format!("test-{:x}", std::process::id());
    update(
        &mut s,
        Message::ApiKey(KeyMessage::Input(Secret::new(&fake))),
    );
    (s, open, fake)
}

fn release(open: &(std::sync::Mutex<bool>, std::sync::Condvar)) {
    *open.0.lock().unwrap() = true;
    open.1.notify_all();
}

#[test]
fn a_slow_keyring_never_blocks_update_and_the_answer_sets_saved() {
    let (mut s, open, fake) = slow_state(false);
    let t = Instant::now();
    super::update(&mut s, Message::ApiKey(KeyMessage::Save));
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "update waited for the keyring"
    );
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Saving…"), "{texts:?}");
    assert_eq!(*s.key.status(), KeyStatus::NotSet, "not saved yet");
    assert!(!format!("{s:?}{texts:?}").contains(&fake), "the key leaked");
    let mut jobs = s.take_key_jobs();
    assert_eq!(jobs.len(), 1);
    let (_, job) = jobs.pop().unwrap();
    let worker = std::thread::spawn(move || job.run());
    release(&open);
    super::update(&mut s, Message::ApiKey(worker.join().unwrap()));
    assert_eq!(*s.key.status(), KeyStatus::Saved);
    let texts = shot::texts(view(&s));
    assert!(!texts.iter().any(|t| t == "Saving…"), "{texts:?}");
    assert!(
        texts.iter().any(|t| t == "Saved in your keyring"),
        "{texts:?}"
    );
}

#[test]
fn a_failed_save_shows_the_error_and_keeps_the_field() {
    let (mut s, open, fake) = slow_state(true);
    super::update(&mut s, Message::ApiKey(KeyMessage::Save));
    let (_, job) = s.take_key_jobs().pop().unwrap();
    release(&open);
    super::update(&mut s, Message::ApiKey(job.run()));
    assert_eq!(*s.key.status(), KeyStatus::NotSet);
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t.contains("keyring refused")),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t == "Saving…"), "{texts:?}");
    assert_eq!(
        s.key.input.expose(),
        fake,
        "the typed key stays for a retry"
    );
}

#[test]
fn the_api_key_is_write_only() {
    let (mut s, store) = state(None);
    update(&mut s, Message::Provider(Provider::Anthropic));
    let typing = Message::ApiKey(KeyMessage::Input(Secret::new(FAKE_KEY)));
    assert!(!format!("{typing:?}").contains("FAKE"), "message Debug");
    update(&mut s, typing);
    assert!(
        !format!("{s:?}").contains("FAKE"),
        "state Debug while typing"
    );
    update(&mut s, Message::Section(Section::Ai));
    update(&mut s, Message::ApiKey(KeyMessage::Save));
    assert_eq!(
        store.get("anthropic").as_deref(),
        Some(FAKE_KEY),
        "went to the keyring"
    );
    assert_eq!(*s.key.status(), KeyStatus::Saved);
    // After saving no view text and no Debug output holds the key.
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t == "Saved in your keyring"),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
    assert!(!format!("{s:?}").contains("FAKE"));
    assert!(!format!("{:?}", s.key).contains("FAKE"));
    // Replace opens an empty field (never prefilled); Remove deletes.
    update(&mut s, Message::ApiKey(KeyMessage::Replace));
    let texts = shot::texts(view(&s));
    assert!(!texts.iter().any(|t| t.contains("FAKE")), "{texts:?}");
    update(&mut s, Message::ApiKey(KeyMessage::Cancel));
    update(&mut s, Message::ApiKey(KeyMessage::Remove));
    assert!(store.is_empty());
    assert_eq!(*s.key.status(), KeyStatus::NotSet);
    // The key never reaches the config file either.
    let path = tmp("nokey");
    update(&mut s, Message::Theme(Theme::Dark));
    update(&mut s, Message::Theme(Theme::Light));
    s.path = Some(path.clone());
    update(&mut s, Message::Save);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("FAKE") && !text.contains("api_key"),
        "{text}"
    );
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn every_control_has_its_own_label() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    let mut all = Vec::new();
    for page in &PAGES {
        update(&mut s, Message::Section(page.id));
        TIPS.with(|t| t.borrow_mut().clear());
        let _ = view(&s);
        all.extend(TIPS.with(|t| t.borrow().clone()));
    }
    // (The sidebar items and segment pills are named by their own text.)
    all.sort();
    all.retain(|t| t != "Save settings");
    let mut unique = all.clone();
    unique.dedup();
    assert_eq!(all, unique, "duplicate control labels");
    assert!(all.len() >= 22, "{all:?}");
}

#[test]
fn no_visible_tooltip_repeats_its_control() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    for page in &PAGES {
        update(&mut s, Message::Section(page.id));
        let _ = view(&s);
        assert_tips_add_something(page.title);
    }
    // Text that says what it is shows no tooltip: Settings has none by default.
    update(&mut s, Message::Section(Section::General));
    let _ = view(&s);
    assert_eq!(shown().len(), 0, "{:?}", shown());
    // A save folder cut short does: the whole path.
    let long = "/a".repeat(40);
    update(&mut s, Message::Folder(Some(PathBuf::from(&long))));
    let _ = view(&s);
    assert_eq!(shown(), [long]);
    assert_tips_add_something("a cut folder");
}

#[test]
fn general_section_edits_default_action_and_save_folder() {
    let path = tmp("general");
    let (mut s, _) = state(Some(path.clone()));
    assert_eq!(s.section, Section::General, "the first section opens");
    assert_eq!(s.draft.general.default_action, DefaultAction::Lens);
    update(&mut s, Message::DefaultAction(DefaultAction::Text));
    // Browse asks the shell for the picker; nothing changes until it answers.
    assert_eq!(update(&mut s, Message::BrowseFolder), Outcome::Browse);
    assert_eq!(s.draft.general.save_folder, "");
    update(&mut s, Message::Folder(None));
    assert_eq!(s.draft.general.save_folder, "", "cancelled");
    let dir = std::env::temp_dir();
    update(&mut s, Message::Folder(Some(dir.clone())));
    assert_eq!(s.draft.general.save_folder, dir.to_string_lossy());
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    assert_eq!(cfg.general.default_action, DefaultAction::Text);
    assert_eq!(
        Config::load_from(&path).0.general.save_folder,
        dir.to_string_lossy()
    );
    update(&mut s, Message::ClearFolder);
    assert_eq!(s.draft.general.save_folder, "");
    // The control shows the choice (a long path is cut in the middle).
    update(
        &mut s,
        Message::Folder(Some(PathBuf::from("/a".repeat(40)))),
    );
    let texts = shot::texts(view(&s));
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with('…') && t.chars().count() == 38),
        "{texts:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn live_interval_is_checked_against_its_range() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Translate));
    assert_eq!(s.interval, "1500");
    for ok in ["500", "1000", "5000", " 2500 "] {
        update(&mut s, Message::LiveInterval(ok.into()));
        assert!(s.problems.is_empty(), "{ok}: {:?}", s.problems);
    }
    for bad in ["499", "5001", "", "fast", "-3"] {
        update(&mut s, Message::LiveInterval(bad.into()));
        let why = issue(&s.problems, "translate.live_interval_ms");
        assert!(why.is_some_and(|w| w.contains("500 to 5000")), "{bad}");
        assert_eq!(s.interval, bad, "the field keeps what was typed");
        assert_eq!(update(&mut s, Message::Save), Outcome::None, "{bad}");
    }
    assert_eq!(*crate::config::LIVE_INTERVAL_MS.start(), 500);
}

#[test]
fn engine_choice_says_what_offline_means() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Translate));
    let texts = shot::texts(view(&s));
    for want in [
        "Engine",
        "AI",
        "Offline",
        "Your AI provider; offline when it isn't set up",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    update(&mut s, Message::Engine(Engine::Offline));
    let texts = shot::texts(view(&s));
    let honest = "No key, nothing leaves your PC; more literal than AI";
    assert!(texts.iter().any(|t| t == honest), "{texts:?}");
    // Source and Target; a pair with no offline model says so
    for want in ["Source language", "Target language"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let gap = "No offline model for this pair: choose AI to translate it";
    assert!(!texts.iter().any(|t| t == gap));
    update(&mut s, Message::Source(lang::find("fr").copied().unwrap()));
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t == gap),
        "fr -> ar offline: {texts:?}"
    );
    shot::save("settings-translate", view(&s), Size::new(860.0, 600.0));
    // a saved "offline" is read as is, without a warning
    let (cfg, warnings) = Config::parse("[translate]\nengine = \"offline\"\n");
    let s = State::new(cfg, warnings, None, Box::new(MemoryStore::default()));
    assert_eq!(s.draft.translate.engine, Engine::Offline);
    assert!(s.notice.is_none(), "{:?}", s.notice);
}

#[test]
fn snip_section_edits_the_snip_choices() {
    let dir = tmp("snip-section");
    let path = dir.join("config.toml");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Section(Section::Snip));
    let texts = shot::texts(view(&s));
    for label in [
        "After a capture",
        "Image size",
        "Save as",
        "Recent captures",
    ] {
        assert!(texts.iter().any(|t| t == label), "{label}: {texts:?}");
    }
    let mut snip = s.draft.snip.clone();
    assert!(!snip.history, "off until the user turns it on");
    snip.after = SnipAfter::Both;
    snip.max_edge = 0;
    snip.format = Format::Jpeg;
    snip.history = true;
    snip.delay = 5;
    update(&mut s, Message::Snip(snip.clone()));
    assert_eq!(s.draft.snip, snip);
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    let (back, w) = Config::load_from(&path);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(back.snip, snip);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn general_switches_updates_and_checks_now() {
    let path = tmp("update");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Section(Section::General));
    assert_eq!(update(&mut s, Message::CheckUpdate), Outcome::CheckUpdate);
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Checking…"));
    update(
        &mut s,
        Message::UpdateStatus("OpenCircle 1.0.0 is up to date".into()),
    );
    assert!(
        shot::texts(view(&s))
            .iter()
            .any(|t| t == "OpenCircle 1.0.0 is up to date")
    );
    update(&mut s, Message::AutoUpdate(false));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("saved");
    };
    assert_eq!(cfg.update.auto, Some(false));
    assert_eq!(Config::load_from(&path).0.update.auto, Some(false));
}

#[test]
fn about_shows_version_license_project_and_no_telemetry() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::About));
    let texts = shot::texts(view(&s));
    for want in ["MIT licence", "Telemetry", "None", "Amer Alreyahi"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(texts.contains(&version), "{texts:?}");
    // Nothing opens until a link is clicked; the click asks the shell to open it.
    let url = pages::about::PROJECT.to_string();
    assert_eq!(
        update(&mut s, Message::OpenLink(url.clone())),
        Outcome::OpenLink(url)
    );
    assert!(!s.dirty(), "About changes nothing");
    for want in ["Copy diagnostics", "Report a problem"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert_eq!(
        update(&mut s, Message::CopyDiagnostics),
        Outcome::CopyDiagnostics
    );
    let Outcome::OpenLink(url) = update(&mut s, Message::ReportProblem) else {
        panic!("Report a problem opens a link");
    };
    assert!(url.starts_with("https://github.com/AmerAlreyahi/OpenCircle/issues/new?"));
}

#[test]
fn shortcuts_show_the_key_the_desktop_actually_uses() {
    let (mut s, _) = state(None);
    s.desktop = |keys| match keys {
        "Ctrl+Shift+Space" => Some(DesktopKey::Other("Super+O".into())),
        "Ctrl+Shift+E" => Some(DesktopKey::Unassigned),
        _ => None,
    };
    update(&mut s, Message::Section(Section::Shortcuts));
    let notes = |s: &State| -> Vec<String> {
        let texts = shot::texts(view(s));
        texts
            .into_iter()
            .filter(|t| t.ends_with(managed()))
            .collect()
    };
    assert_eq!(
        notes(&s),
        [
            format!("In use: Super+O. {}", managed()),
            format!("No key in use. {}", managed()),
        ]
    );
    // A key changed but not saved yet: the desktop only knows the saved one.
    update(&mut s, Message::Record(Slot::Capture));
    assert_eq!(notes(&s).len(), 1, "no note while recording");
    press(&mut s, chord(true, true, "F7"));
    assert_eq!(s.draft.shortcut, "Ctrl+Shift+F7");
    assert_eq!(notes(&s), [format!("No key in use. {}", managed())]);
    // Nothing to say (not Wayland, or the desktop uses our keys): no note at all.
    s.desktop = |_| None;
    assert!(notes(&s).is_empty());
}

#[test]
fn snip_shortcuts_are_listed_and_editable() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    update(&mut s, Message::Section(Section::Shortcuts));
    let texts = shot::texts(view(&s));
    for want in ["Snip a region", "Snip a window", "Snip the full screen"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let names: Vec<_> = s.quick_text.iter().map(|t| t.label.as_str()).collect();
    for want in ["Snip a region", "Snip a window", "Snip the full screen"] {
        assert!(names.contains(&want), "{names:?}");
    }
    let i = s
        .draft
        .quick_actions
        .iter()
        .position(|q| q.shortcut == "Alt+PrintScreen")
        .unwrap();
    update(&mut s, Message::Record(Slot::Quick(i)));
    press(&mut s, chord(false, false, "F8"));
    assert_eq!(s.draft.quick_actions[i].shortcut, "F8");
    // Print Screen is a valid lone key but each key still does one job.
    update(&mut s, Message::Record(Slot::Quick(i)));
    let key = Chord {
        ctrl: false,
        alt: false,
        shift: false,
        logo: false,
        key: "PrintScreen".into(),
    };
    press(&mut s, key);
    assert!(
        s.shortcut_error
            .as_ref()
            .unwrap()
            .1
            .starts_with("Already used")
    );
}

#[test]
fn look_and_behaviour_edits_the_selection_and_the_custom_look() {
    let path = tmp("look");
    let (mut s, _) = state(Some(path.clone()));
    let behaviour = page(&mut s, Section::Look);
    for want in ["Snap to box", "Open Appearance →"] {
        assert!(behaviour.iter().any(|t| t == want), "{want}: {behaviour:?}");
    }
    assert!(
        !behaviour.iter().any(|t| t == "Spectrum"),
        "the looks are in Appearance"
    );
    update(&mut s, Message::Section(Section::Appearance));
    let texts = shot::texts(view(&s));
    for want in [
        "Active look screen borders",
        "Spectrum",
        "Cyber Neon",
        "Liquid Prism",
        "Stealth Frost",
        "Custom",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert!(
        !texts.iter().any(|t| t == "Edge style"),
        "a preset has no custom rows"
    );
    update(&mut s, Message::SelectStyle(SelectStyle::Shape));
    update(&mut s, Message::After(After::Wait));
    update(&mut s, Message::SearchWith(SearchWith::Both));
    // Custom starts as the look in use (Neon here), then changes on its own
    update(&mut s, Message::Look(LookTheme::Neon));
    update(&mut s, Message::Look(LookTheme::Custom));
    assert_eq!(
        s.draft.look.custom,
        LookTheme::Neon.look(&s.draft.look.custom)
    );
    update(&mut s, Message::Glow(55));
    update(&mut s, Message::Dim(30));
    update(&mut s, Message::EdgeMode(EdgeMode::Live));
    update(&mut s, Message::EdgeStyle(EdgeStyle::None));
    update(&mut s, Message::Bar(BarChoice(BarStyle::Solid)));
    update(
        &mut s,
        Message::Palette(PaletteChoice(ColourSet::TwoColour)),
    );
    update(&mut s, Message::Picker(1));
    update(&mut s, Message::Hex("#12345".into())); // not a colour yet: kept as typed
    update(&mut s, Message::Hex("#123456".into()));
    update(&mut s, Message::Picker(0));
    update(&mut s, Message::PickColour(Color::from_rgb8(255, 0, 0)));
    let texts = shot::texts(view(&s));
    for want in [
        "Edge style",
        "Glow strength",
        "Your two colours",
        "Bars and pills",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    let (back, w) = Config::load_from(&path);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(back, cfg);
    let look = back.look.active();
    assert_eq!((look.glow, look.dim, look.bar), (55, 30, BarStyle::Solid));
    assert_eq!(look.live.style, EdgeStyle::None);
    assert_eq!(
        look.overlay,
        LookTheme::Neon.look(&look).overlay,
        "other modes kept"
    );
    assert_eq!(look.colours, [Rgb([255, 0, 0]), Rgb([0x12, 0x34, 0x56])]);
    assert_eq!(back.selection.search_with, SearchWith::Both);
    // the page in both themes, a preset and the custom look
    for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        update(&mut s, Message::Theme(theme_choice));
        assert_eq!(s.picker, Some(0), "the picker is open");
        shot::save(
            &format!("settings-look-custom-{name}"),
            view(&s),
            Size::new(820.0, 1640.0),
        );
    }
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_text_reading_page_is_windows_only_and_saves() {
    let path = tmp("reader");
    let (mut s, _) = state(Some(path.clone()));
    // listed on Windows only (Linux has the one reader); the page holds the engine
    assert_eq!(sidebar(&s).contains(&"Text reading"), cfg!(windows));
    let texts = page(&mut s, Section::TextReading);
    for want in [
        "Engine",
        "Auto",
        "Fast (Windows built-in)",
        "Accurate (PaddleOCR)",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let look = page(&mut s, Section::Look);
    assert!(!look.iter().any(|t| t == "Engine"), "moved: {look:?}");
    update(&mut s, Message::TextReader(TextReader::PpOcr));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    assert_eq!(cfg.selection.text_reader, TextReader::PpOcr);
    assert_eq!(
        Config::load_from(&path).0.selection.text_reader,
        TextReader::PpOcr
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_missing_recognizer_gets_one_quiet_note() {
    let (mut s, _) = state(None);
    s.recognizers = vec!["en-US".into()];
    s.profile = Some("en-GB".into());
    assert_eq!(s.reader_note(), None);
    s.draft.translate.source = "ja".into();
    let note = "Windows has no text recognition for Japanese. Using the downloaded reader.";
    assert_eq!(s.reader_note().as_deref(), Some(note));
    // the note and its button are in the engine row
    let texts = page(&mut s, Section::TextReading);
    assert!(texts.iter().any(|t| t == note), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Add in Windows"));
    // the downloaded reader chosen: nothing to say
    s.draft.selection.text_reader = TextReader::PpOcr;
    assert_eq!(s.reader_note(), None);
    // a display language with no recognizer counts too
    s.draft.selection.text_reader = TextReader::Auto;
    s.draft.translate.source = "auto".into();
    s.profile = Some("de-DE".into());
    assert!(s.reader_note().unwrap().contains("German"));
}

#[test]
fn renders_about_in_both_modes() {
    for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        let (mut s, _) = state(None);
        update(&mut s, Message::Theme(theme_choice));
        update(&mut s, Message::Section(Section::About));
        shot::save(&format!("about-{name}"), view(&s), Size::new(820.0, 1450.0));
    }
    theme::apply(Theme::Dark);
}

/// Every shortcut box is the same size, however many keys it holds or what
/// state it is in (owner, 2026-10-23: every shortcut field the same width).
#[test]
fn every_shortcut_field_is_the_same_size() {
    use crate::ui::kit::shortcut_field::{State as Field, shortcut_field};
    use crate::ui::kit::tokens::size;
    let keys = |s: &str| key_labels(s);
    for (shortcut, field) in [
        ("", Field::Rest),
        ("F9", Field::Rest),
        ("Ctrl+Shift+Space", Field::Rest),
        ("Ctrl+Alt+Shift+K", Field::Rest),
        ("Ctrl+C", Field::Recording),
        ("Ctrl+Shift+T", Field::Conflict),
    ] {
        let e: Element<'_, Message> =
            shortcut_field(keys(shortcut), field, "Press keys…", Message::Save);
        let got = e.as_widget().size();
        assert_eq!(
            (got.width, got.height),
            (
                Length::Fixed(size::SHORTCUT.width),
                Length::Fixed(size::SHORTCUT.height)
            ),
            "{shortcut:?} {field:?}"
        );
    }
}

/// The shortcut page draws one keycap per key: "Ctrl+Shift+Space" is three
/// caps and two keys leave one dimmed "—" cap at the left (the design's box).
#[test]
fn shortcut_boxes_pad_to_three_caps_from_the_left() {
    let (mut s, _) = state(None);
    s.draft.quick_actions[0].shortcut = "Ctrl+T".into();
    let texts = page(&mut s, Section::Shortcuts);
    let at = texts.iter().position(|t| t == "Ctrl").unwrap();
    assert_eq!(texts[at..at + 3], ["Ctrl", "Shift", "Space"]);
    let two = texts.iter().position(|t| t == "—").expect("an empty cap");
    assert_eq!(texts[two..two + 3], ["—", "Ctrl", "T"]);
    // a cleared quick action says so
    s.draft.quick_actions[0].shortcut.clear();
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Not set"), "{texts:?}");
}

#[test]
fn save_is_never_a_dead_button() {
    let (mut s, _) = state(None);
    assert_eq!(update(&mut s, Message::Save), Outcome::None);
    assert_eq!(s.notice.as_deref(), Some("Nothing to save"));
}

#[test]
fn renders_every_section_in_both_themes() {
    for (name, theme_choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        let (mut s, _) = state(None);
        s.draft.labs = Labs::all_on();
        update(&mut s, Message::Theme(theme_choice));
        update(&mut s, Message::Provider(Provider::OpenaiCompatible));
        update(&mut s, Message::Preset(PRESETS[0]));
        for page in &PAGES {
            let (section, label) = (page.id, page.title);
            update(&mut s, Message::Section(section));
            if section == Section::Shortcuts {
                update(&mut s, Message::Record(Slot::Quick(1)));
                shot::save(
                    &format!("settings-shortcuts-recording-{name}"),
                    view(&s),
                    WINDOW_SIZE,
                );
                press(&mut s, chord(true, true, "Space"));
            }
            let slug = label.to_lowercase().replace(" & ", "-");
            let file = format!("settings-{slug}-{name}");
            let px = shot::save(&file, view(&s), WINDOW_SIZE);
            let window = theme::current().window;
            // Not blank: more than the window colour is on screen.
            let bg = [window.r, window.g, window.b].map(|c| (c * 255.0).round() as u8);
            let other = px
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] != bg)
                .count();
            assert!(other > 5_000, "{file} looks empty ({other})");
        }
        // General in full: Save folder and Check now sit below the fold.
        update(&mut s, Message::Section(Section::General));
        shot::save(
            &format!("settings-general-tall-{name}"),
            view(&s),
            Size::new(820.0, 900.0),
        );
        // The error state of the shortcut field (conflict) and an invalid URL.
        update(&mut s, Message::Section(Section::Ai));
        update(&mut s, Message::BaseUrl("ftp://nope".into()));
        shot::save(
            &format!("settings-ai-invalid-{name}"),
            view(&s),
            WINDOW_SIZE,
        );
        update(&mut s, Message::Section(Section::Ai));
        update(&mut s, Message::Provider(Provider::Anthropic));
        update(&mut s, Message::ApiKey(KeyMessage::Input(Secret::new("k"))));
        update(&mut s, Message::ApiKey(KeyMessage::Save));
        shot::save(&format!("settings-ai-claude-{name}"), view(&s), WINDOW_SIZE);
    }
    theme::apply(Theme::Dark);
}

/// The Labs rule: Snip and Song search are listed only while their switch is
/// on, and switching one off on its own page goes back to Labs (it was Snip
/// only: Song search stayed on a page the sidebar no longer lists).
#[test]
fn labs_pages_are_hidden_while_their_switch_is_off() {
    let path = tmp("labs-pages");
    let (mut s, _) = state(Some(path.clone()));
    let mut side = sidebar(&s);
    if cfg!(windows) {
        assert_eq!(side.remove(4), "Text reading");
    }
    assert_eq!(
        side,
        [
            "General",
            "Appearance",
            "Look & behaviour",
            "AI",
            "Translate",
            "Shortcuts",
            "Privacy",
            "About",
            "Labs"
        ]
    );
    for (f, page) in [(Feature::Snip, "Snip"), (Feature::Song, "Song search")] {
        update(&mut s, Message::Labs(f, true));
        assert!(sidebar(&s).contains(&page), "{page} on");
        let id = PAGES.iter().find(|p| p.title == page).unwrap().id;
        update(&mut s, Message::Section(id));
        update(&mut s, Message::Labs(f, false));
        assert!(!sidebar(&s).contains(&page), "{page} off");
        assert_eq!(s.section, Section::Labs, "{page}: back to Labs");
        // the pill says the switch was saved
        assert!(s.saved_at.is_some());
        assert!(shot::texts(view(&s)).iter().any(|t| t == "Saved"));
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A page change glides the sidebar's highlight and raises the page on the
/// design's timings, by the frame clock, which stops once both are done.
#[test]
fn a_page_change_glides_and_rises_then_the_clock_stops() {
    let (mut s, _) = state(None);
    assert!(s.moving().is_none());
    super::update(&mut s, Message::Section(Section::Labs));
    let (from, glide, rise) = s.moving().expect("moving");
    assert_eq!(from, Section::General);
    assert!(glide < 1.0 && rise < 1.0, "{glide} {rise}");
    // the same page again is no motion
    s.moved = None;
    super::update(&mut s, Message::Section(Section::Labs));
    assert!(s.moving().is_none());
    // done: the next tick ends it
    s.moved = Some((Section::General, Instant::now() - motion::GLIDE));
    let (_, glide, rise) = s.moving().unwrap();
    assert_eq!((glide, rise), (1.0, 1.0));
    super::update(&mut s, Message::Tick);
    assert!(s.moved.is_none());
    // a pick glides its thumb from the old value
    super::update(&mut s, Message::Autostart(false));
    let (from, glide) = s.thumb(false, |p| match p {
        Pick::Autostart(v) => Some(v),
        _ => None,
    });
    assert!(from && glide < 1.0, "{from} {glide}");
}

/// The header's buttons are the window's: they never reach the settings logic.
#[test]
fn the_header_reports_its_caption_buttons() {
    let (s, _) = state(None);
    let size = WINDOW_SIZE;
    let click = |x: f32| {
        let events = shot::click(view(&s), size, iced::Point::new(x, 20.0));
        events.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>()
    };
    assert_eq!(click(size.width - 23.0), ["Caption(Close)"]);
    assert_eq!(click(size.width - 69.0), ["Caption(Maximise)"]);
    assert_eq!(click(size.width - 115.0), ["Caption(Minimise)"]);
    assert_eq!(click(100.0), ["Caption(Drag)"]);
    let (mut s, _) = state(None);
    assert_eq!(
        update(&mut s, Message::Caption(Caption::Close)),
        Outcome::None
    );
    assert!(!s.dirty());
}

/// The ported pages (General, About, Labs) as drawn, in Sapphire and
/// Daylight; General with a Latin and an Arabic save folder.
#[test]
fn the_ported_pages_match_their_golden_images() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        let cases: [(&str, Section, &str); 4] = [
            ("general", Section::General, "/home/me/Pictures/OpenCircle"),
            ("general-arabic", Section::General, "/home/me/صور/لقطات"),
            ("about", Section::About, ""),
            ("labs", Section::Labs, ""),
        ];
        for (name, section, folder) in cases {
            let (mut s, _) = state(None);
            s.draft.labs.song = true;
            update(&mut s, Message::Section(section));
            if !folder.is_empty() {
                update(&mut s, Message::Folder(Some(PathBuf::from(folder))));
            }
            themes::set(theme);
            let file = format!("settings-{name}-{}", theme.id);
            failed.extend(golden::check("screens", &file, WINDOW_SIZE, &[1.0], || {
                view(&s)
            }));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The pages of the UI plan's step 33 as drawn, in Sapphire and Daylight:
/// AI (a server of your own; a saved key), Text reading, Translate (Live on),
/// Shortcuts at rest, recording and with a conflict, Snip, Song search,
/// Privacy, and Appearance's custom look rows with a colour picker open.
#[test]
fn the_step_33_pages_match_their_golden_images() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    type Setup = fn(&mut State);
    let cases: [(&str, Section, f32, Setup); 11] = [
        ("ai", Section::Ai, 0.0, |s| {
            update(s, Message::Provider(Provider::OpenaiCompatible));
            update(s, Message::Preset(PRESETS[1]));
        }),
        ("ai-key-saved", Section::Ai, 0.0, |s| {
            update(s, Message::Provider(Provider::Anthropic));
            update(s, Message::ApiKey(KeyMessage::Input(Secret::new("k"))));
            update(s, Message::ApiKey(KeyMessage::Save));
        }),
        ("text-reading", Section::TextReading, 0.0, |_| {}),
        ("translate", Section::Translate, 0.0, |_| {}),
        ("shortcuts", Section::Shortcuts, 0.0, |_| {}),
        ("shortcuts-recording", Section::Shortcuts, 0.0, |s| {
            update(s, Message::Record(Slot::Quick(1)));
        }),
        ("shortcuts-conflict", Section::Shortcuts, 0.0, |s| {
            update(s, Message::Record(Slot::Quick(0)));
            press(s, chord(true, true, "Space"));
        }),
        ("snip", Section::Snip, 0.0, |_| {}),
        ("song", Section::Song, 0.0, |_| {}),
        ("privacy", Section::Privacy, 0.0, |_| {}),
        ("appearance-custom", Section::Appearance, 1500.0, |s| {
            update(s, Message::Look(LookTheme::Custom));
            update(s, Message::Palette(PaletteChoice(ColourSet::TwoColour)));
            update(s, Message::Picker(0));
        }),
    ];
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        for (name, section, height, setup) in cases {
            let (mut s, _) = state(None);
            s.draft.labs = Labs::all_on();
            s.recognizers = vec!["en-US".into()];
            update(&mut s, Message::Section(section));
            setup(&mut s);
            themes::set(theme);
            let size = Size::new(WINDOW_SIZE.width, height.max(WINDOW_SIZE.height));
            let file = format!("settings-{name}-{}", theme.id);
            failed.extend(golden::check("screens", &file, size, &[1.0], || view(&s)));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A theme card is its own mode's pick. When the other mode is showing, the OS
/// mode switches with it; the app re-themes at once, before the write.
#[test]
fn a_theme_card_picks_for_its_mode_and_shows_at_once() {
    use crate::ui::kit::themes;
    let path = tmp("themes");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Theme(Theme::Dark));
    update(&mut s, Message::ThemeCard("emerald"));
    assert_eq!(s.draft.appearance.dark, "emerald");
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::Dark, "emerald")
    );
    // a light card while dark shows: light mode, the dark pick kept
    update(&mut s, Message::ThemeCard("sandstone"));
    assert_eq!(s.draft.appearance.light, "sandstone");
    assert_eq!(s.draft.appearance.dark, "emerald");
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::Light, "sandstone")
    );
    // System on a light OS: a light card keeps System, a dark one switches to Dark
    theme::set_os(iced::theme::Mode::Light);
    update(&mut s, Message::Theme(Theme::System));
    update(&mut s, Message::ThemeCard("frost"));
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::System, "frost")
    );
    update(&mut s, Message::ThemeCard("midnight"));
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::Dark, "midnight")
    );
    theme::set_os(iced::theme::Mode::Dark);
    // the OS mode alone shows that mode's pick
    update(&mut s, Message::Theme(Theme::Light));
    assert_eq!(themes::current().id, "frost");
    update(&mut s, Message::Logo(config::Logo::Purple));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    let back = Config::load_from(&path).0;
    assert_eq!(back, cfg);
    let a = &back.appearance;
    assert_eq!((a.dark.as_str(), a.light.as_str()), ("midnight", "frost"));
    assert_eq!((back.theme, a.logo), (Theme::Light, config::Logo::Purple));
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The look cards write the stored names config v4 keeps: Prism as "aurora",
/// Frost as "mono", Aurora Borealis as "borealis"; picking the shown card
/// again keeps "calm".
#[test]
fn look_cards_write_the_stored_look_names() {
    use crate::ui::kit::looks;
    let (mut s, _) = state(None);
    s.draft.look.theme = LookTheme::Calm;
    update(&mut s, Message::Section(Section::Appearance));
    let texts = shot::texts(view(&s));
    for l in looks::LOOKS {
        assert!(texts.iter().any(|t| t == l.name), "{}: {texts:?}", l.name);
    }
    let of = |s: &State, id: &str| {
        let picks = pages::look_picks(s);
        picks.into_iter().find(|(l, _)| *l == id).unwrap().1
    };
    assert_eq!(
        of(&s, "prism"),
        Some(LookTheme::Calm),
        "the shown card keeps calm"
    );
    assert_eq!(of(&s, "frost"), Some(LookTheme::Mono));
    assert_eq!(of(&s, "horizon"), Some(LookTheme::Horizon));
    assert_eq!(of(&s, "aurora"), Some(LookTheme::Borealis));
    s.draft.look.theme = LookTheme::Spectrum;
    assert_eq!(of(&s, "prism"), Some(LookTheme::Aurora));
    assert_eq!(of(&s, "spectrum"), Some(LookTheme::Spectrum));
}

/// Every theme card draws its three swatch dots.
#[test]
fn every_theme_card_renders_its_swatches() {
    use crate::ui::kit::card::theme_card;
    use crate::ui::kit::themes::{self, SAPPHIRE, THEMES};
    for t in THEMES {
        themes::set(t);
        let px = shot::render(theme_card::<()>(t, false, ()), 200, 48);
        for dot in t.swatch {
            let [r, g, b, _] = dot.into_rgba8();
            let near = |p: &[u8]| p.iter().zip([r, g, b]).all(|(&a, b)| a.abs_diff(b) <= 6);
            assert!(px.chunks(4).any(near), "{}: no {dot:?} dot", t.id);
        }
    }
    themes::set(&SAPPHIRE);
}

/// Appearance and Look & behaviour as drawn, in Sapphire and Daylight.
#[test]
fn appearance_and_look_match_their_golden_images() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        for (name, section) in [("appearance", Section::Appearance), ("look", Section::Look)] {
            let (mut s, _) = state(None);
            update(&mut s, Message::Section(section));
            themes::set(theme);
            let file = format!("settings-{name}-{}", theme.id);
            let size = Size::new(WINDOW_SIZE.width, 900.0);
            failed.extend(golden::check("screens", &file, size, &[1.0], || view(&s)));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}
