//! Screens: what each window or popup shows, composed from kit parts only
//! (no style in place: `arch_tests::screens_never_style`). Ported one at a
//! time from `ui/` (the UI plan's phase 5).
pub mod bar;
pub mod common;
pub mod editor;
pub mod live;
pub mod model_download;
pub mod onboarding;
pub mod overlay {
    pub mod edge;
}
pub mod panel;
pub mod settings;
pub mod setup;
pub mod song;
pub mod toast;
pub mod translate;
pub mod tray_menu;
pub mod update_card;
