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
    LookTheme, Provider, Rgb, SearchWith, SelectStyle, SnipAfter, SongSource, Target, TextReader,
    Theme,
};
use crate::hotkey::{DesktopKey, managed};
use crate::labs::{Feature, Labs};
use crate::music::{self};
use crate::secrets::{AiKey, KeyStore, Secret, SecretsError};
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
    (state.moved, state.picked, state.played) = (None, None, None);
    out
}

fn moving_update(state: &mut State, message: Message) -> Outcome {
    let mut out = super::update(state, message);
    loop {
        // a write first: a key job's answer must not drop it
        if let Outcome::Write(w) = out {
            out = super::update(state, Message::Written(w.run()));
            continue;
        }
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
use crate::ui::kit::testing::shot;
use crate::ui::kit::tooltip::TIPS;
use crate::ui::kit::tooltip::tests_support::{assert_tips_add_something, shown};

#[test]
fn general_has_the_start_with_the_os_switch_on_by_default() {
    let path = tmp("autostart");
    let (mut s, _) = state(Some(path.clone()));
    assert!(s.draft.general.autostart);
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == pages::START_LABEL), "{texts:?}");
    update(&mut s, Message::Autostart(false));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.problems);
    };
    assert!(!cfg.general.autostart);
    assert!(!Config::load_from(&path).0.general.autostart);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Settings > AI: "AI chat stays open", and Chat history's link to History
/// (its switch lives there), for every provider, saved to the file.
#[test]
fn ai_has_the_chat_rows_and_saves_them() {
    let path = tmp("chat-rows");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Section(Section::Ai));
    update(&mut s, Message::Provider(Provider::Browser));
    let texts = shot::texts(view(&s));
    for want in [
        "AI chat stays open",
        "After a follow-up",
        "Chat history",
        "Open History",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    update(&mut s, Message::ChatStays(crate::config::StaysOpen::Always));
    update(&mut s, Message::KeepChats(false));
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.problems);
    };
    assert_eq!(cfg.chat.stays_open, crate::config::StaysOpen::Always);
    let back = Config::load_from(&path).0;
    assert!(!back.chat.keep_history && back.chat.stays_open == crate::config::StaysOpen::Always);
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
    assert!(texts.iter().any(|t| t == KEY_MASK));
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
    fn is_set(&self, p: AiKey) -> Result<bool, SecretsError> {
        self.0.is_set(p)
    }
    fn read(&self, p: AiKey) -> Result<Option<Secret>, SecretsError> {
        self.0.read(p)
    }
    fn set(&self, p: AiKey, k: &Secret) -> Result<(), SecretsError> {
        self.0.set(p, k)
    }
    fn delete(&self, p: AiKey) -> Result<(), SecretsError> {
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
    assert_eq!(
        window_limits(work(3440.0, 1440.0, 1.0)).0,
        Size::new(WINDOW_SIZE.width, WINDOW_SIZE.height.max(FIT_HEIGHT))
    );
    // The header and every sidebar page are what FIT_HEIGHT counts.
    const { assert!(FIT_HEIGHT > 400.0 && FIT_HEIGHT <= WINDOW_SIZE.height + 20.0) };
}

#[test]
fn hidden_features_have_no_settings_page_row_or_shortcut() {
    let (mut s, _) = state(None);
    let side = sidebar(&s);
    assert!(!side.contains(&"Song search"), "{side:?}");
    assert!(side.contains(&"Labs") && side.contains(&"Snip"));
    // No song shortcut field, no search-history row (Snip's are there: out of Labs).
    let shortcuts = page(&mut s, Section::Shortcuts);
    assert!(
        !shortcuts.iter().any(|t| t.contains("Song")),
        "{shortcuts:?}"
    );
    assert!(shortcuts.iter().any(|t| t == "Snip"));
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
    super::update(&mut s, Message::Labs(Feature::Song, true));
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
        super::update(&mut s, Message::Labs(Feature::Song, true)),
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
    assert!(back.theme == Theme::Light && back.labs.song && !back.general.autostart);
}

#[test]
fn a_labs_switch_saves_and_applies_at_once_and_brings_its_rows_back() {
    let path = tmp("labs-switch");
    let (mut s, _) = state(Some(path.clone()));
    // An unsaved edit elsewhere is not saved by a Labs switch.
    update(&mut s, Message::Theme(Theme::Light));
    let Outcome::Saved(cfg) = update(&mut s, Message::Labs(Feature::Song, true)) else {
        panic!("not saved: {:?}", s.notice)
    };
    assert!(cfg.labs.song && !cfg.labs.live && cfg.theme == Theme::System);
    let (back, w) = Config::load_from(&path);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(back.labs, cfg.labs);
    assert_eq!(back.theme, Theme::System, "other draft edits stay unsaved");
    assert_eq!(s.draft.theme, Theme::Light);
    // Song search's page and its shortcut are back.
    assert!(sidebar(&s).contains(&"Song search"));
    let shortcuts = page(&mut s, Section::Shortcuts);
    assert!(
        shortcuts.iter().any(|t| t == "Song search"),
        "{shortcuts:?}"
    );
    // History likewise.
    update(&mut s, Message::Labs(Feature::History, true));
    assert!(
        page(&mut s, Section::General)
            .iter()
            .any(|t| t == "Search history")
    );
    // Off again: gone again, and the page it was on is left.
    update(&mut s, Message::Section(Section::Song));
    update(&mut s, Message::Labs(Feature::Song, false));
    assert_eq!(s.section, Section::Labs);
    assert!(!sidebar(&s).contains(&"Song search"));
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
/// problem is held back until it is fixed.
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
    assert!(!s.dirty());
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
    update(&mut s, Message::Preset(preset("Ollama (local)")));
    update(&mut s, Message::Model("llama3:8b".into()));
    assert!(!s.dirty(), "{:?}", s.problems);
    assert_eq!(Config::load_from(&path).0.ai.model, "llama3:8b");

    // Saving says nothing (owner, 2026-10-29): no pill, no frame clock.
    assert!(!shot::texts(view(&s)).iter().any(|t| t == "Saved"));
    assert!(!s.animating());
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_round_trips_through_a_temp_config() {
    let path = tmp("roundtrip");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Theme(Theme::Light));
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    update(&mut s, Message::Preset(preset("Ollama (local)")));
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
    update(&mut s, Message::Preset(preset("OpenAI")));
    assert_eq!(
        s.draft.ai.model, "gpt-4o-mini",
        "preset fills an empty model"
    );
    update(&mut s, Message::Provider(Provider::Anthropic));
    assert_eq!(s.draft.ai.model, crate::ai::DEFAULT_ANTHROPIC_MODEL);
    assert_eq!(s.draft.ai.base_url, "", "Anthropic uses its own endpoint");
}

const FAKE_KEY: &str = "sk-ant-FAKE-0123456789";

/// A saved key shows as dots only, the same for every key: no view text
/// holds any part of it (not its last characters either), and nothing offers
/// to reveal it; with no key the field asks for one.
#[test]
fn a_saved_key_shows_as_dots_only() {
    // built at run time: no key-shaped literal in the source
    let key: String = [
        "sk",
        "-test-",
        &format!("{:x}", std::process::id()),
        "Q9z7Wm4V",
    ]
    .concat();
    let (mut s, store) = state(None);
    update(&mut s, Message::Preset(preset("OpenAI")));
    update(&mut s, Message::Section(Section::Ai));
    assert_eq!(super::state::API_KEY.placeholder, "Paste your API key");
    update(
        &mut s,
        Message::ApiKey(KeyMessage::Input(Secret::new(key.clone()))),
    );
    update(&mut s, Message::ApiKey(KeyMessage::Save));
    assert_eq!(store.get("openai").as_deref(), Some(key.as_str()));
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == KEY_MASK), "{texts:?}");
    assert!(KEY_MASK.chars().all(|c| c == '•'));
    for want in ["Replace", "Remove"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    let chars: Vec<char> = key.chars().collect();
    for t in &texts {
        assert!(!t.contains(&key), "{t}");
        // no 4 characters of the key in a row (its last 4 included)
        for part in chars.windows(4) {
            let part: String = part.iter().collect();
            assert!(!t.contains(&part), "{t} holds {part}");
        }
        assert!(!["Reveal", "Show key"].iter().any(|r| t.contains(r)), "{t}");
    }
}

/// Each provider has its own key (lead review, 2026-10-28): after a pick the
/// field shows "no key" until that provider's key is saved, a Custom server
/// never shows another's, and going back shows the first one's again.
#[test]
fn each_provider_shows_only_its_own_key() {
    let fake = |tag: &str| Secret::new(format!("fake-{tag}-{:x}", std::process::id()));
    let (mut s, store) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    let saved = |s: &State| shot::texts(view(s)).iter().any(|t| t == KEY_MASK);
    update(&mut s, Message::Preset(preset("OpenRouter")));
    update(
        &mut s,
        Message::ApiKey(KeyMessage::Input(fake("openrouter"))),
    );
    update(&mut s, Message::ApiKey(KeyMessage::Save));
    assert_eq!(*s.key.status(), KeyStatus::Saved);
    assert!(saved(&s));
    for other in ["Google Gemini", "OpenAI", "Custom server", "Anthropic"] {
        update(&mut s, Message::Preset(preset(other)));
        assert_eq!(*s.key.status(), KeyStatus::NotSet, "{other}");
        assert!(!saved(&s), "{other} shows no key");
    }
    // a Custom server's typed URL changes nothing
    update(&mut s, Message::Preset(preset("Custom server")));
    update(&mut s, Message::BaseUrl("https://my.example/v1".into()));
    assert_eq!(*s.key.status(), KeyStatus::NotSet);
    update(&mut s, Message::Preset(preset("Google Gemini")));
    update(&mut s, Message::ApiKey(KeyMessage::Input(fake("gemini"))));
    update(&mut s, Message::ApiKey(KeyMessage::Save));
    update(&mut s, Message::Preset(preset("OpenRouter")));
    assert_eq!(*s.key.status(), KeyStatus::Saved, "its own key again");
    let mine = |id| store.get(id).unwrap_or_default();
    assert!(mine("openrouter").contains("openrouter") && mine("gemini").contains("gemini"));
    assert!(store.get("openai_compatible").is_none() && store.get("custom").is_none());
}

/// The Provider dropdown's entry called `name`.
fn preset(name: &str) -> Preset {
    *PRESETS.iter().find(|p| p.name == name).expect("a preset")
}

/// Each provider in the dropdown writes the API it speaks and its server:
/// Anthropic its own endpoint, the rest the OpenAI-compatible client at
/// their URL, Custom server the URL typed; the key row follows the provider.
#[test]
fn each_provider_writes_its_api_and_server() {
    let want = [
        (
            "OpenRouter",
            Provider::OpenaiCompatible,
            "https://openrouter.ai/api/v1",
        ),
        (
            "OpenAI",
            Provider::OpenaiCompatible,
            "https://api.openai.com/v1",
        ),
        ("Anthropic", Provider::Anthropic, ""),
        (
            "Google Gemini",
            Provider::OpenaiCompatible,
            "https://generativelanguage.googleapis.com/v1beta/openai",
        ),
        (
            "Groq",
            Provider::OpenaiCompatible,
            "https://api.groq.com/openai/v1",
        ),
        (
            "Mistral",
            Provider::OpenaiCompatible,
            "https://api.mistral.ai/v1",
        ),
        (
            "DeepSeek",
            Provider::OpenaiCompatible,
            "https://api.deepseek.com/v1",
        ),
        ("xAI", Provider::OpenaiCompatible, "https://api.x.ai/v1"),
        (
            "Ollama (local)",
            Provider::OpenaiCompatible,
            "http://localhost:11434/v1",
        ),
        (
            "LM Studio (local)",
            Provider::OpenaiCompatible,
            "http://localhost:1234/v1",
        ),
        ("Custom server", Provider::OpenaiCompatible, ""),
    ];
    let names: Vec<_> = PRESETS.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        want.clone().map(|w| w.0),
        "the owner's list, in order"
    );
    for (name, provider, url) in want {
        // from the browser, and from each other provider
        for from in [None, Some("Anthropic"), Some("OpenRouter")] {
            let (mut s, _) = state(None);
            if let Some(from) = from {
                update(&mut s, Message::Preset(preset(from)));
            }
            update(&mut s, Message::Preset(preset(name)));
            let ai = &s.draft.ai;
            assert_eq!(
                (&ai.provider, ai.base_url.as_str()),
                (&provider, url),
                "{name}"
            );
            assert_eq!(Preset::of(ai), Some(preset(name)), "{name} shows as itself");
            assert!(!ai.model.is_empty() || name == "Custom server", "{name}");
            update(&mut s, Message::Section(Section::Ai));
            let texts = shot::texts(view(&s));
            let url_row = texts.iter().any(|t| t == "Server URL");
            assert_eq!(url_row, name == "Custom server", "{name}: {texts:?}");
        }
    }
    // A Custom server keeps the URL typed when picked again.
    let (mut s, _) = state(None);
    update(&mut s, Message::Preset(preset("Custom server")));
    update(&mut s, Message::BaseUrl("https://my.example/v1".into()));
    update(&mut s, Message::Preset(preset("Custom server")));
    assert_eq!(s.draft.ai.base_url, "https://my.example/v1");
}

/// A config from before the Provider dropdown reads as the same provider and
/// is written back unchanged (the config format is frozen).
#[test]
fn old_configs_show_as_the_same_provider_and_save_unchanged() {
    let cases = [
        ("anthropic", "", "Anthropic"),
        ("openai_compatible", "https://api.openai.com/v1", "OpenAI"),
        (
            "openai_compatible",
            "https://openrouter.ai/api/v1/",
            "OpenRouter",
        ),
        (
            "openai_compatible",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "Google Gemini",
        ),
        (
            "openai_compatible",
            "http://localhost:11434/v1",
            "Ollama (local)",
        ),
        (
            "openai_compatible",
            "http://localhost:1234/v1",
            "LM Studio (local)",
        ),
        (
            "openai_compatible",
            "http://192.168.1.2:8080/v1",
            "Custom server",
        ),
    ];
    for (provider, url, name) in cases {
        let path = tmp("old-provider");
        let file =
            format!("[ai]\nprovider = \"{provider}\"\nbase_url = \"{url}\"\nmodel = \"m\"\n");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &file).unwrap();
        let (config, warnings) = Config::load_from(&path);
        assert!(warnings.is_empty(), "{warnings:?}");
        let before = config.ai.clone();
        let mut s = State::new(
            config,
            vec![],
            Some(path.clone()),
            Box::new(MemoryStore::default()),
        );
        assert_eq!(
            Preset::of(&s.draft.ai),
            Some(preset(name)),
            "{provider} {url}"
        );
        update(&mut s, Message::Section(Section::Ai));
        let texts = shot::texts(view(&s));
        assert!(texts.iter().any(|t| t == "AI provider"), "{texts:?}");
        // a change elsewhere saves; [ai] is written as it was
        update(&mut s, Message::DefaultAction(DefaultAction::Copy));
        assert!(
            matches!(update(&mut s, Message::Save), Outcome::Saved(_)),
            "{name}: {:?}",
            s.problems
        );
        assert_eq!(Config::load_from(&path).0.ai, before, "{name}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

/// Settings never offers an API protocol as a choice (owner, 2026-10-28).
#[test]
fn the_ai_page_never_names_a_protocol() {
    let banned = [
        "OpenAI-like",
        "OpenAI-compatible",
        "OpenAI compatible",
        "Anthropic API",
    ];
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    let mut seen: Vec<String> = PRESETS.iter().map(|p| p.name.to_string()).collect();
    seen.extend(shot::texts(view(&s)));
    for p in PRESETS {
        update(&mut s, Message::Preset(*p));
        seen.extend(shot::texts(view(&s)));
    }
    for t in &seen {
        assert!(!banned.iter().any(|b| t.contains(b)), "{t}");
    }
}

#[test]
fn the_browser_provider_needs_no_key_model_or_server() {
    let (mut s, store) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    assert_eq!(s.draft.ai.provider, Provider::Browser, "the default");
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Answers from"), "{texts:?}");
    for hidden in ["API key", "Model", "Server URL", "Provider"] {
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
    fn is_set(&self, _: AiKey) -> Result<bool, SecretsError> {
        Ok(false)
    }
    fn read(&self, _: AiKey) -> Result<Option<Secret>, SecretsError> {
        Ok(None)
    }
    fn set(&self, _: AiKey, _: &Secret) -> Result<(), SecretsError> {
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
    fn delete(&self, _: AiKey) -> Result<(), SecretsError> {
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
    assert!(texts.iter().any(|t| t == KEY_MASK), "{texts:?}");
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
    assert!(texts.iter().any(|t| t == KEY_MASK), "{texts:?}");
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
        // (The sidebar items and segment pills are named by their own text.)
        let mut labels = TIPS.with(|t| t.borrow().clone());
        labels.sort();
        labels.retain(|t| t != "Save settings");
        let mut unique = labels.clone();
        unique.dedup();
        // one page never has two controls of one name (a row shown on two
        // pages, like Snip's shortcuts and the save folder, is fine)
        assert_eq!(labels, unique, "duplicate control labels on {}", page.title);
        all.extend(labels);
    }
    all.sort();
    all.dedup();
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
            .any(|t| t.starts_with('…') && t.chars().count() == 20),
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
    update(&mut s, Message::Engine(Engine::Ai));
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
    let gap =
        "No offline model for this pair yet (coming in a future update): choose AI to translate it";
    assert!(!texts.iter().any(|t| t == gap));
    update(&mut s, Message::Source(lang::find("pt").copied().unwrap()));
    let texts = shot::texts(view(&s));
    assert!(
        texts.iter().any(|t| t == gap),
        "pt -> ar offline: {texts:?}"
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

/// Windows sends no key-down for Print Screen, only its key-up: the recorder
/// takes Print Screen (alone, with Alt, with Ctrl) as it goes up; any other
/// key's key-up records nothing.
#[test]
fn print_screen_records_on_its_key_up() {
    use iced::keyboard::{self, Key, Location, Modifiers, key};
    let up = |code, modifiers| {
        iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key: Key::Unidentified,
            modified_key: Key::Unidentified,
            physical_key: key::Physical::Code(code),
            location: Location::Standard,
            modifiers,
        })
    };
    let shortcut = |e| super::state::chord_of(e).and_then(|c| c.shortcut());
    assert_eq!(
        shortcut(up(key::Code::PrintScreen, Modifiers::ALT)).as_deref(),
        Some("Alt+PrintScreen")
    );
    assert_eq!(
        shortcut(up(key::Code::PrintScreen, Modifiers::empty())).as_deref(),
        Some("PrintScreen")
    );
    assert_eq!(
        shortcut(up(key::Code::KeyA, Modifiers::ALT)),
        None,
        "a key-up"
    );
}

#[test]
fn snip_shortcuts_are_listed_and_editable() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    update(&mut s, Message::Section(Section::Shortcuts));
    // Snip is one feature (owner, 2026-10-31): ONE row; the window and
    // full-screen entries stay in the file, unseen.
    let texts = shot::texts(view(&s));
    // (the sidebar's page and the one row)
    assert_eq!(
        texts.iter().filter(|t| *t == "Snip").count(),
        2,
        "{texts:?}"
    );
    for gone in ["Snip a region", "Snip a window", "Snip the full screen"] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    let snips = |s: &State| {
        s.slots()
            .filter(
                |slot| matches!(slot, Slot::Quick(i) if s.draft.quick_actions[*i].action == "snip"),
            )
            .collect::<Vec<_>>()
    };
    let [Slot::Quick(i)] = snips(&s)[..] else {
        panic!("one snip row")
    };
    assert_eq!(
        s.draft.quick_actions[i].shortcut, "PrintScreen",
        "the user's key"
    );
    for (target, key) in [
        (Target::Window, "Alt+PrintScreen"),
        (Target::FullScreen, "Ctrl+PrintScreen"),
    ] {
        let kept = (s.draft.quick_actions.iter())
            .any(|q| q.action == "snip" && q.target == target && q.shortcut == key);
        assert!(kept, "{key}: kept in the file (it pre-picks), unseen");
    }
    update(&mut s, Message::Record(Slot::Quick(i)));
    press(&mut s, chord(false, false, "F8"));
    assert_eq!(s.draft.quick_actions[i].shortcut, "F8");
    // Print Screen alone is valid; so is Alt alone + a key (Alt is a modifier).
    for (alt, want) in [
        (false, "PrintScreen"),
        (true, "Alt+PrintScreen"),
        (true, "Alt+F8"),
    ] {
        update(&mut s, Message::Record(Slot::Quick(i)));
        let key = Chord {
            ctrl: false,
            alt,
            shift: false,
            logo: false,
            key: want.trim_start_matches("Alt+").into(),
        };
        press(&mut s, key);
        assert_eq!(s.shortcut_error, None, "{want}");
        assert_eq!(s.draft.quick_actions[i].shortcut, want);
        assert!(s.problems.is_empty(), "{want}: {:?}", s.problems);
    }
    // the unseen window snip gave Alt+Print Screen up to the Snip row
    let window = (s.draft.quick_actions.iter())
        .find(|q| q.action == "snip" && q.target == Target::Window)
        .unwrap();
    assert_eq!(window.shortcut, "", "one key, one job");
    // each key still does one job
    update(&mut s, Message::Record(Slot::Quick(i)));
    press(&mut s, chord(true, true, "Space"));
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
        "Custom",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert!(
        !texts.iter().any(|t| t == "Stealth Frost"),
        "no card (owner, 2026-10-30)"
    );
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
    for want in ["Edge style", "Glow strength", "Your two colours"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    // hidden until it does something: glass bars (0.1.4); Snip is out of
    // Labs, so the edge row has two modes at least
    assert!(!texts.iter().any(|t| t == "Bars and pills"));
    assert!(texts.iter().any(|t| t == "Screen edge for"), "{texts:?}");
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
    for want in ["Engine", "Auto", "Fast", "Accurate"] {
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
        update(&mut s, Message::Preset(preset("OpenAI")));
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
            let window = crate::ui::kit::themes::c().bg;
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

/// The Labs rule: Song search is listed only while its switch is on, and
/// switching it off on its own page goes back to Labs. Snip is out of Labs
/// (N-41): always listed.
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
            "Snip",
            "History",
            "Privacy",
            "About",
            "Labs"
        ]
    );
    update(&mut s, Message::Labs(Feature::Song, true));
    assert!(sidebar(&s).contains(&"Song search"));
    update(&mut s, Message::Section(Section::Song));
    update(&mut s, Message::Labs(Feature::Song, false));
    assert!(!sidebar(&s).contains(&"Song search"));
    assert_eq!(s.section, Section::Labs, "back to Labs");
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

/// The hairline under the header and the sidebar's edge are one line
/// (owner, 2026-10-28): the same colour, both 1 px, in both themes; the
/// edge starts at the line under the header.
#[test]
fn the_header_line_and_the_sidebar_edge_are_the_same_hairline() {
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    use crate::ui::kit::tokens::size;
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        let (mut s, _) = state(None);
        update(&mut s, Message::Section(Section::Privacy));
        let (w, h) = (WINDOW_SIZE.width as usize, WINDOW_SIZE.height as usize);
        let px = shot::render(view(&s), w as u32, h as u32);
        let at = |x: usize, y: usize| px[(y * w + x) * 4..][..3].to_vec();
        // 1 px rim, then the header; the sidebar's last column is its edge
        let (line_y, edge_x) = (1 + size::HEADER as usize, size::SIDEBAR as usize);
        let line = at(w / 2, line_y);
        assert_eq!(at(edge_x, h / 2), line, "{}: the same colour", theme.id);
        assert_eq!(
            at(edge_x, line_y + 1),
            line,
            "{}: the edge starts there",
            theme.id
        );
        for (x, y) in [(w / 2, line_y - 1), (w / 2, line_y + 1)] {
            assert_ne!(at(x, y), line, "{}: the line is 1 px tall", theme.id);
        }
        for x in [edge_x - 1, edge_x + 1] {
            assert_ne!(at(x, h / 2), line, "{}: the edge is 1 px wide", theme.id);
        }
    }
    themes::set(&SAPPHIRE);
}

/// Settings harmony (owner, 2026-10-28): on every page, every row's control
/// has the same left edge and width, laid out as the window draws it (every
/// Labs feature on, the AI page with a server of your own and its key rows).
#[test]
fn every_rows_control_shares_one_column_on_every_page() {
    use crate::ui::kit::setting_row::CONTROL;
    use crate::ui::kit::tokens::size;
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    s.draft.look.theme = LookTheme::Custom;
    update(&mut s, Message::Provider(Provider::OpenaiCompatible));
    update(
        &mut s,
        Message::BaseUrl("http://192.168.1.2:8080/v1".into()),
    );
    let mut columns = Vec::new();
    let shown: Vec<_> = pages::visible(&s.draft.labs).collect();
    for page in shown {
        update(&mut s, Message::Section(page.id));
        let found = shot::containers(view(&s), WINDOW_SIZE, CONTROL);
        if !matches!(page.id, Section::Privacy) {
            assert!(!found.is_empty(), "{}: no rows", page.title);
        }
        columns.extend(found.into_iter().map(|b| (page.title, b.x, b.width)));
    }
    let (_, x, width) = columns[0];
    assert_eq!(width, size::CONTROL_COLUMN);
    for (page, at, w) in columns {
        assert!(
            (at - x).abs() < 0.5 && (w - width).abs() < 0.5,
            "{page}: a control at x {at}, {w} wide; the column is x {x}, {width} wide"
        );
    }
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
    let cases: [(&str, Section, f32, Setup); 14] = [
        ("ai", Section::Ai, 0.0, |s| {
            update(s, Message::Provider(Provider::OpenaiCompatible));
            update(s, Message::Preset(preset("OpenRouter")));
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
        ("snip", Section::Snip, 1180.0, |_| {}),
        ("snip-print-screen", Section::Snip, 1280.0, |s| {
            s.print_screen_taken = true;
        }),
        ("history", Section::History, 0.0, |s| {
            update(
                s,
                Message::Kept(crate::history::Counts {
                    captures: 12,
                    chats: 0,
                }),
            );
        }),
        ("history-clear-all", Section::History, 0.0, |s| {
            update(
                s,
                Message::Kept(crate::history::Counts {
                    captures: 3,
                    chats: 7,
                }),
            );
            update(s, Message::ClearAll(false));
        }),
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
    // From System (never picked: follows the OS), a card sets its own mode,
    // even the mode the OS shows (owner, 2026-10-28: the cards decide)
    theme::set_os(iced::theme::Mode::Light);
    update(&mut s, Message::Theme(Theme::System));
    update(&mut s, Message::ThemeCard("frost"));
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::Light, "frost")
    );
    update(&mut s, Message::ThemeCard("midnight"));
    assert_eq!(
        (s.draft.theme, themes::current().id),
        (Theme::Dark, "midnight")
    );
    theme::set_os(iced::theme::Mode::Dark);
    update(&mut s, Message::ThemeCard("frost"));
    // no OS mode or logo rows; the logo key is kept as it is
    update(&mut s, Message::Section(Section::Appearance));
    let texts = shot::texts(view(&s));
    for gone in ["OS mode", "Logo colour", "System", "Purple"] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    let Outcome::Saved(cfg) = update(&mut s, Message::Save) else {
        panic!("not saved: {:?}", s.notice)
    };
    let back = Config::load_from(&path).0;
    assert_eq!(back, cfg);
    let a = &back.appearance;
    assert_eq!((a.dark.as_str(), a.light.as_str()), ("midnight", "frost"));
    assert_eq!((back.theme, a.logo), (Theme::Light, config::Logo::Theme));
    assert!(std::fs::read_to_string(&path).unwrap().contains("logo"));
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The look cards write the stored names config v4 keeps: Prism as "aurora",
/// Aurora Borealis as "borealis" (Frost, "mono", has no card); picking the shown card
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
    assert!(
        pages::look_picks(&s).iter().all(|(l, _)| *l != "frost"),
        "no card"
    );
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

// ---- feature previews (`previews`, `kit::hero`) ----

/// `s`'s page with the fixture as its clips (no clip is bundled yet).
fn with_clip(s: &mut State) {
    s.play_clip(super::previews::player::TINY, Duration::ZERO);
}

/// Until the owner's clips exist (owner, 2026-10-30), a page shows only the
/// feature's heading: no Hide / Show preview, no stage, no placeholder, no
/// clock. With a clip, the button and the stage are there.
#[test]
fn a_preview_without_a_clip_shows_only_its_heading() {
    let (mut s, _) = state(None);
    moving_update(&mut s, Message::Section(Section::Translate));
    (s.moved, s.picked) = (None, None);
    assert!(s.players.is_empty() && !s.animating(), "nothing plays");
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Translate"), "{texts:?}");
    for gone in [
        "Preview",
        "5 s preview",
        "Replay",
        "Hide preview",
        "Show preview",
    ] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    // hidden by key, still no button to bring the stage back
    moving_update(&mut s, Message::Preview("translate", false));
    assert!(!shot::texts(view(&s)).iter().any(|t| t == "Show preview"));
    // the clip arrives: the button and the stage appear
    moving_update(&mut s, Message::Preview("translate", true));
    with_clip(&mut s);
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Hide preview"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "5 s preview"), "{texts:?}");
    // a page without a preview starts no clock
    moving_update(&mut s, Message::Section(Section::Privacy));
    s.moved = None;
    assert!(!s.animating());
}

/// A clip plays once from the visit, then rests on its last frame and the
/// clock stops; Replay plays it again; leaving the page drops it.
#[test]
fn a_preview_plays_once_then_rests_and_the_clock_stops() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Ai));
    with_clip(&mut s);
    assert!(s.animating(), "playing");
    let texts = shot::texts(view(&s));
    assert!(!texts.iter().any(|t| t == "Preview"));
    assert!(texts.iter().any(|t| t == "Replay"), "{texts:?}");
    // the clip's time (1.5 s) has passed
    s.played = Some(Instant::now() - Duration::from_millis(1600));
    moving_update(&mut s, Message::Tick);
    assert!(!s.animating(), "rests: no clock");
    assert!(
        s.players.iter().all(|(_, p)| !p.playing()),
        "on its last frame"
    );
    assert_eq!(s.players.len(), 1, "the last frame stays shown");
    // Replay: from the bundled clips again (none yet: the placeholder)
    moving_update(&mut s, Message::Replay);
    assert!(s.players.is_empty() && !s.animating());
    with_clip(&mut s);
    moving_update(&mut s, Message::Section(Section::General));
    assert!(s.players.is_empty(), "left the page: freed");
}

/// Reduced motion: the first frame at once, and no clock for it.
#[test]
fn with_reduced_motion_a_preview_shows_its_first_frame() {
    motion::set_reduced(true);
    let (mut s, _) = state(None);
    moving_update(&mut s, Message::Section(Section::Ai));
    with_clip(&mut s);
    s.moved = None;
    assert!(s.played.is_none() && !s.animating());
    assert!(s.players.iter().all(|(_, p)| !p.playing()));
    motion::set_reduced(false);
}

/// Hide / Show preview write `[appearance].hidden_previews`; hidden, the
/// stage is gone and nothing plays.
#[test]
fn hiding_a_preview_writes_its_key_and_showing_removes_it() {
    let path = tmp("previews");
    let (mut s, _) = state(Some(path.clone()));
    update(&mut s, Message::Section(Section::Ai));
    with_clip(&mut s);
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Hide preview"));
    moving_update(&mut s, Message::Preview("ai", false));
    assert!(!s.animating(), "hidden: no clock");
    update(&mut s, Message::Save);
    let saved = Config::load_from(&path).0.appearance.hidden_previews;
    assert_eq!(saved, ["ai"]);
    with_clip(&mut s); // no clip is bundled yet: the player stands in for it
    let texts = shot::texts(view(&s));
    assert!(texts.iter().any(|t| t == "Show preview"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "5 s preview" || t == "Replay"));
    moving_update(&mut s, Message::Preview("ai", true));
    with_clip(&mut s);
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Hide preview"));
    update(&mut s, Message::Save);
    assert!(
        Config::load_from(&path)
            .0
            .appearance
            .hidden_previews
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Every preview is on its feature's page; a Labs one only while its switch
/// is on (Live translate's on Translate's page).
#[test]
fn labs_previews_show_only_while_their_switch_is_on() {
    use super::previews::{HEROES, on};
    let (mut s, _) = state(None);
    let live = |t: &Vec<String>| t.iter().any(|t| t == "Live translate");
    assert!(!live(&page(&mut s, Section::Translate)));
    s.draft.labs.live = true;
    let texts = page(&mut s, Section::Translate);
    assert!(
        live(&texts) && texts.iter().any(|t| t == "Translate"),
        "{texts:?}"
    );
    for h in HEROES.iter().filter(|h| h.lab.is_some()) {
        let mut labs = Labs::default();
        assert!(
            !on(&with_labs(labs), h.page).any(|o| o.key == h.key),
            "{}",
            h.key
        );
        labs.set(h.lab.unwrap(), true);
        assert!(
            on(&with_labs(labs), h.page).any(|o| o.key == h.key),
            "{}",
            h.key
        );
    }
    // a preview shows its feature's real shortcut
    let texts = page(&mut s, Section::Look);
    let at = texts.iter().position(|t| t == "Circle to Search").unwrap();
    let keys = key_labels(&s.draft.shortcut);
    assert!(
        texts[at..].windows(keys.len()).any(|w| w == keys),
        "{texts:?}"
    );
}

fn with_labs(labs: Labs) -> State {
    let (mut s, _) = state(None);
    s.draft.labs = labs;
    s
}

/// Each page's previews (the placeholders until the clips exist), in
/// Sapphire and Daylight.
#[test]
fn previews_last_frames_match_their_golden_images() {
    use crate::ui::kit::surface::{Material, surface};
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        for (name, section, height) in [
            ("look", Section::Look, 280.0),
            ("text-reading", Section::TextReading, 280.0),
            ("translate-live", Section::Translate, 540.0),
            ("ai", Section::Ai, 280.0),
            ("snip", Section::Snip, 280.0),
            ("song", Section::Song, 280.0),
        ] {
            let (mut s, _) = state(None);
            s.draft.labs = Labs::all_on();
            update(&mut s, Message::Section(section));
            themes::set(theme);
            let file = format!("preview-{name}-{}", theme.id);
            let size = Size::new(640.0, height);
            failed.extend(golden::check("screens", &file, size, &[1.0], || {
                surface(Material::Window, 0.0, super::previews::view(&s).unwrap())
                    .padding(16)
                    .width(iced::Fill)
                    .height(iced::Fill)
                    .into()
            }));
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// N-41: the Snip page has its three shortcuts (the same rows as Shortcuts),
/// the save folder, the long-capture limit and Recent captures' link to
/// History, with every Labs switch off; the limit is saved.
#[test]
fn the_snip_page_has_its_shortcuts_folder_and_long_capture_limit() {
    let dir = tmp("snip-page");
    let path = dir.join("config.toml");
    let (mut s, _) = state(Some(path.clone()));
    assert_eq!(s.draft.labs, Labs::default());
    let texts = page(&mut s, Section::Snip);
    for want in [
        "Snip",
        "PrtSc",
        "Save folder",
        "Long capture",
        "20,000 px",
        "Recent captures",
        "Open History",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    // one shortcut row: Snip is one feature (owner, 2026-10-31)
    for gone in [
        "Snip a region",
        "Snip a window",
        "Snip the full screen",
        "Alt",
    ] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    assert!(!texts.iter().any(|t| t == "Print Screen key"), "no note");
    let mut snip = s.draft.snip.clone();
    snip.long_max = 5_000;
    update(&mut s, Message::Snip(snip));
    assert!(matches!(update(&mut s, Message::Save), Outcome::Saved(_)));
    assert_eq!(Config::load_from(&path).0.snip.long_max, 5_000);
    // the link opens History, which counts what is kept
    assert_eq!(
        update(&mut s, Message::Section(Section::History)),
        Outcome::History(Vec::new())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Print Screen taken by Windows' own screen capture: a calm note with a
/// button to Windows' setting (never changed by us), and the Print Screen
/// row says who has the key. Another app holding a key: its row says so.
#[test]
fn the_snip_page_says_when_print_screen_or_a_key_is_taken() {
    let (mut s, _) = state(None);
    update(&mut s, Message::Section(Section::Snip));
    s.print_screen_taken = true;
    let texts = shot::texts(view(&s));
    for want in [
        "Print Screen key",
        "Open Windows settings",
        "Windows' screen capture uses this key",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    // only bare Print Screen: Alt+ / Ctrl+Print Screen are ours
    let notes = texts.iter().filter(|t| t.contains("uses this key")).count();
    assert_eq!(notes, 1, "{texts:?}");
    assert_eq!(
        update(&mut s, Message::OpenKeyboardSettings),
        Outcome::None,
        "opens Windows' page (Windows only), nothing saved"
    );
    assert!(!s.dirty());
    s.print_screen_taken = false;
    s.desktop = |keys| (keys == "PrintScreen").then_some(DesktopKey::Taken);
    let texts = shot::texts(view(&s));
    assert!(
        texts
            .iter()
            .any(|t| t == "Another app uses this key: pick a different one"),
        "{texts:?}"
    );
}

/// N-39: one History page: each kind's switch, count and Clear; Clear all
/// asks first. Counting and clearing are handed to the shell (a thread).
#[test]
fn history_counts_clears_and_asks_before_clearing_all() {
    use crate::history::{Counts, Kind};
    let (mut s, _) = state(None);
    assert_eq!(
        update(&mut s, Message::Section(Section::History)),
        Outcome::History(Vec::new())
    );
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Counting…"));
    let kept = Counts {
        captures: 3,
        chats: 1,
    };
    update(&mut s, Message::Kept(kept));
    let texts = shot::texts(view(&s));
    for want in [
        "Recent captures",
        "Chat history",
        "3 kept",
        "1 kept",
        "Clear all…",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert_eq!(
        update(&mut s, Message::ClearHistory(Kind::Chats)),
        Outcome::History(vec![Kind::Chats])
    );
    // Clear all asks; Cancel keeps everything; Delete all clears every kind
    assert_eq!(update(&mut s, Message::ClearAll(false)), Outcome::None);
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Delete all"));
    update(&mut s, Message::KeepHistory);
    assert!(!shot::texts(view(&s)).iter().any(|t| t == "Delete all"));
    update(&mut s, Message::ClearAll(false));
    assert_eq!(
        update(&mut s, Message::ClearAll(true)),
        Outcome::History(Kind::ALL.to_vec())
    );
    assert!(!s.clearing_all);
    // a switch off deletes that kind: its count goes to nothing at once
    update(&mut s, Message::KeepChats(false));
    assert_eq!(s.kept.unwrap().chats, 0);
    assert!(shot::texts(view(&s)).iter().any(|t| t == "Nothing kept"));
}

/// The switches for what is kept exist once: on History (Snip and AI link there).
#[test]
fn the_history_switches_exist_only_on_the_history_page() {
    let (mut s, _) = state(None);
    s.draft.labs = Labs::all_on();
    for p in PAGES.iter().filter(|p| p.id != Section::History) {
        let texts = page(&mut s, p.id);
        for old in [
            "Keep chat history",
            "The last 20 snips, on this PC only. Off deletes them",
        ] {
            assert!(!texts.iter().any(|t| t == old), "{}: {old}", p.title);
        }
    }
}
