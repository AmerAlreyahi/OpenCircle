//! The design's sizes, radii, spacing, type roles and fixed colours
//! (`newdesing/styles/ui-kit.css`): every screen takes its numbers from here.

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
    pub const WINDOW: f32 = 16.0;
    pub const DRAWER: f32 = 18.0;
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
    /// Inside a toolbar's panel (`surface::panel`).
    pub const PANEL: f32 = 12.0;
}

/// Keyboard focus (`focus::shade`): how far clear of a scroll area's edge a
/// focused control is scrolled. (Its look is the theme's `accent_soft`.)
pub mod focus {
    pub const MARGIN: f32 = 12.0;
}

/// Settings round 2 (owner-approved floating-card design, 2026-11-26): the shell's sizes. One block:
/// every number of the new Settings window that no other part shares.
pub mod settings {
    /// The sidebar card's width, and its collapsed (icons only) width.
    /// The title bar's height (`r2` `.tbar`: 48 px).
    pub const HEADER: f32 = 48.0;
    pub const NAV: f32 = 246.0;
    pub const NAV_THIN: f32 = 60.0;
    /// The gap round and between the two floating cards.
    pub const GUTTER: f32 = 12.0;
    pub const CARD_RADIUS: f32 = 18.0;
    /// The page's widest (padding included), and its padding `[top, side]`.
    pub const CONTENT_MAX: f32 = 880.0;
    pub const CONTENT_PAD: [f32; 2] = [28.0, 36.0];
    /// Between a page's sections.
    pub const SECTION_GAP: f32 = 18.0;
    /// The title bar's search, at its widest.
    pub const SEARCH_W: f32 = 560.0;
    /// The page header's icon tile.
    pub const TILE: f32 = 40.0;
    /// The window width from which a page's clip stands in a column beside the page, that
    /// column's width and its side padding.
    pub const WIDE: f32 = 1560.0;
    pub const CLIP_COLUMN: f32 = 380.0;
    pub const CLIP_PAD_X: f32 = 20.0;
    /// The Report a problem dialog's width, and its flag.
    pub const REPORT_W: f32 = 420.0;
    pub const FLAG: f32 = 18.0;
    /// An empty state's glyph and its vertical padding (Extensions).
    pub const EMPTY_GLYPH: f32 = 20.0;
    pub const EMPTY_PAD_Y: f32 = 24.0;
    /// Bottom breathing room inside the page scroll (`r2.css` `.r2 .main .inner`).
    pub const CONTENT_PAD_BOTTOM: f32 = 48.0;
    /// Section box corners, outer padding `[y, x]` and caption padding (`r2.css` `.r2 .sec`).
    pub const SECTION_RADIUS: f32 = 14.0;
    pub const SECTION_PAD: [f32; 2] = [4.0, 20.0];
    pub const SECTION_HEAD_TOP: f32 = 15.0;
    pub const SECTION_HEAD_BOTTOM: f32 = 5.0;
    /// Vertical padding of a row inside a section (`kit.css` `.sec .row`: 12 px + 36 px = 60 px).
    pub const ROW_PAD_Y: f32 = 12.0;
    /// Gap between a block's text and its cards, and between cards in a grid (`extra.css` `.chcs`, `r2.css` `.tgrid`/`.lgrid`).
    pub const BLOCK_GAP: f32 = 12.0;
    pub const CARD_GAP: f32 = 12.0;
    /// Choice card dimensions (`extra.css` `.chc`, `r2.css` `.chc.v`).
    pub const CHOICE_PAD: f32 = 14.0;
    pub const CHOICE_TILE: f32 = 36.0;
    pub const CHOICE_ICON: f32 = 18.0;
    pub const CHOICE_WIDE_H: f32 = 92.0;
    pub const CHOICE_TALL_H: f32 = 136.0;
    /// Appearance mini-window heights (16:10 at 3-col mode and 5-col theme/look widths) and card padding.
    pub const MINI_MODE_H: f32 = 140.0;
    pub const MINI_THEME_H: f32 = 82.0;
    pub const MINI_CARD_PAD: f32 = 6.0;
    pub const MINI_CARD_PAD_BOTTOM: f32 = 8.0;
    pub const MINI_CARD_GAP: f32 = 7.0;
    /// Demo drawer (`r2.css` `.r2 .sec.exp` / `r2.py` `drawer`).
    pub const DEMO_W: f32 = 372.0;
    pub const DEMO_H: f32 = 210.0;
    pub const DEMO_GAP: f32 = 22.0;
    pub const DEMO_PAD: f32 = 16.0;
    pub const DEMO_MEDIA_RADIUS: f32 = 11.0;
    /// Offline model list: icon tile, progress bar width, and row spacing (`extra.css` `.mic`, `.dlp`, `.mrow2`).
    pub const MODEL_TILE: f32 = 30.0;
    pub const MODEL_BAR_W: f32 = 150.0;
    pub const NOTE_TILE: f32 = 28.0;
}

/// Fixed control sizes.
pub mod size {
    use iced::Size;

    pub const ICON_BUTTON: f32 = 34.0;
    /// A toolbar's panel (Snip's group panels, More).
    pub const PANEL: f32 = 252.0;
    pub const HEADER: f32 = 40.0;
    pub const HEADER_COMPACT: f32 = 36.0;
    pub const CAPTION_BUTTON: f32 = 46.0;
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
    /// A side panel's closed handle (`edge_slide::handle`): its width and height, and its arrow.
    pub const HANDLE: (f32, f32) = (22.0, 64.0);
    pub const HANDLE_ARROW: f32 = 14.0;
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
    /// Settings round 2: a page header's title and subtitle (`r2.css` `.ph h1`, `.ph p`).
    pub const PAGE_HEAD: Role = role(20.0, Weight::Semibold);
    pub const PAGE_SUB: Role = role(12.5, Weight::Normal);
    /// Settings round 2: a choice card's title and description (`extra.css` `.chc .ct`, `.chc .cd`).
    pub const CHOICE_TITLE: Role = role(13.5, Weight::Semibold);
    pub const CHOICE_DESC: Role = role(12.0, Weight::Normal);
    /// Settings round 2: a demo block's title (`extra.css` `.rt.big`).
    pub const DEMO_TITLE: Role = role(15.5, Weight::Semibold);
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

    /// emerald-400.
    pub const SUCCESS: Color = Color::from_rgb8(0x34, 0xd3, 0x99);
    /// The recording red (light, dark theme) and the paused amber (R1 bar design).
    pub const REC: (Color, Color) = (
        Color::from_rgb8(0xdc, 0x26, 0x26),
        Color::from_rgb8(0xf8, 0x71, 0x71),
    );
    pub const AMBER: (Color, Color) = (
        Color::from_rgb8(0xb4, 0x53, 0x09),
        Color::from_rgb8(0xfb, 0xbf, 0x24),
    );
    /// The duration chip's dark glass and the thumbnail's timer line rest colour.
    pub const GLASS: Color = Color::from_rgba8(8, 12, 24, 0.72);
    pub const GLASS_LINE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.25);
    /// rose-400.
    pub const DANGER: Color = Color::from_rgb8(0xfb, 0x71, 0x85);
    /// amber-400/500 (older version / warning chip).
    pub const WARN: Color = Color::from_rgb8(0xf5, 0x9e, 0x0b);
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
    /// A code block's colours per `code_colors::Kind` (plain, keyword, string,
    /// number, comment, function, type) on the dark and on the light surface
    /// (each >= 4.5:1 on it: `code_kinds_read_on_their_surface`).
    pub const CODE_DARK_KINDS: [Color; 7] = [
        CODE_DARK.1,
        Color::from_rgb8(0xff, 0x7b, 0x72),
        Color::from_rgb8(0xa5, 0xd6, 0xff),
        Color::from_rgb8(0x79, 0xc0, 0xff),
        Color::from_rgb8(0x8b, 0x94, 0x9e),
        Color::from_rgb8(0xd2, 0xa8, 0xff),
        Color::from_rgb8(0xff, 0xa6, 0x57),
    ];
    pub const CODE_LIGHT_KINDS: [Color; 7] = [
        CODE_LIGHT.1,
        Color::from_rgb8(0xcf, 0x22, 0x2e),
        Color::from_rgb8(0x0a, 0x30, 0x69),
        Color::from_rgb8(0x05, 0x50, 0xae),
        Color::from_rgb8(0x57, 0x60, 0x6a),
        Color::from_rgb8(0x6f, 0x42, 0xc1),
        Color::from_rgb8(0x95, 0x38, 0x00),
    ];

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
    /// The dim over a window behind a modal dialog.
    pub const MODAL_DIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
    /// The squares behind a picture with a cut-out background (the same on
    /// any theme, like every editor's transparency grid).
    pub const CHECKER: (Color, Color) = (
        Color::from_rgb8(0xf4, 0xf4, 0xf5),
        Color::from_rgb8(0xd4, 0xd4, 0xd8),
    );
}

// === widgets style E ===
/// Floating-widget style E (two-tone, grouped segments): recessed `track`
/// tray with raised `widget` segments. Radii and the gap reuse `radius::*` / `space::GAP`.
pub mod widget_e {
    /// Outer tray padding, rim included (design: `.ws` 1 px border + 5 px margin).
    pub const TRAY_PAD_X: f32 = 6.0;
    pub const TRAY_PAD_Y: f32 = 6.0;
    /// Raised segment (`.gp`) inner padding and item gap.
    pub const SEGMENT_PAD_X: f32 = 5.0;
    /// The bar's (36 px pills in a 56 px tray); the Snip toolbar's is 3 (34 px buttons, 52 px tray),
    /// the translate pill's 1 (34 px chips, 48 px tray).
    pub const SEGMENT_PAD_Y: f32 = 4.0;
    pub const SEGMENT_PAD_Y_SNIP: f32 = 3.0;
    pub const SEGMENT_PAD_Y_PILL: f32 = 1.0;
    pub const SEGMENT_GAP: f32 = 2.0;
    /// The chat composer's round send / stop button (`.send`).
    pub const SEND: f32 = 30.0;
    /// Quiet close button (`.qx`).
    pub const QUIET_X: f32 = 28.0;
    /// Action pill height inside a segment (`.pl`).
    pub const PILL_H: f32 = 36.0;
    /// Size readout pill inside a segment (`.szc`).
    pub const SIZE_PILL_H: f32 = 30.0;
    /// Menu padding and row height (`.mn` / `.mr`).
    pub const MENU_PAD: f32 = 7.0;
    /// A floating menu's width (`.mn` 244 with the glossary's 272 / the language list's 264 apart)
    /// and its separator (`.mhr`: 1 px + 5 px above and below).
    pub const MENU_W: f32 = 244.0;
    pub const MENU_SEP_H: f32 = 11.0;
    pub const MENU_ITEM_H: f32 = 34.0;
    /// Current-row start indicator bar (`.mr.cur::before`).
    pub const CURRENT_BAR_W: f32 = 3.0;
    pub const CURRENT_BAR_H: f32 = 16.0;
    /// Steps strip (`.strip`, `.sth`, `.stp`, `.btnw`): padding/gap, header,
    /// header quiet buttons, thumbnail height, badge inset, action buttons.
    pub const STRIP_PAD: f32 = 8.0;
    /// The strip's 1 px rim (design `.strip`: 112 = 94 + 2 x 8 + 2, 418 = 4 steps).
    pub const STRIP_RIM: f32 = 1.0;
    pub const STRIP_TOP: f32 = 24.0;
    pub const STRIP_HEAD_BTN: f32 = 22.0;
    pub const STRIP_THUMB_W: f32 = 94.0;
    pub const STRIP_THUMB_H: f32 = 64.0;
    pub const STRIP_BADGE_INSET: f32 = 5.0;
    pub const STRIP_BTN_H: f32 = 36.0;
    pub const STRIP_BTN_RADIUS: f32 = 11.0;
    /// Live pin frame border (`.pinw`) and hover toolbar inset (`.pinbar`).
    pub const PIN_BORDER: f32 = 3.0;
    pub const PIN_BAR_INSET: f32 = 10.0;
    /// The live pin's corner (`.pinw`, 14 px).
    pub const PIN_CORNER: f32 = super::radius::DROPDOWN;
}
// === end widgets style E ===
