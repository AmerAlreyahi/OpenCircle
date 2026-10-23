//! Tray icon with "Circle to search" and "Quit".
use std::time::Instant;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::{Trigger, Triggers};

/// Starts the tray. Keep the returned icon alive (Linux keeps it on its GTK thread).
pub fn start(tx: Triggers) -> Option<TrayIcon> {
    #[cfg(target_os = "linux")]
    {
        // tray-icon on Linux needs a GTK main loop on the thread that owns the icon.
        std::thread::spawn(move || {
            if gtk::init().is_err() {
                eprintln!("tray unavailable: GTK init failed");
                return;
            }
            let _tray = build(tx);
            gtk::main();
        });
        None
    }
    #[cfg(not(target_os = "linux"))]
    build(tx)
}

fn build(tx: Triggers) -> Option<TrayIcon> {
    let circle = MenuItem::new("Circle to search", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::with_items(&[&circle, &quit]).ok()?;
    let (circle, quit) = (circle.id().clone(), quit.id().clone());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let trigger = if event.id == circle {
            Trigger::Capture(Instant::now())
        } else if event.id == quit {
            Trigger::Quit
        } else {
            return;
        };
        let _ = tx.unbounded_send(trigger);
    }));
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("OpenCircle")
        .with_icon(icon())
        .build()
        .map_err(|e| eprintln!("tray unavailable: {e}"))
        .ok()
}

/// The 32 px app icon, embedded in the binary.
// ponytail: one size; pick 16/24/32 by tray DPI if it looks soft on scaled displays.
fn icon() -> Icon {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../assets/icons/32x32.png"
    )));
    let mut reader = decoder.read_info().expect("embedded icon is a valid PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("small icon")];
    let info = reader.next_frame(&mut rgba).expect("embedded icon decodes");
    rgba.truncate(info.buffer_size());
    Icon::from_rgba(rgba, info.width, info.height).expect("embedded icon is RGBA")
}
