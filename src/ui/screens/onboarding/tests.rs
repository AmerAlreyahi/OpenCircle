//! Onboarding's tests: the flow, Finish, and every step as drawn.
use std::path::PathBuf;

use iced::{Point, Size};

use super::*;
use crate::config::{Config, Provider, Theme};
use crate::secrets::{MemoryStore, Secret};
use crate::ui::kit::testing::shot;
use crate::ui::screens::settings::{self, KeyMessage, PRESETS, tests::tmp};
use crate::ui::theme;

/// `update` with the keyring calls run at once (the shell runs them on a thread).
fn update(state: &mut State, message: Message) -> Outcome {
    let out = super::update(state, message);
    for job in state.take_key_jobs() {
        let k = job.run();
        super::update(state, Message::Settings(settings::Message::ApiKey(k)));
    }
    out
}

/// A Settings row's message.
fn row(state: &mut State, message: settings::Message) {
    update(state, Message::Settings(message));
}

/// A fake key, built at run time (no key-shaped literal in the source).
fn fake_key() -> String {
    ["FAKE", "9876543210"].join("-")
}

fn state(path: Option<PathBuf>) -> State {
    State::new(Config::default(), path, Box::new(MemoryStore::default()))
}

fn next(s: &mut State, times: usize) {
    for _ in 0..times {
        update(s, Message::Next);
    }
}

fn texts(s: &State) -> Vec<String> {
    shot::texts(view(s))
}

/// Back is off on the first step (a click does nothing), Next stops at Done,
/// a pill of the track jumps to its step.
#[test]
fn the_browser_is_preselected_and_steps_stay_in_range() {
    let mut s = state(None);
    assert_eq!(s.config().ai.provider, Provider::Browser);
    let back = shot::click(
        view(&s),
        WINDOW_SIZE,
        Point::new(50.0, WINDOW_SIZE.height - 30.0),
    );
    assert!(back.is_empty(), "Back is off on step 1: {back:?}");
    update(&mut s, Message::Back);
    assert_eq!(s.step(), Step::Welcome);
    next(&mut s, 9);
    assert_eq!(s.step(), Step::Done);
    assert!(texts(&s).iter().any(|t| t == "Finish"));
    update(&mut s, Message::Back);
    assert_eq!(s.step(), Step::Look);
    let back = shot::click(
        view(&s),
        WINDOW_SIZE,
        Point::new(50.0, WINDOW_SIZE.height - 30.0),
    );
    assert!(matches!(back[..], [Message::Back]), "{back:?}");
    // the track: "1. Welcome" … "4. Done", each a jump
    let t = texts(&s);
    for label in ["1. Welcome", "2. Connect AI", "3. Look", "4. Done"] {
        assert!(t.iter().any(|x| x == label), "{label}: {t:?}");
    }
    let pill = shot::click(view(&s), WINDOW_SIZE, Point::new(100.0, 64.0));
    assert!(matches!(pill[..], [Message::Go(Step::Welcome)]), "{pill:?}");
}

#[test]
fn wayland_adds_the_screenshot_step_and_asks_once_at_a_time() {
    let mut s = state(None);
    next(&mut s, 1);
    assert_eq!(s.step(), Step::Ai, "no Screenshots step off Wayland");
    let mut s = state(None).on_wayland(true);
    next(&mut s, 1);
    assert_eq!(s.step(), Step::Screenshots);
    assert!(texts(&s).iter().any(|t| t == "2. Screenshots"));

    // Allow sends one request; a second click while it is out sends none.
    assert_eq!(update(&mut s, Message::Allow), Outcome::AskScreenshot);
    assert_eq!(s.access, Access::Asking);
    assert_eq!(update(&mut s, Message::Allow), Outcome::None);
    assert!(texts(&s).iter().any(|t| t.contains("Waiting")));
    // Refused: said so, and Allow works again.
    update(
        &mut s,
        Message::Allowed(Err(crate::capture::CaptureError::from(
            "no portal".to_string(),
        ))),
    );
    let t = texts(&s);
    assert!(
        t.iter().any(|t| t.contains("Not allowed (no portal)")),
        "{t:?}"
    );
    #[cfg(target_os = "linux")]
    {
        let refused = crate::capture::CaptureError::Refused("refused by the desktop".into());
        update(&mut s, Message::Allowed(Err(refused)));
        let t = texts(&s);
        assert!(
            t.iter().any(|t| t.starts_with("Your desktop said no")),
            "{t:?}"
        );
    }
    assert_eq!(update(&mut s, Message::Allow), Outcome::AskScreenshot);
    update(&mut s, Message::Allowed(Ok(())));
    assert_eq!(s.access, Access::Allowed);
    assert!(texts(&s).iter().any(|t| t == "Allowed"));
    // Next goes on whatever the answer.
    next(&mut s, 1);
    assert_eq!(s.step(), Step::Ai);
    // The prompt after a refused capture uses the same control.
    let mut access = Access::Unasked;
    assert_eq!(ask(&mut access, Message::Allow), Outcome::AskScreenshot);
    let t = shot::texts(prompt_view(&access));
    assert!(t.iter().any(|t| t.contains("needs screenshot permission")));
    let px = shot::save(
        "onboarding-prompt",
        prompt_view(&Access::Unasked),
        PROMPT_SIZE,
    );
    assert!(!px.is_empty());
}

/// Finish writes what the rows picked: Claude with a key (the key stays out
/// of the file), a light theme card.
#[test]
fn finish_writes_the_picked_config() {
    let path = tmp("onboarding-claude");
    let mut s = state(Some(path.clone()));
    next(&mut s, 1);
    row(&mut s, settings::Message::Provider(Provider::Anthropic));
    row(
        &mut s,
        settings::Message::ApiKey(KeyMessage::Input(Secret::new(fake_key()))),
    );
    row(&mut s, settings::Message::ApiKey(KeyMessage::Save));
    next(&mut s, 1);
    assert_eq!(s.step(), Step::Look);
    row(&mut s, settings::Message::ThemeCard("frost"));
    next(&mut s, 1);
    assert_eq!(s.step(), Step::Done);
    let Outcome::Finished(cfg) = update(&mut s, Message::Finish) else {
        panic!("{:?}", s.error)
    };
    let (back, warnings) = Config::load_from(&path);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, cfg);
    assert_eq!(back.ai.provider, Provider::Anthropic);
    assert_eq!(back.ai.model, crate::ai::DEFAULT_ANTHROPIC_MODEL);
    assert_eq!(
        (back.theme, back.appearance.light.as_str()),
        (Theme::Light, "frost")
    );
    assert_eq!(back.update.auto, Some(true), "the switch is pre-set on");
    assert!(back.general.autostart, "pre-set on");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("FAKE"), "the key stays out of the file");
    theme::apply(Theme::Dark);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The last step shows Settings' Start-with-the-OS row (pre-set on) and the
/// shortcut really set; the first step Settings' Keep up to date row.
#[test]
fn the_switches_are_settings_rows_and_kept() {
    let mut s = state(None);
    assert!(texts(&s).iter().any(|t| t == "Keep OpenCircle up to date"));
    row(&mut s, settings::Message::AutoUpdate(false));
    assert_eq!(s.config().update.auto, Some(false));
    next(&mut s, 3);
    assert_eq!(s.step(), Step::Done);
    let t = texts(&s);
    assert!(t.iter().any(|t| t == settings::START_LABEL), "{t:?}");
    let keys = settings::key_labels(&s.config().shortcut).join("+");
    assert!(t.iter().any(|t| t.contains(&keys)), "{keys}: {t:?}");
    row(&mut s, settings::Message::Autostart(false));
    assert!(!s.config().general.autostart);
}

/// A local preset needs no key; Skip puts the AI settings back as loaded.
#[test]
fn a_local_preset_and_skip() {
    let path = tmp("onboarding-ollama");
    let mut s = state(Some(path.clone()));
    next(&mut s, 1);
    row(
        &mut s,
        settings::Message::Provider(Provider::OpenaiCompatible),
    );
    let local = PRESETS.iter().find(|p| p.local).copied().unwrap();
    row(&mut s, settings::Message::Preset(local));
    assert!(
        texts(&s)
            .iter()
            .any(|t| t == "Not needed for a server on this PC")
    );
    let Outcome::Finished(cfg) = update(&mut s, Message::Finish) else {
        panic!("{:?}", s.error)
    };
    assert_eq!(cfg.ai.provider, Provider::OpenaiCompatible);
    assert_eq!(cfg.ai.base_url, local.base_url);
    assert!(!cfg.ai.model.is_empty());
    // Skip: the AI settings as they were, on to the next step
    let mut s = state(Some(path.clone()));
    next(&mut s, 1);
    row(&mut s, settings::Message::Provider(Provider::Anthropic));
    update(&mut s, Message::Skip);
    assert_eq!(s.step(), Step::Look);
    assert_eq!(s.config().ai, Config::default().ai);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A server of your own with no URL yet can't be written: Finish says why.
#[test]
fn finish_names_what_to_fix_and_an_unwritable_path() {
    let mut s = state(None);
    row(
        &mut s,
        settings::Message::Provider(Provider::OpenaiCompatible),
    );
    assert_eq!(update(&mut s, Message::Finish), Outcome::None);
    assert!(s.error.as_deref().unwrap().starts_with("Fix this first"));
    assert!(texts(&s).iter().any(|t| t.starts_with("Fix this first")));

    let dir = tmp("onboarding-blocked");
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::write(&dir, "a file where a folder should be").unwrap();
    let mut s = state(Some(dir.join("config.toml")));
    assert_eq!(update(&mut s, Message::Finish), Outcome::None);
    assert!(s.error.as_deref().unwrap().contains("Could not save"));
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn the_key_never_shows_up() {
    let mut s = state(None).on_wayland(true);
    let typing = settings::Message::ApiKey(KeyMessage::Input(Secret::new(fake_key())));
    assert!(!format!("{typing:?}").contains("FAKE"));
    row(&mut s, settings::Message::Provider(Provider::Anthropic));
    row(&mut s, typing);
    row(&mut s, settings::Message::ApiKey(KeyMessage::Save));
    s.step = Step::Ai;
    assert!(texts(&s).iter().any(|t| t == "Saved in your keyring"));
    for page in STEPS {
        s.step = page.id;
        let t = texts(&s);
        assert!(
            !t.iter().any(|t| t.contains("FAKE")),
            "{:?}: {t:?}",
            page.id
        );
    }
    assert!(!format!("{s:?}").contains("FAKE"));
}

/// The window has the design's fixed size: every step's page fits its body
/// (nothing scrolls, nothing is cut), the tallest AI step too.
#[test]
fn every_step_fits_the_fixed_window() {
    use crate::ui::kit::testing::shot::out_of_reach;
    let mut s = state(None).on_wayland(true);
    row(
        &mut s,
        settings::Message::Provider(Provider::OpenaiCompatible),
    );
    row(&mut s, settings::Message::Preset(PRESETS[0]));
    for page in STEPS {
        s.step = page.id;
        let lost = out_of_reach((page.view)(&s), BODY);
        assert!(lost.is_empty(), "{:?}: out of reach {lost:?}", page.id);
        let lost = out_of_reach(view(&s), WINDOW_SIZE);
        assert!(lost.is_empty(), "{:?}: out of reach {lost:?}", page.id);
    }
}

/// The step's body: the window less its header, rim, padding, the track,
/// the footer and the gaps between them (the design's 680x525 window).
const BODY: Size = Size::new(638.0, 355.0);

#[test]
fn every_control_has_its_own_label() {
    use crate::ui::widgets::TIPS;
    use crate::ui::widgets::tests_support::assert_tips_add_something;
    let mut s = state(None).on_wayland(true);
    row(
        &mut s,
        settings::Message::Provider(Provider::OpenaiCompatible),
    );
    for page in STEPS {
        s.step = page.id;
        TIPS.with(|t| t.borrow_mut().clear());
        let _ = shot::render(view(&s), 680, 525);
        assert_tips_add_something(&format!("onboarding {:?}", page.id));
        let mut tips = TIPS.with(|t| t.borrow().clone());
        let n = tips.len();
        tips.sort();
        tips.dedup();
        assert_eq!(tips.len(), n, "{:?}: {tips:?}", page.id);
    }
}

/// Every step as drawn, in Sapphire and Daylight (the AI step with a key of
/// your own; the Screenshots step as Wayland shows it).
#[test]
fn every_step_matches_its_golden_image() {
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    let mut failed = Vec::new();
    for t in [&SAPPHIRE, &DAYLIGHT] {
        for page in STEPS {
            let mut s = state(None).on_wayland(page.id == Step::Screenshots);
            s.step = page.id;
            if page.id == Step::Ai {
                row(&mut s, settings::Message::Provider(Provider::Anthropic));
            }
            themes::set(t);
            let name = format!(
                "onboarding-{}-{}",
                page.label.to_lowercase().replace(' ', "-"),
                t.id
            );
            failed.extend(golden::check("screens", &name, WINDOW_SIZE, &[1.0], || {
                view(&s)
            }));
            // the Windows contact sheet (`OPENCIRCLE_RENDER_DIR`, Segoe UI)
            let _ = shot::save(&name, view(&s), WINDOW_SIZE);
        }
    }
    themes::set(&SAPPHIRE);
    theme::apply(Theme::Dark);
    assert!(failed.is_empty(), "{failed:#?}");
}
