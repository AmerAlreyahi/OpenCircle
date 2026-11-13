//! The iced daemon: owns the overlay window (pre-created and hidden between uses
//! where windows can hide, see `can_hide_windows`) and its state.

use crate::Trigger;
use crate::app::App;
use iced::futures::channel::mpsc::UnboundedReceiver;

pub fn run(triggers: UnboundedReceiver<Trigger>) -> iced::Result {
    crate::app::background::set_triggers(triggers);
    crate::features::circle::actions::search_in_browser::page::sweep_stale(); // temp files left by a crash
    let logo = crate::shared::kit::logo::window_icon();
    crate::shared::platform::windows::set_icon(logo.rgba.clone(), logo.width, logo.height);
    let result = with_fonts(iced::daemon(App::boot, App::update, App::view))
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
        .inspect(|()| crate::features::snip::annotate::dragout::clear_own()); // a drop's grace ends with us
    crate::shared::platform::tray::release(); // the tray icon goes with the one `main` holds
    result
}

/// `app` with the kit's fonts, the UI font the default (the app, and the long
/// capture pop-up's own process).
pub(crate) fn with_fonts<P: iced::Program>(mut app: iced::Daemon<P>) -> iced::Daemon<P> {
    for font in crate::shared::kit::text::FONTS {
        app = app.font(font);
    }
    app.default_font(crate::shared::kit::text::ui_font())
}
