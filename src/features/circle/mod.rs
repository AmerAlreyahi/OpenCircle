//! Circle to Search (main mode): the frozen screen, circling or dragging a
//! selection, the bottom bar, zoom, and what runs on a selection (the action
//! registry: crop, copy, save, search in browser, QR, share).
pub(crate) mod actions;
pub(crate) mod app;
pub(crate) mod screens;
// What the Studio's selectable text (R-16) reuses of main mode's: the clipboard, the link finder
// and the right-click menu on selected text.
pub(crate) use actions::copy as clipboard;
pub(crate) use actions::entities;
pub(crate) use screens::context_menu as text_menu;
