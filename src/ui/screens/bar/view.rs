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
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::tokens::{Role, radius, size};
use crate::ui::kit::{button, chip, divider, field, grip, progress};
use crate::ui::song;
use crate::ui::widgets::{self, Why};

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
            widgets::chrome::logo_handle(),
            kbd(Stop::Logo),
            Message::AskButton,
        );
        let badge = field::badge("AI", kbd(Stop::Badge), Message::AskSubmit);
        let top = iced::widget::tooltip::Position::Top;
        let slots = (
            widgets::hint(logo, "Open OpenCircle AI chat", Why::Icon, top),
            widgets::hint(badge, "Ask AI", Why::Keys("Enter"), top),
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
            widgets::hint(pill, label, why, top)
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

    /// The note on the text reader (`Reader`): its download offered with its
    /// size, its progress, a failure to try again, or "Reading text…".
    pub(in crate::ui) fn reader_note(&self) -> Option<Element<'_, Message>> {
        // what it says, then its button or progress bar
        let (say, then): (Element<'_, Message>, Element<'_, Message>) = match &self.o.reader {
            Reader::Idle => return None,
            Reader::Missing(bytes) => {
                let mb = bytes.div_ceil(1_000_000).max(1);
                let say = format!("Text selection needs the text reader (~{mb} MB).");
                let download = chip::action("Download".into(), Message::ReaderDownload);
                (sub(Role::NOTE, say).into(), download)
            }
            Reader::Downloading(done) => {
                let percent = (done * 100.0).round();
                let say = sub(
                    Role::NOTE,
                    format!("Downloading the text reader… {percent}%"),
                );
                let bar = container(progress::bar(*done)).width(bar::NOTE_PROGRESS);
                (say.into(), bar.into())
            }
            Reader::Failed(e) => {
                let say = error(Role::NOTE, format!("The text reader didn't download: {e}"));
                let again = chip::action("Try again".into(), Message::ReaderDownload);
                (say.into(), again)
            }
        };
        let row = iced::widget::row![say, then]
            .spacing(bar::NOTE_GAP)
            .padding([0.0, bar::NOTE_PAD])
            .align_y(iced::Center);
        let note = surface(Material::Widget, radius::WINDOW, row)
            .padding(bar::PAD)
            .height(bar::HEIGHT);
        Some(note.into())
    }

    /// `note` centred on the bar at `at`: above it, or below it when the bar
    /// is at the top; opaque, so a press on it never starts a selection.
    pub(in crate::ui) fn beside_bar<'a>(
        &self,
        note: Element<'a, Message>,
        at: Point,
    ) -> Element<'a, Message> {
        let (_, size) = self.bar_size();
        let (y, align) = match at.y > self.o.screen.height / 2.0 {
            true => (at.y - bar::SPACING - bar::HEIGHT, iced::Bottom),
            false => (at.y + size.height + bar::SPACING, iced::Top),
        };
        let note = container(opaque(note))
            .center_x(size.width)
            .height(bar::HEIGHT)
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
    widgets::fitting(&ASK_PLACEHOLDERS, field::SLOTS_TEXT, bar::text_room(width))
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
    widgets::hint(button, label, why, iced::widget::tooltip::Position::Top)
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
        // The whole host on the button (like the Text panel's link chips): the
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
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }

    #[test]
    fn qr_chip_offers_copy_and_open_only_for_links() {
        let screen = Size::new(1200.0, 420.0);
        let link = "https://example.com/a-long-link/that/goes/on/and/on/forever";
        let with_link = || qr_chip(link, (), Some(()));
        let texts = widgets::shot::texts(with_link());
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
        let px = widgets::shot::save("bar-qr-chip", with_link(), screen);
        assert!(px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4]));

        let wifi = qr_chip("WIFI:S:cafe;T:WPA;P:secret;;", (), None);
        let texts = widgets::shot::texts(wifi);
        assert!(texts.iter().any(|t| t.starts_with("QR code: WIFI:S:cafe")));
        assert!(!texts.iter().any(|t| t.starts_with("Open link")));
        // Text-direction controls: dropped from the chip, and a warning beside it.
        let rlo = qr_chip("https://evil.com/\u{202e}moc.elgoog", (), None);
        let texts = widgets::shot::texts(rlo);
        assert!(
            texts.contains(&"QR code: https://evil.com/moc.elgoog… (copies 28 chars)".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Hidden text-direction marks".to_string()));
        assert!(!texts.iter().any(|t| t.contains('\u{202e}')));
    }
}
