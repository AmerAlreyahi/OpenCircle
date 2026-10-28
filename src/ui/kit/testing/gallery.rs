//! The kit's gallery: one sheet per component (`tests/fixtures/kit/<component>-
//! <theme>@1x.png`), its states and variants as rows, Latin and Arabic as
//! columns, in Sapphire and Daylight. Each cell is drawn on its own (one
//! cursor per cell: hover, press, a click that focuses), then laid into the
//! sheet. `OPENCIRCLE_UPDATE_GOLDEN=1` writes the sheets anew.
use iced::widget::{container, text};
use iced::{Element, Event, Fill, Point, Size, mouse};

use super::super::themes::{self, DAYLIGHT, SAPPHIRE, c};
use super::super::tokens::{Role, radius};
use super::super::{
    button, card, chip, divider, field, header, panel_frame, progress, scroll, segmented,
    setting_row, sidebar, surface, toast,
};
use super::{golden, shot};

/// The state names' column.
const LABEL: f32 = 112.0;

#[derive(Clone, Copy)]
enum Act {
    Rest,
    Hover,
    Press,
    /// Press and release: focuses a field.
    Click,
}

struct Row {
    state: &'static str,
    act: Act,
    /// Where the cursor is across the cell (else its middle).
    x: Option<f32>,
    view: Box<dyn Fn(bool) -> Element<'static, ()>>,
}

fn row(
    state: &'static str,
    act: Act,
    view: impl Fn(bool) -> Element<'static, ()> + 'static,
) -> Row {
    Row {
        state,
        act,
        x: None,
        view: Box::new(view),
    }
}

/// Latin or Arabic.
fn s(arabic: bool, latin: &'static str, ar: &'static str) -> &'static str {
    if arabic { ar } else { latin }
}

fn on_bg(content: impl Into<Element<'static, ()>>) -> Element<'static, ()> {
    container(content)
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style::default().background(c().bg))
        .into()
}

fn cell(r: &Row, arabic: bool, size: Size) -> Vec<u8> {
    let at = Point::new(r.x.unwrap_or(size.width / 2.0), size.height / 2.0);
    let moved = Event::Mouse(mouse::Event::CursorMoved { position: at });
    let down = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let up = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let (cursor, events) = match r.act {
        Act::Rest => (None, vec![]),
        Act::Hover => (Some(at), vec![moved]),
        Act::Press => (Some(at), vec![moved, down]),
        Act::Click => (Some(at), vec![moved, down, up]),
    };
    let view = on_bg(container((r.view)(arabic)).center(Fill).padding([0, 16]));
    shot::render_with(view, size, cursor, &events)
}

fn label(state: &'static str, height: f32) -> Vec<u8> {
    let view = on_bg(
        container(super::super::text::text(Role::ROW_DESC, state))
            .center_y(Fill)
            .padding([0, 12]),
    );
    shot::render_with(view, Size::new(LABEL, height), None, &[])
}

/// The sheet of `component` (cells of `size`) in Sapphire and Daylight
/// against its golden images.
fn sheet(component: &str, size: Size, rows: &[Row]) {
    if !golden::enabled() {
        return;
    }
    let (cw, ch, lw) = (size.width as usize, size.height as usize, LABEL as usize);
    let w = lw + 2 * cw;
    let mut failed = Vec::new();
    for theme in [&SAPPHIRE, &DAYLIGHT] {
        themes::set(theme);
        let mut pixels = vec![0; w * ch * rows.len() * 4];
        for (i, r) in rows.iter().enumerate() {
            let parts = [
                (0, lw, label(r.state, size.height)),
                (lw, cw, cell(r, false, size)),
                (lw + cw, cw, cell(r, true, size)),
            ];
            for (x, pw, part) in parts {
                for y in 0..ch {
                    let to = ((i * ch + y) * w + x) * 4;
                    pixels[to..to + pw * 4].copy_from_slice(&part[y * pw * 4..(y + 1) * pw * 4]);
                }
            }
        }
        let name = format!("{component}-{}@1x", theme.id);
        let (w, h) = (w as u32, (ch * rows.len()) as u32);
        if let Err(e) = golden::compare("kit", &name, w, h, &pixels) {
            failed.push(e);
        }
    }
    themes::set(&SAPPHIRE);
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn text_sheet() {
    use super::super::text::text;
    let roles = [
        ("page title", Role::PAGE_TITLE),
        ("row title", Role::ROW_TITLE),
        ("row desc", Role::ROW_DESC),
        ("nav", Role::NAV),
        ("control", Role::CONTROL),
        ("bar pill", Role::BAR_PILL),
        ("chat", Role::CHAT),
        ("card title", Role::CARD_TITLE),
        ("card desc", Role::CARD_DESC),
        ("badge", Role::BADGE),
        ("keycap", Role::KEYCAP),
        ("section", Role::SECTION),
        ("app name", Role::APP_NAME),
    ];
    let rows: Vec<_> = (roles.into_iter())
        .map(|(state, role)| {
            row(state, Act::Rest, move |ar| {
                text(role, s(ar, "Search in browser", "بحث في المتصفح")).into()
            })
        })
        .collect();
    sheet("text", Size::new(260.0, 36.0), &rows);
}

#[test]
fn icon_sheet() {
    use super::super::icon::{Icon, icon};
    let mut rows = Vec::new();
    for i in Icon::ALL {
        for px in [14.0, 16.0] {
            let state = match px == 14.0 {
                true => "14 px",
                false => "16 px",
            };
            rows.push(row(state, Act::Rest, move |_| {
                icon(i, px).color(c().text).into()
            }));
        }
    }
    sheet("icon", Size::new(80.0, 36.0), &rows);
}

#[test]
fn surface_sheet() {
    use surface::{Material, surface};
    let rows: Vec<_> = [
        ("window", Material::Window),
        ("recessed", Material::Recessed),
        ("widget", Material::Widget),
    ]
    .into_iter()
    .map(|(state, m)| {
        row(state, Act::Rest, move |ar| {
            let line =
                super::super::text::text(Role::ROW_DESC, s(ar, "Copying files", "جارٍ نسخ الملفات"));
            surface(m, radius::CARD, line)
                .padding(12)
                .width(Fill)
                .into()
        })
    })
    .collect();
    sheet("surface", Size::new(240.0, 72.0), &rows);
}

#[test]
fn button_sheet() {
    use button::{Kind, button, cta_label, label};
    fn content(kind: Kind, ar: bool) -> Element<'static, ()> {
        match kind {
            Kind::Accent => cta_label(s(ar, "Install", "تثبيت"), Some(" · 19 MB".into())),
            Kind::Success => cta_label(s(ar, "Launch", "تشغيل"), None),
            Kind::Danger => cta_label(s(ar, "Uninstall", "إزالة التثبيت"), None),
            Kind::Elevated => label(s(ar, "Browse…", "استعراض…")).into(),
            Kind::Quiet => text(s(ar, "Customize install ▾", "تخصيص التثبيت"))
                .size(11.5)
                .into(),
        }
    }
    let mut rows = Vec::new();
    for kind in Kind::ALL {
        let name = match kind {
            Kind::Elevated => ["elevated", "  hover", "  pressed", "  disabled"],
            Kind::Accent => ["accent", "  hover", "  pressed", "  disabled"],
            Kind::Success => ["success", "  hover", "  pressed", "  disabled"],
            Kind::Danger => ["danger", "  hover", "  pressed", "  disabled"],
            Kind::Quiet => ["quiet", "  hover", "  pressed", "  disabled"],
        };
        for (state, act, enabled) in [
            (name[0], Act::Rest, true),
            (name[1], Act::Hover, true),
            (name[2], Act::Press, true),
            (name[3], Act::Rest, false),
        ] {
            rows.push(row(state, act, move |ar| {
                button(content(kind, ar), kind, enabled.then_some(()))
            }));
        }
    }
    sheet("button", Size::new(260.0, 60.0), &rows);
}

#[test]
fn segmented_sheet() {
    use segmented::{boolean, segmented};
    let off_on = |ar| [s(ar, "Off", "إيقاف"), s(ar, "On", "تشغيل")];
    let three = |ar| {
        let options = [
            (0, s(ar, "System", "النظام")),
            (1, s(ar, "Light", "فاتح")),
            (2, s(ar, "Dark", "داكن")),
        ];
        segmented(&options, 0, 0, 1.0, |_| ())
    };
    let rows = vec![
        row("off", Act::Rest, move |ar| {
            boolean(off_on(ar), false, false, 1.0, |_| ())
        }),
        row("on", Act::Rest, move |ar| {
            boolean(off_on(ar), true, true, 1.0, |_| ())
        }),
        row("glide 0.5", Act::Rest, move |ar| {
            boolean(off_on(ar), true, false, 0.5, |_| ())
        }),
        row("3, hover", Act::Hover, three),
        row("3, pressed", Act::Press, three),
    ];
    sheet("segmented", Size::new(240.0, 48.0), &rows);
}

#[test]
fn field_sheet() {
    use field::field;
    let value = |ar| {
        s(
            ar,
            r"C:\Program Files\OpenCircle\",
            r"C:\المستخدمون\OpenCircle\",
        )
    };
    let rows = vec![
        row("rest", Act::Rest, move |ar| {
            field(value(ar), Some(|_| ()), ())
        }),
        row("hover", Act::Hover, move |ar| {
            field(value(ar), Some(|_| ()), ())
        }),
        row("focused", Act::Click, move |ar| {
            field(value(ar), Some(|_| ()), ())
        }),
        row("read-only", Act::Rest, move |ar| {
            field(value(ar), None::<fn(String)>, ())
        }),
    ];
    sheet("field", Size::new(260.0, 44.0), &rows);
}

#[test]
fn chip_sheet() {
    use chip::Tone;
    let status =
        |tone| move |ar| chip::status(s(ar, "Auto-save active", "الحفظ التلقائي يعمل"), tone);
    let rows = vec![
        row("accent", Act::Rest, |ar| {
            chip::chip(s(ar, "v0.2.0", "الإصدار 0.2.0").to_string())
        }),
        row("tag", Act::Rest, |ar| {
            chip::tag(s(ar, "Experimental", "تجريبي"))
        }),
        row("badge", Act::Rest, |ar| chip::badge(s(ar, "Exp", "تجريبي"))),
        row("pill", Act::Rest, |ar| {
            chip::pill(s(ar, "Pure Rust · iced", "رست خالص"), false, None)
        }),
        row("pill, mono 155", Act::Rest, |ar| {
            chip::pill(s(ar, "Apache-2.0", "أباتشي 2.0"), true, Some(155.0))
        }),
        row("status ok", Act::Rest, status(Tone::Ok)),
        row("status busy", Act::Rest, status(Tone::Busy)),
        row("status error", Act::Rest, status(Tone::Error)),
    ];
    sheet("chip", Size::new(200.0, 40.0), &rows);
}

#[test]
fn card_sheet() {
    let rows = vec![row("info", Act::Rest, |ar| {
        card::card(super::super::text::text(
            Role::ROW_TITLE,
            s(ar, "Made by others", "صنعه آخرون"),
        ))
    })];
    sheet("card", Size::new(240.0, 72.0), &rows);
}

#[test]
fn scroll_sheet() {
    let lines = |ar| {
        let line = move |_| super::super::text::text(Role::ROW_DESC, s(ar, "A line", "سطر"));
        scroll::scroll(iced::widget::column((0..8).map(|i| line(i).into())))
            .height(Fill)
            .into()
    };
    let rows = vec![row("overflowing", Act::Rest, lines)];
    sheet("scroll", Size::new(160.0, 72.0), &rows);
}

#[test]
fn setting_row_sheet() {
    use segmented::boolean;
    let off_on = |ar| [s(ar, "Off", "إيقاف"), s(ar, "On", "تشغيل")];
    let rows = vec![
        row("row", Act::Rest, move |ar| {
            setting_row::row(
                setting_row::title(s(ar, "Start with Windows", "التشغيل مع ويندوز")),
                s(ar, "Waits in the tray", "ينتظر في شريط المهام"),
                boolean(off_on(ar), true, true, 1.0, |_| ()),
            )
        }),
        row("wide", Act::Rest, move |ar| {
            setting_row::wide(
                setting_row::title(s(ar, "Default action", "الإجراء الافتراضي")),
                s(ar, "What Enter does", "ما يفعله Enter"),
                boolean(off_on(ar), false, false, 1.0, |_| ()),
            )
        }),
    ];
    sheet("setting_row", Size::new(360.0, 120.0), &rows);
}

#[test]
fn sidebar_sheet() {
    use sidebar::{Item, sidebar};
    let view = |active: usize, from: usize, glide: f32| {
        move |ar| {
            let items = [
                Item {
                    label: s(ar, "General", "عام"),
                    badge: None,
                },
                Item {
                    label: s(ar, "About", "حول"),
                    badge: None,
                },
                Item {
                    label: s(ar, "Labs", "مختبرات"),
                    badge: Some(s(ar, "Exp", "تجريبي")),
                },
            ];
            iced::widget::row![
                sidebar(&items, active, from, glide, |_| (), ["v0.1.2", "Windows"]),
                iced::widget::Space::new().width(Fill),
            ]
            .into()
        }
    };
    let rows = vec![
        row("first", Act::Rest, view(0, 0, 1.0)),
        row("glide 0.5", Act::Rest, view(2, 0, 0.5)),
        Row {
            x: Some(60.0),
            ..row("hover", Act::Hover, view(0, 0, 1.0))
        },
    ];
    sheet("sidebar", Size::new(200.0, 180.0), &rows);
}

#[test]
fn divider_sheet() {
    let rows = vec![row("rest", Act::Rest, |_| divider::divider())];
    sheet("divider", Size::new(160.0, 24.0), &rows);
}

#[test]
fn progress_sheet() {
    let rows: Vec<_> = [("0 %", 0.0), ("46 %", 0.46), ("100 %", 1.0)]
        .into_iter()
        .map(|(state, share)| row(state, Act::Rest, move |_| progress::bar(share)))
        .collect();
    sheet("progress", Size::new(240.0, 36.0), &rows);
}

#[test]
fn header_sheet() {
    const SIZE: Size = Size::new(360.0, 52.0);
    let title = |ar| {
        s(
            ar,
            "OpenCircle-Setup-0.2.0-x64.exe",
            "إعداد-OpenCircle-0.2.0.exe",
        )
    };
    let view = move |ar| header::compact(title(ar), (), ());
    let close = SIZE.width - 16.0 - 23.0;
    let regular = move |ar| {
        let status = chip::status(s(ar, "Saved", "تم الحفظ"), chip::Tone::Ok);
        let logo = crate::ui::widgets::chrome::logo_handle();
        header::regular(logo, s(ar, "Settings", "الإعدادات"), status, false, |_| ())
    };
    let rows = vec![
        row("regular", Act::Rest, regular),
        Row {
            x: Some(close),
            ..row("regular close hover", Act::Hover, regular)
        },
        Row {
            x: Some(close - 46.0),
            ..row("regular max hover", Act::Hover, regular)
        },
        row("compact", Act::Rest, view),
        Row {
            x: Some(close),
            ..row("close hover", Act::Hover, view)
        },
        Row {
            x: Some(close),
            ..row("close pressed", Act::Press, view)
        },
    ];
    sheet("header", SIZE, &rows);
}

#[test]
fn toast_sheet() {
    use toast::{Tone, toast};
    let rows: Vec<_> = [
        ("ok", Tone::Ok),
        ("error", Tone::Error),
        ("neutral", Tone::Neutral),
    ]
    .into_iter()
    .map(|(state, tone)| {
        row(state, Act::Rest, move |ar| {
            toast(s(ar, "Copied", "تم النسخ"), tone, radius::CARD)
        })
    })
    .collect();
    sheet("toast", Size::new(160.0, 48.0), &rows);
}

#[test]
fn panel_frame_sheet() {
    use super::super::icon::Icon;
    let view = |glyph, ar, long: bool| {
        let line =
            || super::super::text::text(Role::NOTE, s(ar, "A refreshed look.", "مظهر جديد."));
        let body: Element<'static, ()> = match long {
            true => iced::widget::column((0..12).map(|_| line().into())).into(),
            false => line().into(),
        };
        let later = button::label(s(ar, "Later", "لاحقاً"));
        let actions = button::button(later, button::Kind::Quiet, Some(()));
        panel_frame::frame(glyph, "OpenCircle 0.1.2", body, actions, radius::DRAWER, ())
    };
    let rows = vec![
        row("download", Act::Rest, move |ar| {
            view(Icon::Download, ar, false)
        }),
        row("check, long", Act::Rest, move |ar| {
            view(Icon::Check, ar, true)
        }),
    ];
    sheet("panel_frame", Size::new(300.0, 200.0), &rows);
}
