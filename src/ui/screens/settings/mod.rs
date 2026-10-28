//! The Settings window: its state and logic (`state`), the shell (`window`:
//! header, sidebar, the page), and one file per page (`pages`, one list).
mod legacy;
mod pages;
mod state;
#[cfg(test)]
pub(crate) mod tests;
mod window;

pub use legacy::START_LABEL;
#[allow(unused_imports)]
use legacy::*;
pub use state::*;
pub use window::view;
