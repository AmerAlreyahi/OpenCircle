//! The UI's test harness: headless renders (`shot`), golden images
//! (`golden`) and the kit's gallery sheets (`gallery`).
pub mod golden;
pub mod shot;

mod gallery;

/// Arabic for as long as the guard lives: the app's language and the layout
/// direction (a view and its size are built in it), English again after.
#[must_use]
pub struct Arabic(#[allow(dead_code)] super::dir::Scope);

pub fn arabic() -> Arabic {
    crate::app_language::set(crate::app_language::Lang::Ar);
    Arabic(super::dir::scope(true))
}

impl Drop for Arabic {
    fn drop(&mut self) {
        crate::app_language::set(crate::app_language::Lang::En);
    }
}
