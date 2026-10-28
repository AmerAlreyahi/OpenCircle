//! What the bottom bar shows (the new design's search bar, solid), composed
//! from kit parts only: the grip, the AI field with its logo and "AI ↵"
//! slots, the compact pills, the round lab buttons; the note beside it, its
//! shape while it turns into the editing toolbar, and a QR code's chip.
use iced::widget::{container, opaque, space};
use iced::{Element, Point};

use super::geometry::{self as bar, Stop};
use crate::actions::{self, lens, qr, text};
use crate::labs::{self, BarButton};
use crate::ui::app::keys;
use crate::ui::app::message::Message;
use crate::ui::app::{ASK_INPUT, ASK_PLACEHOLDERS, App, LIVE, Reader};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, radius, size};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::kit::{button, chip, divider, field, grip};
use crate::ui::song;

impl App {
    /// The bottom bar, the one action hub, as the new design's search bar: a
    /// grip to drag it, the AI field ("Ask OpenCircle AI about your
    /// screen…": Enter, the logo or the "AI ↵" badge ask about the selection,
    /// else the whole screen; empty, they open Ask AI), the pills Search in
    /// browser (highlighted until another is pressed), Copy text and
    /// Translate, then the round icon buttons. Search in browser and Copy text
    /// are dimmed until something is selected; Translate without a selection
    /// takes the whole screen. Always the same size (`bar_size`). Its field
    /// takes the keys only once clicked: until then letters are the action
    /// shortcuts (L, A, T still search, ask and translate).
    pub(in crate::ui) fn bottom_bar(&self) -> Element<'_, Message> {
        let (width, _) = self.bar_size();
        let kbd = |stop| self.o.bar_focus == Some(stop);
        let logo = field::logo(
            crate::ui::kit::logo::logo(16),
            kbd(Stop::Logo),
            Message::AskButton,
        );
        let badge = field::badge("AI", kbd(Stop::Badge), Message::AskSubmit);
        let top = iced::widget::tooltip::Position::Top;
        // Include screen: accent while on, dimmed while off (the chat's own switch)
        let on = self.o.include;
        let include = field::toggle(Icon::Screen, on, kbd(Stop::Include), Message::IncludeScreen);
        let named = match on {
            true => "Screen included",
            false => "Question only",
        };
        let slots = (
            hint(logo, "Open OpenCircle AI chat", Why::Icon, top),
            field::pair(
                hint(include, named, Why::Icon, top),
                hint(badge, "Ask AI", Why::Keys("Enter"), top),
            ),
        );
        let field = field::slots(
            ASK_INPUT,
            ask_placeholder(width),
            &self.o.ask,
            Message::AskInput,
            Message::AskSubmit,
            width,
            kbd(Stop::Field),
            slots,
        );
        let field = keys::guard(field, Message::Field);
        let keys = &self.o.bar_keys;
        let selected = self.selected_for_bar();
        let shown = labs::bar(&self.o.labs);
        let icons = shown.iter().filter(|b| !b.is_pill()).count() as f32;
        let widths = bar::pill_widths(self.o.screen.width, icons);
        let pills = [
            (
                BarButton::Search,
                lens::ACTION.icon,
                lens::ACTION.label,
                &keys.search,
            ),
            (
                BarButton::CopyText,
                text::ACTION.icon,
                "Copy text",
                &keys.copy_text,
            ),
            (
                BarButton::Translate,
                actions::translate::ACTION.icon,
                actions::translate::ACTION.label,
                &keys.translate,
            ),
        ]
        .into_iter()
        .zip(widths)
        .map(|((which, glyph, label, keys), width)| {
            let labelled = width > bar::ICON_PILL;
            let pill = button::compact(
                glyph,
                labelled.then_some(label),
                width,
                (self.o.pill == which, kbd(Stop::Button(which))),
                self.bar_message(which),
            );
            // Its label is written on it; an icon-only pill (narrow screens)
            // names itself and its shortcut in a tooltip.
            if labelled {
                return pill;
            }
            let why = match keys.is_empty() {
                true => Why::Icon,
                false => Why::Keys(keys),
            };
            hint(pill, label, why, top)
        });
        let pills = iced::widget::row(pills)
            .spacing(bar::PILL_GAP)
            .align_y(iced::Center);
        let icons: Vec<_> = [
            (
                BarButton::Live,
                LIVE,
                "Live translate (Beta)",
                &keys.live,
                self.live.is_some(),
            ),
            (
                BarButton::Song,
                song::MUSIC,
                "Song search",
                &keys.song,
                false,
            ),
            (
                BarButton::Edit,
                actions::edit::ACTION.icon,
                if selected {
                    "Edit image"
                } else {
                    "Edit the screen"
                },
                &keys.edit,
                false,
            ),
        ]
        .into_iter()
        .filter(|(which, ..)| shown.contains(which))
        .map(|(which, glyph, label, keys, active)| {
            let state = icon_button::State {
                active,
                focused: kbd(Stop::Button(which)),
            };
            bar_button(glyph, label, keys, state, self.bar_message(which))
        })
        .collect();
        let line = || container(divider::vertical(bar::DIVIDER_H)).center_x(bar::DIVIDER);
        let mut row = iced::widget::row![grip::grip(Message::BarGrab), field, line(), pills]
            .spacing(bar::SPACING)
            .align_y(iced::Center);
        if !icons.is_empty() {
            row = row.push(line()).extend(icons);
        }
        // Held: the design's accent ring round it.
        let material = match self.o.bar_drag {
            Some(_) => Material::Held,
            None => Material::Widget,
        };
        let shape = surface(material, radius::WINDOW, row)
            .padding(bar::PAD)
            .height(bar::HEIGHT);
        // A press on the padding (not taken by the field or a button) drags
        // the bar too; opaque: a press on it never starts a selection.
        let bar = iced::widget::mouse_area(shape)
            .on_press(Message::BarGrab)
            .interaction(iced::mouse::Interaction::Grab);
        opaque(bar)
    }

    /// The text reader's download card (`model_download`) over the bar: its
    /// question with its size, its progress, a failure to try again.
    pub(in crate::ui) fn reader_note(&self) -> Option<Element<'_, Message>> {
        use crate::ui::screens::model_download::{self as download, On, Stage};
        let (bytes, stage) = match &self.o.reader {
            Reader::Idle => return None,
            Reader::Missing(bytes) => (*bytes, Stage::Ask),
            Reader::Downloading(done) => (crate::ocr::models::full_size(), Stage::at(*done)),
            Reader::Failed(e) => (crate::ocr::models::full_size(), Stage::Failed(e)),
        };
        // ponytail: a started download can't be stopped (it runs on unseen)
        let dismiss = match self.o.reader {
            Reader::Downloading(_) => None,
            _ => Some(Message::ReaderDismiss),
        };
        let on = On {
            download: Message::ReaderDownload,
            dismiss,
            provider: None,
        };
        Some(download::card(&download::OCR, bytes, stage, on))
    }

    /// `note` centred on the bar at `at`: above it, or below it when the bar
    /// is at the top, as tall as it is; opaque, so a press on it never
    /// starts a selection.
    pub(in crate::ui) fn beside_bar<'a>(
        &self,
        note: Element<'a, Message>,
        at: Point,
    ) -> Element<'a, Message> {
        let (_, size) = self.bar_size();
        let screen = self.o.screen.height;
        let (y, room, align) = match at.y > screen / 2.0 {
            true => (0.0, at.y - bar::SPACING, iced::Bottom),
            false => {
                let y = at.y + size.height + bar::SPACING;
                (y, screen - y, iced::Top)
            }
        };
        let note = container(opaque(note))
            .center_x(size.width)
            .height(room.max(0.0))
            .align_y(align);
        iced::widget::pin(note).position(Point::new(at.x, y)).into()
    }

    /// The bar turning into the editing toolbar (or back) at `at`: its
    /// surface only, sized between the two (`dock_size`); opaque, so a press
    /// on it never starts a selection.
    // ponytail: a plain size morph; the full motion polish is v0.1.1.
    pub(in crate::ui) fn morphing_bar(&self, at: Point) -> Element<'_, Message> {
        let size = self.dock_size();
        let shape = surface(Material::Widget, radius::WINDOW, space())
            .width(size.width)
            .height(size.height);
        iced::widget::pin(opaque(shape)).position(at).into()
    }
}

/// The placeholder that fits a field `width` wide, in the UI font.
pub(in crate::ui) fn ask_placeholder(width: f32) -> &'static str {
    crate::ui::kit::text::fitting(&ASK_PLACEHOLDERS, field::SLOTS_TEXT, bar::text_room(width))
}

/// A round button of the bottom bar, named `label` (its tooltip adds `keys`);
/// `on_press: None` = dimmed (nothing to act on yet).
pub(in crate::ui) fn bar_button<'a>(
    glyph: char,
    label: &'a str,
    keys: &'a str,
    state: icon_button::State,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let why = match keys.is_empty() {
        true => Why::Icon,
        false => Why::Keys(keys),
    };
    let button = icon_button(glyph, state, on_press);
    hint(button, label, why, iced::widget::tooltip::Position::Top)
}

/// The chip for a QR code found in the selection: its text (made safe to show, see
/// [`qr::shown`]) copies it, and "Open link · host" (the link's whole host)
/// (only offered for an http(s) link) opens it. Neither happens by itself.
pub fn qr_chip<'a, Message: Clone + 'a>(
    text: &str,
    copy: Message,
    open: Option<Message>,
) -> Element<'a, Message> {
    use crate::ui::kit::icon::FONT;
    let shown = qr::shown(text, bar::QR_SHOWN);
    let glyph = crate::ui::kit::text::text(Role::NOTE, crate::actions::copy::ACTION.icon)
        .font(FONT)
        .size(size::ICON);
    let mut chips = iced::widget::row![
        glyph,
        chip::action(format!("QR code: {}", shown.text), copy),
    ]
    .spacing(bar::QR_GAP)
    .align_y(iced::Center);
    if shown.warn {
        chips = chips.push(error(Role::NOTE, "Hidden text-direction marks"));
    }
    let host = qr::web_url(text).and_then(crate::sys::open::web_host);
    if let (Some(open), Some(host)) = (open, host) {
        // The whole host on the button (like the selection's link chips): the
        // site it really opens, never cut.
        chips = chips.push(chip::action(format!("Open link · {host}"), open));
    }
    surface(
        Material::Widget,
        radius::WINDOW,
        container(chips).center_y(iced::Fill),
    )
    .height(bar::HEIGHT)
    .padding([0.0, bar::QR_PAD])
    .into()
}

/// The chip for a link or email read in the selected words: "Open <host>" /
/// "Write <address>". It opens only on its click (`open`).
pub fn link_chip<'a, Message: Clone + 'a>(
    entity: &crate::actions::entities::Entity,
    open: Message,
) -> Element<'a, Message> {
    use crate::actions::entities::Kind;
    let verb = match entity.kind {
        Kind::Email => "Write",
        _ => "Open",
    };
    action_chip(format!("{verb} {}", entity.shown()), open)
}

/// One acting chip floating beside a selection (a link's Open, Select text):
/// `chip::action` on the bar's surface, the bar's height.
pub fn action_chip<'a, Message: Clone + 'a>(
    label: String,
    on_press: Message,
) -> Element<'a, Message> {
    surface(
        Material::Widget,
        radius::WINDOW,
        container(chip::action(label, on_press)).center_y(iced::Fill),
    )
    .height(bar::HEIGHT)
    .padding([0.0, bar::QR_PAD])
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::tests::{drag, open_app};
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    use iced::Size;

    /// The bar at rest, with a selection made, with its field focused, and
    /// with Arabic typed in it, in Sapphire and Daylight, at 1x and 2x (all
    /// Labs on: every button there).
    #[test]
    fn every_bar_state_matches_its_golden_image() {
        let mut app = open_app(1280, 800, 1.0);
        app.o.labs = crate::labs::Labs::all_on();
        let size = app.bar_size().1;
        let mut failed = Vec::new();
        let mut shoot = |app: &App, state: &str| {
            for theme in [&SAPPHIRE, &DAYLIGHT] {
                themes::set(theme);
                let name = format!("bar-{state}-{}", theme.id);
                failed.extend(golden::check("screens", &name, size, &[1.0, 2.0], || {
                    app.bottom_bar()
                }));
            }
        };
        shoot(&app, "rest");
        let (a, b) = (
            iced::Point::new(200.0, 150.0),
            iced::Point::new(500.0, 350.0),
        );
        drag(&mut app, a, b, false);
        app.o.panel = None; // an auto-search's card
        assert!(app.selected_for_bar());
        shoot(&app, "selected");
        app.o.bar_focus = Some(Stop::Field);
        shoot(&app, "focused");
        app.o.ask = "ما هذا الخطأ؟ what is it".into();
        shoot(&app, "arabic");
        // Include screen off (dimmed), Tab on it
        app.o.include = false;
        app.o.bar_focus = Some(Stop::Include);
        shoot(&app, "question-only");
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    #[test]
    fn qr_chip_offers_copy_and_open_only_for_links() {
        let screen = Size::new(1200.0, 420.0);
        let link = "https://example.com/a-long-link/that/goes/on/and/on/forever";
        let with_link = || qr_chip(link, (), Some(()));
        let texts = crate::ui::kit::testing::shot::texts(with_link());
        assert!(
            texts.contains(
                &"QR code: https://example.com/a-long-link/that… (copies 59 chars)".to_string()
            ),
            "{texts:?}"
        );
        // The whole host, never cut, on the button that opens it.
        assert!(
            texts.contains(&"Open link · example.com".to_string()),
            "{texts:?}"
        );
        let px = crate::ui::kit::testing::shot::save("bar-qr-chip", with_link(), screen);
        assert!(px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4]));

        let wifi = qr_chip("WIFI:S:cafe;T:WPA;P:secret;;", (), None);
        let texts = crate::ui::kit::testing::shot::texts(wifi);
        assert!(texts.iter().any(|t| t.starts_with("QR code: WIFI:S:cafe")));
        assert!(!texts.iter().any(|t| t.starts_with("Open link")));
        // Text-direction controls: dropped from the chip, and a warning beside it.
        let rlo = qr_chip("https://evil.com/\u{202e}moc.elgoog", (), None);
        let texts = crate::ui::kit::testing::shot::texts(rlo);
        assert!(
            texts.contains(&"QR code: https://evil.com/moc.elgoog… (copies 28 chars)".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Hidden text-direction marks".to_string()));
        assert!(!texts.iter().any(|t| t.contains('\u{202e}')));
    }

    /// The link and email chips beside a selection, in both themes.
    #[test]
    fn link_chips_match_their_golden_images() {
        use crate::actions::entities::{Entity, Kind};
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for (name, kind, text) in [
                ("link", Kind::Link, "https://shop.example/track/4412"),
                ("email", Kind::Email, "help@shop.example"),
            ] {
                let entity = Entity {
                    kind,
                    text: text.into(),
                };
                let file = format!("chip-{name}-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    Size::new(300.0, bar::HEIGHT),
                    &[1.0],
                    || link_chip(&entity, ()),
                ));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
