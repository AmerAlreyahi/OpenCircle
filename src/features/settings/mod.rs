//! The Settings window: its state and logic (`state`), the shell (`window`:
//! header, sidebar, the page), one file per page (`pages`, one list) and one preview per feature
//! (`previews`, one list).
pub(crate) mod labs;
mod model_list;
mod pages;
pub(crate) mod previews;
pub(crate) mod search;
mod state;
#[cfg(test)]
pub(crate) mod tests;
mod window;

#[cfg(test)]
pub use pages::START_LABEL;
pub use pages::rows;
/// rav1d behind its safe API, for the Studio walkthrough tests (they decode what they encode).
#[cfg(test)]
pub(crate) use previews::av1::Decoder as Av1Decoder;
pub use state::*;
pub use window::view;
