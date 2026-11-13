//! What every feature uses and no feature owns: the UI kit and its tokens, the OS
//! layer, config, the app language, storage, updates, setup and the small
//! services (log, paths, secrets, network). Never imports a feature or the app
//! shell (`arch_tests::shared_never_imports_features_or_app`).
pub(crate) mod kit;
pub(crate) mod theme;
