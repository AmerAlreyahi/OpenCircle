//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.
mod app;
#[cfg(test)]
mod arch_tests;
mod border;
mod edit;
/// Frames as the window draws them (damage), and the animations' timing.
#[cfg(test)]
mod frame_times;
mod glass;
mod kit;
mod live_controller;
mod live_layer;
mod overlay;
mod panel;
mod picker;
mod platform;
mod sapphire;
mod screens;
use screens::settings;
pub(crate) mod setup;
mod snip;
mod song;
pub mod theme;
mod translate;
pub(crate) mod widgets;
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
    let logo = widgets::chrome::logo();
    platform::windows::set_icon(logo.rgba.clone(), logo.width, logo.height);
    let mut app = iced::daemon(App::boot, App::update, App::view);
    for font in widgets::FONTS {
        app = app.font(font);
    }
    app.default_font(widgets::ui_font())
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
}
