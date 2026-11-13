//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.
mod app;
#[cfg(test)]
mod arch_tests;
/// Frames as the window draws them (damage), and the animations' timing.
#[cfg(test)]
mod frame_times;
mod live_layer;
pub(crate) mod long_capture_popup;
mod overlay;
mod platform;
pub(crate) mod screens;
use screens::common::language_picker as picker;
use screens::live::controller as live_controller;
pub(crate) use screens::setup;
use screens::{ai_chat, settings, song, translate};
mod snip;
#[cfg(test)]
mod snip_shots;
mod touch;
mod windows;

use crate::Trigger;
use app::App;
use app::background::background;
use app::keys;
use iced::futures::channel::mpsc::UnboundedReceiver;
use platform::monitor::taskbar_inset;
use platform::shape;
pub use platform::tray::{refresh as refresh_tray, start as start_tray};
use screens::{update_card, update_pill};

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    app::background::set_triggers(triggers);
    crate::actions::search_in_browser::page::sweep_stale(); // temp files left by a crash
    let logo = crate::shared::kit::logo::window_icon();
    platform::windows::set_icon(logo.rgba.clone(), logo.width, logo.height);
    let result = with_fonts(iced::daemon(App::boot, App::update, App::view))
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
        .inspect(|()| crate::snip::annotate::dragout::clear_own()); // a drop's grace ends with us
    platform::tray::release(); // the tray icon goes with the one `main` holds
    result
}

/// `app` with the kit's fonts, the UI font the default (the app, and the long
/// capture pop-up's own process).
fn with_fonts<P: iced::Program>(mut app: iced::Daemon<P>) -> iced::Daemon<P> {
    for font in crate::shared::kit::text::FONTS {
        app = app.font(font);
    }
    app.default_font(crate::shared::kit::text::ui_font())
}
