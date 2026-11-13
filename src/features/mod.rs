//! One folder per feature users see: its logic, state, screens and tests. A
//! feature uses another only through that feature's `mod.rs`
//! (`arch_tests::features_meet_only_through_mod_rs`); what several features use
//! lives in `shared/`.
pub(crate) mod ask_ai;
pub(crate) mod live_translate;
pub(crate) mod onboarding;
pub(crate) mod settings;
pub(crate) mod song;
pub(crate) mod text_reading;
pub(crate) mod translate;
