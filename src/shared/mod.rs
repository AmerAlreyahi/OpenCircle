//! What every feature uses and no feature owns: the UI kit and its tokens, the OS
//! layer, config, the app language, storage, updates, setup and the small
//! services (log, paths, secrets, network). Never imports a feature or the app
//! shell (`arch_tests::shared_never_imports_features_or_app`).
pub(crate) mod app_language;
pub(crate) mod config;
pub(crate) mod diagnostics;
pub(crate) mod history;
pub(crate) mod kit;
pub(crate) mod log;
pub(crate) mod net;
pub(crate) mod paths;
pub(crate) mod picture;
pub(crate) mod platform;
pub(crate) mod secrets;
pub(crate) mod setup;
pub(crate) mod store;
pub(crate) mod theme;
pub(crate) mod touch;
pub(crate) mod update;
