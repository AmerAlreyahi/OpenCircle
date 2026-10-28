//! The design's sizes, radii, spacing, type roles and fixed colours
//! (`newdesing/styles/ui-kit.css`): every screen takes its numbers from here.
// ponytail: unread until step 5 (the kit components) and its later ports.
#![allow(dead_code)]

use iced::font::Weight;

/// Corner radii.
pub mod radius {
    pub const KEYCAP: f32 = 6.0;
    /// A menu's row (`rounded-md`) and the menu itself (`rounded-lg`, the tray menu).
    pub const MENU_ITEM: f32 = 6.0;
    pub const MENU: f32 = 8.0;
    pub const PILL: f32 = 7.5;
    /// Button, icon button, nav item.
    pub const CONTROL: f32 = 9.0;
    /// Segmented track, input, shortcut box.
    pub const TRACK: f32 = 10.0;
    /// Card, toast, chip, the bar's input, CTA, tray row.
    pub const CARD: f32 = 12.0;
    pub const DROPDOWN: f32 = 14.0;
    pub const EDGE: f32 = 14.0;
    pub const WINDOW: f32 = 16.0;
    pub const DRAWER: f32 = 18.0;
    pub const VOICE: f32 = 20.0;
    pub const AI_CARD: f32 = 22.0;
    pub const FULL: f32 = 999.0;
}

/// Gaps and paddings; `[vertical, horizontal]` pairs as iced's `Padding` takes them.
pub mod space {
    pub const GAP: f32 = 6.0;
    pub const BAR_PAD: f32 = 6.0;
    pub const TRACK_PAD: f32 = 3.0;
    pub const TRACK_GAP: f32 = 2.0;
    pub const ROW_Y: f32 = 10.0;
    pub const PAGE: [f32; 2] = [16.0, 20.0];
    pub const CARD: [f32; 2] = [9.0, 10.0];
    pub const SIDEBAR_PAD: f32 = 10.0;
}

/// Fixed control sizes.
pub mod size {
    use iced::Size;

    pub const ICON_BUTTON: f32 = 34.0;
    pub const HEADER: f32 = 40.0;
    pub const HEADER_COMPACT: f32 = 36.0;
    pub const CAPTION_BUTTON: f32 = 46.0;
    pub const SIDEBAR: f32 = 168.0;
    pub const NAV_ITEM: f32 = 36.0;
    /// A settings page's control column (owner, 2026-10-28: every row's
    /// control has this one left edge and width; `setting_row`).
    pub const CONTROL_COLUMN: f32 = 300.0;
    /// A settings row's least height inside its padding (a one-line
    /// description beside a 36 px control).
    pub const ROW_MIN: f32 = 38.0;
    pub const SHORTCUT: Size = Size::new(CONTROL_COLUMN, 36.0);
    pub const KEYCAP_H: f32 = 24.0;
    pub const CTA_H: f32 = 44.0;
    pub const ICON: f32 = 16.0;
    pub const ICON_SMALL: f32 = 14.0;
    pub const DOT: f32 = 6.0;
    pub const TOAST_DOT: f32 = 8.0;
}

/// A text style: size in px, weight, monospace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Role {
    pub size: f32,
    pub weight: Weight,
    pub mono: bool,
}

const fn role(size: f32, weight: Weight) -> Role {
    Role {
        size,
        weight,
        mono: false,
    }
}

impl Role {
    pub const PAGE_TITLE: Role = role(16.0, Weight::Semibold);
    pub const ROW_TITLE: Role = role(13.0, Weight::Medium);
    pub const ROW_DESC: Role = role(11.5, Weight::Normal);
    pub const NAV: Role = role(12.5, Weight::Medium);
    pub const CONTROL: Role = role(12.0, Weight::Medium);
    /// A pill of onboarding's step track (`!text-[11px]`).
    pub const STEP: Role = role(11.0, Weight::Medium);
    /// A call to action's label (`button::cta_label`, the setup's "→").
    pub const CTA: Role = role(13.0, Weight::Semibold);
    /// A figure in mono (the setup's progress percent).
    pub const FIGURE: Role = Role {
        mono: true,
        ..role(12.0, Weight::Semibold)
    };
    /// A card's title (the model download card's, `.oc-model-title`).
    pub const CARD_HEADING: Role = role(14.0, Weight::Semibold);
    /// A small line beside a title (`.oc-model-meta`), and its strong part.
    pub const META: Role = role(11.0, Weight::Normal);
    pub const META_STRONG: Role = role(11.0, Weight::Semibold);
    pub const BAR_PILL: Role = role(11.5, Weight::Medium);
    /// A small label over a control (the chat drawer's "AI chat stays open").
    pub const LABEL: Role = role(11.5, Weight::Medium);
    pub const CHAT: Role = role(13.0, Weight::Normal);
    /// A menu row's label and its shortcut hint (the tray menu).
    pub const MENU: Role = role(13.0, Weight::Normal);
    pub const MENU_HINT: Role = role(10.5, Weight::Normal);
    /// A card's note and a toast's line (`.as-note`, `.oc-toast`).
    pub const NOTE: Role = role(12.0, Weight::Normal);
    pub const CARD_TITLE: Role = role(11.5, Weight::Semibold);
    pub const CARD_DESC: Role = role(10.5, Weight::Normal);
    pub const BADGE: Role = role(10.0, Weight::Semibold);
    pub const KEYCAP: Role = Role {
        mono: true,
        ..role(11.0, Weight::Medium)
    };
    /// Upper case, muted text (`kit::text` applies both).
    pub const SECTION: Role = role(10.5, Weight::Semibold);
    pub const APP_NAME: Role = role(18.0, Weight::Semibold);
}

/// Colours that stay the same in every theme.
pub mod fixed {
    use iced::Color;

    /// emerald-400 / -300.
    pub const SUCCESS: Color = Color::from_rgb8(0x34, 0xd3, 0x99);
    pub const SUCCESS_TEXT: Color = Color::from_rgb8(0x6e, 0xe7, 0xb7);
    /// rose-400.
    pub const DANGER: Color = Color::from_rgb8(0xfb, 0x71, 0x85);
    /// amber-400.
    pub const WARNING: Color = Color::from_rgb8(0xfb, 0xbf, 0x24);
    /// The window's close button under the cursor, and its glyph.
    pub const CLOSE_HOVER: Color = Color::from_rgb8(0xc4, 0x2b, 0x1c);
    pub const ON_CLOSE: Color = Color::WHITE;
    /// Text on an accent or success button (slate-950).
    pub const ON_ACCENT: Color = Color::from_rgb8(0x02, 0x06, 0x17);
    /// `.oc-code-surface`: (background, text) on dark and on light themes.
    pub const CODE_DARK: (Color, Color) = (
        Color::from_rgba8(12, 16, 26, 0.72),
        Color::from_rgb8(0xe2, 0xe8, 0xf0),
    );
    pub const CODE_LIGHT: (Color, Color) = (
        Color::from_rgba8(15, 23, 42, 0.06),
        Color::from_rgb8(0x0f, 0x17, 0x2a),
    );
    /// The dim behind a drawer (`bg-black/35`).
    pub const SCRIM: Color = Color::from_rgba8(0, 0, 0, 0.35);

    /// The overlay's own marks over the frozen frame (the same over any
    /// picture, so no theme): text found in select-text mode, every word and
    /// the picked ones; a selected word (flat, like a document's selection);
    /// a selection's handles, and the shadow under its corner brackets; the
    /// black behind a zoomed-out frame.
    pub const WORD: Color = Color::from_rgba8(0x9b, 0xb4, 0xff, 0.22);
    pub const WORD_PICKED: Color = Color::from_rgba8(0x5a, 0x8c, 0xff, 0.55);
    pub const TEXT_SELECTION: Color = Color::from_rgba8(0x8f, 0x80, 0xff, 0.35);
    pub const HANDLE: Color = Color::WHITE;
    pub const BRACKET_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.35);
    pub const BACKDROP: Color = Color::BLACK;
}
