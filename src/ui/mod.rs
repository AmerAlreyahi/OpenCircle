//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.
mod app;
#[cfg(test)]
mod arch_tests;
/// Frames as the window draws them (damage), and the animations' timing.
#[cfg(test)]
mod frame_times;
pub(crate) mod kit;
mod live_layer;
mod overlay;
mod platform;
pub(crate) mod screens;
use screens::common::language_picker as picker;
use screens::live::controller as live_controller;
pub(crate) use screens::setup;
use screens::{panel, settings, song, translate};
mod snip;
#[cfg(test)]
mod snip_shots;
pub mod theme;
mod windows;

use crate::Trigger;
use app::App;
use app::background::background;
use app::keys;
use iced::futures::channel::mpsc::UnboundedReceiver;
use platform::monitor::taskbar_inset;
use platform::shape;
pub use platform::tray::{refresh as refresh_tray, start as start_tray};
use screens::update_card;

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    app::background::set_triggers(triggers);
    crate::actions::lens::page::sweep_stale(); // pages left by a crash
    let logo = kit::logo::window_icon();
    platform::windows::set_icon(logo.rgba.clone(), logo.width, logo.height);
    let mut app = iced::daemon(App::boot, App::update, App::view);
    for font in kit::text::FONTS {
        app = app.font(font);
    }
    app.default_font(kit::text::ui_font())
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
}
