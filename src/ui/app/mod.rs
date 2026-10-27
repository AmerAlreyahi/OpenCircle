//! The overlay's state and logic: its messages, the work off the UI thread, key
//! focus, word picking and the quick actions. No widgets are built here.

pub(in crate::ui) mod background;
pub(in crate::ui) mod keys;
pub(in crate::ui) mod message;
pub(in crate::ui) mod quick;
pub(in crate::ui) mod word;
