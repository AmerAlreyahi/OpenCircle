//! R-11: the picked area's right-click menu, on the App.
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::{Point, Rectangle};

use crate::app::App;
use crate::app::tests::{overlay_key, snipping_on};
use crate::features::snip::annotate::{Menu, Message as E};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::features::snip::snip_more::{self as more, Read};
use crate::shared::app_language::tr;
use crate::shared::kit::testing::shot;

const AREA: Rectangle = Rectangle {
    x: 100.0,
    y: 80.0,
    width: 300.0,
    height: 200.0,
};

fn open() -> App {
    super::snip_more::MODELS_HERE.set(Some(true));
    let mut app = snipping_on(800, 600, AREA);
    app.select(AREA);
    let crop = app.crop().unwrap();
    let _ = app.snip_routed(crop, true, false);
    assert!(app.editing());
    app
}

fn editor(app: &App) -> &crate::features::snip::annotate::Annotator {
    &app.o.edit.as_ref().unwrap().annotator
}

fn names(app: &App) -> Vec<&'static str> {
    toolbar::area_rows(editor(app))
        .iter()
        .map(|r| tr(r.name))
        .collect()
}

/// R-11.1: the menu opens at the pointer on the area, over the toolbar's panels (More closes),
/// with the rows in the owner's order and the area's text read for its text rows.
#[test]
fn r11_1_the_menu_opens_on_the_area() {
    let mut app = open();
    let _ = app.on_snip_toolbar(E::Menu(Menu::More));
    let at = Point::new(120.0, 90.0);
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(at)));
    let e = editor(&app);
    assert_eq!(e.area_menu, Some(at));
    assert_eq!(e.menu, None, "More closed");
    assert_eq!(
        names(&app),
        [
            "Copy",
            "Save",
            "Pin to screen",
            "Open in OpenCircle Studio",
            "Copy text",
            "Translate",
            "Ask AI"
        ]
    );
    assert_eq!(
        editor(&app).more.read,
        Read::Reading,
        "the text rows' read starts"
    );
    let words = shot::texts(app.view(app.overlay));
    for want in [
        "Copy",
        "Save",
        "Open in OpenCircle Studio",
        "Ctrl+S",
        "Ctrl+E",
    ] {
        assert!(words.iter().any(|w| w == want), "{want}: {words:?}");
    }
}

/// R-11.3: one source. The first four rows are the toolbar's own buttons (same messages and keys),
/// the text rows are More's rows (same messages, keys, reasons): nothing is a second copy.
#[test]
fn r11_3_the_list_is_the_toolbars_and_mores() {
    let mut app = open();
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(120.0, 90.0))));
    let e = editor(&app);
    let rows = toolbar::area_rows(e);
    // the four finishing rows ARE the toolbar's table (`Act`): the same names, keys, messages
    let acts: Vec<_> = toolbar::Act::ALL.iter().collect();
    for (row, act) in rows.iter().zip(&acts) {
        assert_eq!(row.name, act.name(), "{act:?}");
        assert_eq!(row.caps.join("+"), act.key(), "{act:?}");
        assert_eq!(row.message, Some(act.message()), "{act:?}");
    }
    // and the toolbar's buttons read it too: its Tab stops carry the same messages
    let stops = toolbar::stops(e);
    for act in &acts {
        assert!(stops.contains(&act.message()), "{act:?}");
    }
    assert_eq!(
        toolbar::Act::ALL.map(|a| a.message()),
        [E::Copy, E::Save, E::PinButton, E::OpenStudio]
    );
    let from_more = toolbar::more_rows(e);
    for row in rows.iter().filter_map(|r| r.of.map(|of| (of, r))) {
        let (of, mine) = row;
        let theirs = from_more.iter().find(|m| m.of == Some(of)).unwrap();
        assert_eq!(mine.message, theirs.message, "{of:?}");
        assert_eq!(mine.caps, theirs.caps, "{of:?}");
        assert_eq!(mine.name, theirs.name, "{of:?}");
    }
    assert_eq!(
        rows.iter().filter_map(|r| r.of).collect::<Vec<_>>(),
        [more::Row::CopyText, more::Row::Translate, more::Row::AskAi]
    );
    // no text found: Copy text and Translate are off and say why, as in More
    let (opening, id) = (app.opening, app.more_reads());
    let _ = app.on_more_read(opening, id, Ok(vec![]));
    let rows = toolbar::area_rows(editor(&app));
    let text_rows: Vec<_> = rows[4..]
        .iter()
        .map(|r| (r.message.is_some(), r.why.map(tr)))
        .collect();
    assert_eq!(
        text_rows,
        [
            (false, Some("No text found in this area")),
            (false, Some("No text found in this area")),
            (true, None)
        ]
    );
}

/// R-11.4: the Menu key and Shift+F10 open the menu at the area's centre with the first row on;
/// arrows, Home / End, Enter, Esc work; a press elsewhere closes it.
#[test]
fn r11_4_the_keyboard_opens_and_walks_it() {
    let mut app = open();
    let key = |app: &App, k: Key, m: Modifiers| editor(app).key(&k, None, m, None);
    assert_eq!(
        key(&app, Key::Named(Named::ContextMenu), Modifiers::empty()),
        Some(E::AreaMenu(None))
    );
    assert_eq!(
        key(&app, Key::Named(Named::F10), Modifiers::SHIFT),
        Some(E::AreaMenu(None))
    );
    assert_eq!(key(&app, Key::Named(Named::F10), Modifiers::empty()), None);
    let _ = app.on_snip_toolbar(E::AreaMenu(None));
    let e = editor(&app);
    assert_eq!(e.area_menu, Some(e.doc.view().center()), "at the centre");
    assert_eq!(e.row, Some(0), "the first row is on");
    for (k, want) in [
        (Named::ArrowDown, E::MenuStep(false)),
        (Named::ArrowUp, E::MenuStep(true)),
        (Named::End, E::MenuEdge(true)),
        (Named::Home, E::MenuEdge(false)),
        (Named::Enter, E::Activate),
        (Named::Escape, E::AreaMenuClose),
        (Named::ContextMenu, E::AreaMenuClose),
    ] {
        assert_eq!(
            key(&app, Key::Named(k), Modifiers::empty()),
            Some(want),
            "{k:?}"
        );
    }
    // down then up wraps over the live rows; Enter on the last one (Ask AI) runs it
    let _ = app.on_snip_toolbar(E::MenuEdge(true));
    assert_eq!(editor(&app).row, Some(6));
    let _ = app.on_snip_toolbar(E::Activate);
    assert!(app.o.ai_chat.is_some(), "Ask AI opened its chat");
    assert_eq!(editor(&app).area_menu, None, "a row closes the menu");
    // Esc closes it without acting; a press on the picture closes it too
    let _ = app.on_snip_toolbar(E::AreaMenu(None));
    let _ = app.on_snip_toolbar(E::AreaMenuClose);
    assert_eq!((editor(&app).area_menu, editor(&app).row), (None, None));
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(120.0, 90.0))));
    let _ = app.on_snip_toolbar(E::Press(Point::new(5.0, 5.0)));
    assert_eq!(editor(&app).area_menu, None);
}

/// R-11.5: no area menu while a long capture runs (it would sit in the window's hole, invisible,
/// yet take the arrows and Enter): the Menu key (the overlay's real key path), Shift+F10 and a
/// right press do nothing; the toolbar's Copy / Save / Pin still finish the capture; Esc is as
/// on main (the long capture's Cancel).
#[test]
fn r11_5_no_menu_while_a_long_capture_runs() {
    use crate::features::snip::screens::long_capture::Finish;
    let mut app = open();
    let _ = app.on_snip_toolbar(E::Scroll);
    assert!(app.scroll().is_some());
    overlay_key(&mut app, "menu");
    assert_eq!(editor(&app).area_menu, None, "the Menu key opens nothing");
    let _ = app.on_snip_toolbar(E::AreaMenu(None));
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(120.0, 90.0))));
    assert_eq!(editor(&app).area_menu, None, "nor the editor's own message");
    // Enter with no menu is the long capture's Copy, as before (nothing eats it)
    overlay_key(&mut app, "enter");
    assert_eq!(app.scroll().and_then(|l| l.then), Some(Finish::Copy));
    for (msg, finish) in [
        (E::Copy, Finish::Copy),
        (E::Save, Finish::Save),
        (E::PinButton, Finish::Pin),
    ] {
        let mut app = open();
        let _ = app.on_snip_toolbar(E::Scroll);
        let _ = app.on_snip_toolbar(msg);
        assert_eq!(app.scroll().and_then(|l| l.then), Some(finish));
    }
}

/// R-11.7: an anchor outside every window never opens the menu (it would be invisible and still
/// take the arrows and Enter).
#[test]
fn r11_7_an_anchor_outside_every_window_does_not_open_it() {
    let mut app = open();
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(9000.0, 9000.0))));
    assert_eq!((editor(&app).area_menu, editor(&app).row), (None, None));
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(120.0, 90.0))));
    assert!(editor(&app).area_menu.is_some());
}

/// R-11.6: Arabic mirrors the menu: the Arabic names, the keys kept left to right.
#[test]
fn r11_6_arabic_names_and_isolated_keys() {
    let _ar = crate::shared::kit::testing::arabic();
    let mut app = open();
    let _ = app.on_snip_toolbar(E::AreaMenu(Some(Point::new(120.0, 90.0))));
    let words = shot::texts(app.view(app.overlay));
    for name in names(&app) {
        assert!(words.iter().any(|w| w == name), "{name}: {words:?}");
    }
    assert!(
        words.iter().any(|w| w == "\u{2066}Ctrl+S\u{2069}"),
        "{words:?}"
    );
}
