//! The Settings window: its state and logic (`state`), the shell (`window`:
//! header, sidebar, the page), one file per page (`pages`, one list) and one preview per feature
//! (`previews`, one list).
mod pages;
mod previews;
mod state;
#[cfg(test)]
pub(crate) mod tests;
mod window;

#[cfg(test)]
pub use pages::START_LABEL;
pub use pages::rows;
pub use state::*;
pub use window::view;
