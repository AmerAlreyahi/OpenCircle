//! An AI answer as Markdown (the design's assistant message, `ai-chat.js`):
//! 13 px paragraphs in `text`, headings, lists, tables, inline `code` on
//! `hover`, links in `accent` that act only on a click, and code blocks on
//! `.oc-code-surface` (radius 12) with a header strip (the language, Copy).
//! iced's own parser (`markdown` feature); the text is selectable with the
//! mouse across its blocks (`iced_selection`, `selection`: drag, double-click a word,
//! triple-click a line, Ctrl+C copies, in main mode's flat highlight) and code
//! is coloured by `code_colors` from the kit's tokens. Every paragraph, list
//! item and quote has the direction of its own first letter (an English answer
//! stays left to right in an Arabic UI and the other way round: bullets and
//! quote bars at the side the text starts); code is always left to right.
//! Sizes follow the text zoom (`text::zoomed`, the chat's Ctrl+= / Ctrl+-).
use iced::font::Weight;
use iced::widget::markdown::{self, Bullet, HeadingLevel, Item, Settings, Style, Text, Viewer};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text};
use iced::{Border, Element, Fill, Padding, Pixels};
use iced_selection::rich_text;

use super::button::{Kind, button, label};
use super::code_colors;
use super::dir::{first_strong_rtl, rtl_letter};
use super::text::{mono, ui_font, weight, zoom};
use super::themes::{c, current};
use super::tokens::{Role, fixed, radius};

/// What a click in an answer asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// A link (open it in the browser, if it is a web link).
    Link(String),
    /// A code block's Copy: its code.
    Copy(String),
}

/// The code block language of a formula made readable (the chat's maths).
pub const MATH: &str = "math";
/// Between blocks (`space-y-2.5`).
const SPACING: f32 = 10.0;
/// Code (`text-[11.5px]`) and its padding (`p-3`); the header strip (`px-3 py-1.5`).
const CODE: f32 = 11.5;
const CODE_PAD: f32 = 12.0;
const STRIP: [f32; 2] = [6.0, 12.0];
const STRIP_TEXT: f32 = 11.0;
/// Headings: h1, h2, the rest (bold, a little over the 13 px text).
const HEADINGS: [f32; 3] = [17.0, 15.0, 13.5];
/// Inline code's padding (`px-1.5 py-0.5`, laid on the line's height).
const INLINE_X: f32 = 3.0;
const INLINE_RADIUS: f32 = 4.0;

fn settings() -> Settings {
    let size = Pixels(super::text::size(Role::CHAT));
    let z = zoom();
    Settings {
        text_size: size,
        h1_size: Pixels(HEADINGS[0] * z),
        h2_size: Pixels(HEADINGS[1] * z),
        h3_size: Pixels(HEADINGS[2] * z),
        h4_size: size,
        h5_size: size,
        h6_size: size,
        code_size: Pixels(CODE * z),
        spacing: Pixels(SPACING * z),
        style: Style {
            font: ui_font(),
            inline_code_highlight: markdown::Highlight {
                background: c().hover.into(),
                border: Border::default().rounded(INLINE_RADIUS),
            },
            inline_code_padding: Padding::ZERO.left(INLINE_X).right(INLINE_X),
            inline_code_color: c().text,
            inline_code_font: mono(Weight::Normal),
            code_block_font: mono(Weight::Normal),
            link_color: c().accent,
        },
    }
}

/// The answer viewer; `copy` is the code blocks' Copy button (the app's language).
struct Answer {
    copy: &'static str,
}

impl<'a> Viewer<'a, Act> for Answer {
    fn on_link_click(url: markdown::Uri) -> Act {
        Act::Link(url)
    }

    fn heading(
        &self,
        settings: Settings,
        level: &'a HeadingLevel,
        line: &'a Text,
        _index: usize,
    ) -> Element<'a, Act> {
        let size = match level {
            HeadingLevel::H1 => settings.h1_size,
            HeadingLevel::H2 => settings.h2_size,
            HeadingLevel::H3 => settings.h3_size,
            _ => settings.text_size,
        };
        let style = Style {
            font: weight(Weight::Semibold),
            ..settings.style
        };
        block(line, style, size)
    }

    fn paragraph(&self, settings: Settings, line: &Text) -> Element<'a, Act> {
        block(line, settings.style, settings.text_size)
    }

    fn table(
        &self,
        settings: Settings,
        columns: &'a [markdown::Column],
        rows: &'a [markdown::Row],
    ) -> Element<'a, Act> {
        // (iced's own: columns as wide as their cells, a wide one scrolls across)
        markdown::table(&Cell, settings, columns, rows)
    }

    fn unordered_list(&self, settings: Settings, bullets: &'a [Bullet]) -> Element<'a, Act> {
        let list = list(self, settings, bullets, |_, _| {
            text("•").size(settings.text_size).font(ui_font()).into()
        });
        container(list).padding([0.0, settings.spacing.0]).into()
    }

    fn ordered_list(
        &self,
        settings: Settings,
        start: u64,
        bullets: &'a [Bullet],
    ) -> Element<'a, Act> {
        let digits = ((start + bullets.len() as u64).max(1) as f32)
            .log10()
            .ceil();
        list(self, settings, bullets, move |i, rtl| {
            let side = match rtl {
                true => iced::alignment::Horizontal::Left,
                false => iced::alignment::Horizontal::Right,
            };
            text(format!("{}.", i as u64 + start))
                .size(settings.text_size)
                .font(ui_font())
                .align_x(side)
                .width(settings.text_size * ((digits / 2.0).ceil() + 1.0))
                .into()
        })
    }

    fn quote(&self, settings: Settings, contents: &'a [Item]) -> Element<'a, Act> {
        let rtl = contents
            .first()
            .is_some_and(|i| item_rtl(i, settings.style));
        let body = column(
            contents
                .iter()
                .enumerate()
                .map(|(i, content)| markdown::item(self, settings, content, i)),
        )
        .spacing(settings.spacing.0);
        let bar = iced::widget::rule::vertical(4);
        let parts: [Element<'a, Act>; 2] = match rtl {
            true => [body.into(), bar.into()],
            false => [bar.into(), body.into()],
        };
        row(parts)
            .height(iced::Length::Shrink)
            .spacing(settings.spacing.0)
            .into()
    }

    fn code_block(
        &self,
        settings: Settings,
        language: Option<&'a str>,
        code: &'a str,
        lines: &'a [Text],
    ) -> Element<'a, Act> {
        let (fill, kinds) = match current().light {
            true => (fixed::CODE_LIGHT.0, fixed::CODE_LIGHT_KINDS),
            false => (fixed::CODE_DARK.0, fixed::CODE_DARK_KINDS),
        };
        // a formula (`ask_ai::math`): a quiet monospace block, no strip
        if language == Some(MATH) {
            let maths = rich_text([iced::advanced::text::Span::<markdown::Uri>::new(
                code.trim_end().to_string(),
            )])
            .font(settings.style.code_block_font)
            .size(settings.text_size)
            .wrapping(Wrapping::None)
            .style(selectable)
            .click_interval(CLICK_GAP);
            let maths = super::scroll::across(container(maths).padding(CODE_PAD * zoom()));
            return container(maths)
                .width(Fill)
                .clip(true)
                .style(|_| container::Style {
                    background: Some(c().hover.into()),
                    border: Border::default().rounded(radius::CARD),
                    ..container::Style::default()
                })
                .into();
        }
        let name = text(language.filter(|l| !l.is_empty()).unwrap_or("code"))
            .size(STRIP_TEXT * zoom())
            .font(mono(Weight::Normal))
            .color(c().text_sub);
        let copy = button(
            label(self.copy).size(STRIP_TEXT * zoom()),
            Kind::Quiet,
            Some(Act::Copy(code.trim_end().to_string())),
        );
        let strip = container(row![container(name).width(Fill), copy])
            .padding(STRIP)
            .style(|_| container::Style::default().background(c().hover));
        let code = rich_text(code_spans(language, code, lines, settings.style, kinds))
            .font(settings.style.code_block_font)
            .size(settings.code_size)
            .wrapping(Wrapping::None)
            .style(selectable)
            .click_interval(CLICK_GAP);
        let code = super::scroll::across(container(code).padding(CODE_PAD * zoom()));
        container(column![strip, code])
            .width(Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(fill.into()),
                border: Border::default().rounded(radius::CARD),
                ..container::Style::default()
            })
            .into()
    }
}

/// What a bullet holds.
fn items(bullet: &Bullet) -> &[Item] {
    match bullet {
        Bullet::Point { items } | Bullet::Task { items, .. } => items,
    }
}

/// A list: each bullet's marker at the side its own text starts (iced's own
/// list layout, mirrored per bullet).
fn list<'a>(
    viewer: &Answer,
    settings: Settings,
    bullets: &'a [Bullet],
    marker: impl Fn(usize, bool) -> Element<'a, Act>,
) -> Element<'a, Act> {
    column(bullets.iter().enumerate().map(|(i, bullet)| {
        let rtl = (items(bullet).first()).is_some_and(|item| item_rtl(item, settings.style));
        let mark = match bullet {
            Bullet::Task { done, .. } => Element::from(
                container(iced::widget::checkbox(*done).size(settings.text_size)).center_y(
                    iced::widget::text::LineHeight::default().to_absolute(settings.text_size),
                ),
            ),
            Bullet::Point { .. } => marker(i, rtl),
        };
        let inner = Settings {
            spacing: settings.spacing * 0.6,
            ..settings
        };
        let body = markdown::view_with(items(bullet), inner, viewer);
        let parts: [Element<'a, Act>; 2] = match rtl {
            true => [container(body).width(Fill).into(), mark],
            false => [mark, body],
        };
        row(parts).spacing(settings.spacing).into()
    }))
    .spacing(settings.spacing * 0.75)
    .into()
}

/// An item starts right to left (a paragraph or heading by its first letter).
fn item_rtl(item: &Item, style: Style) -> bool {
    match item {
        Item::Paragraph(line) | Item::Heading(_, line) => {
            let spans = line.spans(style);
            first_strong_rtl(&spans.iter().map(|s| s.text.as_ref()).collect::<String>())
        }
        _ => false,
    }
}

/// A table's cell: as wide as its text (a table sizes its columns to them).
struct Cell;

impl<'a> Viewer<'a, Act> for Cell {
    fn on_link_click(url: markdown::Uri) -> Act {
        Act::Link(url)
    }

    fn paragraph(&self, settings: Settings, line: &Text) -> Element<'a, Act> {
        rich_text(upright(line, settings.style))
            .size(settings.text_size)
            .on_link_click(Self::on_link_click)
            .style(selectable)
            .click_interval(CLICK_GAP)
            .into()
    }
}

/// How soon a second click counts as a double click (iced's own 300 ms). A test builds
/// the whole window between its clicks, which on a loaded machine takes longer.
const CLICK_GAP: std::time::Duration =
    std::time::Duration::from_millis(if cfg!(test) { 60_000 } else { 300 });

/// The selection's look: main mode's flat highlight, the text's own colour.
fn selectable(_: &iced::Theme) -> iced_selection::text::Style {
    iced_selection::text::Style {
        color: None,
        selection: fixed::TEXT_SELECTION,
    }
}

/// A code block's spans: its lines coloured by kind when the language is
/// known (`code_colors`), else iced's plain lines in the plain colour.
fn code_spans(
    language: Option<&str>,
    code: &str,
    lines: &[Text],
    style: Style,
    kinds: [iced::Color; 7],
) -> Vec<iced::advanced::text::Span<'static, markdown::Uri>> {
    use iced::advanced::text::Span;
    let ink = kinds[code_colors::Kind::Plain as usize];
    let Some(coloured) = language.and_then(|l| code_colors::highlight(l, code.trim_end())) else {
        let plain = lines.iter().map(|line| {
            let text: String = line.spans(style).iter().map(|s| s.text.as_ref()).collect();
            text
        });
        return vec![Span::new(plain.collect::<Vec<_>>().join("\n")).color(ink)];
    };
    let mut spans = Vec::new();
    for (i, line) in coloured.iter().enumerate() {
        if i > 0 {
            spans.push(Span::new("\n"));
        }
        spans.extend(
            (line.iter())
                .map(|(kind, piece)| Span::new(piece.clone()).color(kinds[*kind as usize])),
        );
    }
    spans
}

/// A paragraph or heading: across the width, wrapped, and laid from the
/// right when it starts in a right-to-left script (Arabic). Arabic in an
/// emphasis stays upright: no Arabic font has an italic, and cosmic-text
/// falls back only to a face of the same style (it would draw boxes).
fn block<'a>(line: &Text, style: Style, size: Pixels) -> Element<'a, Act> {
    let spans = upright(line, style);
    let rtl = first_strong_rtl(&spans.iter().map(|s| s.text.as_ref()).collect::<String>());
    let block = rich_text(spans)
        .size(size)
        .width(Fill)
        .on_link_click(Answer::on_link_click)
        .style(selectable)
        .click_interval(CLICK_GAP);
    match rtl {
        true => block.align_x(iced::alignment::Horizontal::Right).into(),
        false => block.into(),
    }
}

/// `line`'s spans, Arabic in an emphasis upright.
fn upright(line: &Text, style: Style) -> Vec<iced::advanced::text::Span<'static, markdown::Uri>> {
    (line.spans(style).iter())
        .map(|span| {
            let mut span = span.clone();
            if let Some(font) = &mut span.font
                && font.style != iced::font::Style::Normal
                && span.text.chars().any(rtl_letter)
            {
                font.style = iced::font::Style::Normal;
            }
            span
        })
        .collect()
}

/// `items` (parsed with `markdown::Content`) in the theme's text colour, one
/// selection over all its blocks (`selection`).
pub fn markdown<'a>(items: &'a [Item], copy: &'static str) -> Element<'a, Act> {
    let answer = container(markdown::view_with(items, settings(), &Answer { copy }))
        .width(Fill)
        .style(|_| container::Style::default().color(c().text));
    selection(answer)
}

/// `content`'s selectable texts as one selection (W52): a drag selects across
/// paragraphs, list items, headings and code blocks, and Ctrl+C copies all of
/// it at once, in order (`iced_selection::group`). Nested: the outermost writes.
pub fn selection<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    super::wrap::wrap(content, Selection)
}

struct Selection;

impl<M> super::wrap::Hook<M> for Selection {
    fn update(
        &mut self,
        content: &mut Element<'_, M>,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, M>,
        viewport: &iced::Rectangle,
    ) {
        use iced::{Event, mouse, touch};
        iced_selection::group::enter();
        content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        let released = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
        );
        if let Some(text) = iced_selection::group::leave(released) {
            clipboard.write(iced::advanced::clipboard::Kind::Standard, text);
            shell.capture_event();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_code_come_back_as_acts() {
        assert_eq!(Answer::on_link_click("https://a.example".into()), {
            Act::Link("https://a.example".into())
        });
        let content = markdown::Content::parse("```rust\nfn main() {}\n```\n");
        let Item::CodeBlock { language, code, .. } = &content.items()[0] else {
            panic!("a code block");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(code.trim_end(), "fn main() {}");
    }

    #[test]
    fn right_to_left_letters_are_found() {
        assert!("مرحبا".chars().all(rtl_letter) && "שלום".chars().all(rtl_letter));
        assert!(!"Hello, 123 «»".chars().any(rtl_letter));
    }

    use crate::shared::kit::testing::shot::{self, Clip};
    use iced::{Point, Size};

    const SIZE: Size = Size::new(400.0, 120.0);

    /// `md` rendered, after a drag (`clicks` presses first) and Ctrl+C:
    /// what was copied, the acts, and which pixels the selection changed.
    fn select(
        md: &str,
        from: Point,
        to: Point,
        clicks: usize,
    ) -> (Vec<String>, Vec<Act>, Vec<usize>) {
        let content = markdown::Content::parse(md);
        let view = || markdown(content.items(), "Copy");
        let mut clip = Clip::default();
        let (acts, after) =
            shot::drag(view, SIZE, (from, to, clicks), &shot::chord('c'), &mut clip);
        let mut none = Clip::default();
        let away = Point::new(1.0, SIZE.height - 1.0);
        let (_, before) = shot::drag(view, SIZE, (away, away, 1), &[], &mut none);
        let changed = (0..(SIZE.width * SIZE.height) as usize)
            .filter(|i| after[i * 4..i * 4 + 3] != before[i * 4..i * 4 + 3])
            .map(|i| i % SIZE.width as usize)
            .collect();
        (clip.0, acts, changed)
    }

    /// #126: a drag selects within a paragraph, Ctrl+C copies just that, and
    /// the selection is drawn.
    #[test]
    fn a_drag_selects_and_ctrl_c_copies_it() {
        let text = "Hello wide world of answers";
        let (copied, _, changed) = select(text, Point::new(1.0, 8.0), Point::new(60.0, 8.0), 1);
        assert_eq!(copied.len(), 1, "{copied:?}");
        assert!(
            text.starts_with(&copied[0]) && copied[0].len() < text.len(),
            "{copied:?}"
        );
        assert!(
            !changed.is_empty() && changed.iter().all(|&x| x <= 62),
            "drawn where dragged"
        );
        // a code block's line too (left to right, its own block)
        let (copied, ..) = select(
            "```\nlet x = 1;\n```",
            Point::new(14.0, 48.0),
            Point::new(390.0, 48.0),
            3,
        );
        assert_eq!(copied, ["let x = 1;"]);
    }

    /// Arabic: a line selects whole (triple click), and a drag from its start
    /// (the right) selects its first words, drawn at the right; upstream drew
    /// no highlight (grapheme counts against byte offsets, left-to-right glyphs).
    #[test]
    fn arabic_selects_in_its_own_order_and_is_drawn_where_it_is() {
        let _ar = crate::shared::kit::testing::arabic();
        let text = "مرحبا بالعالم الجميل اليوم";
        let (copied, _, changed) = select(text, Point::new(398.0, 8.0), Point::new(398.0, 8.0), 3);
        assert_eq!(copied, [text]);
        let (lo, hi) = (changed.iter().min().copied(), changed.iter().max().copied());
        assert!(
            lo.is_some_and(|x| x < 300) && hi.is_some_and(|x| x > 380),
            "{lo:?}..{hi:?}"
        );
        let (copied, _, changed) = select(text, Point::new(399.0, 8.0), Point::new(340.0, 8.0), 1);
        assert_eq!(copied.len(), 1);
        assert!(
            text.starts_with(&copied[0]) && copied[0].len() < text.len(),
            "{copied:?}"
        );
        assert!(
            !changed.is_empty() && changed.iter().all(|&x| x >= 336),
            "drawn at the right"
        );
    }

    /// W52: a drag selects across paragraphs, a list and a code block, and
    /// Ctrl+C copies all of it once, in order, the code as written.
    #[test]
    fn a_drag_selects_across_blocks_and_copies_them_once() {
        let md = "First para here\n\n- item one\n- item two\n\n```\nlet x = 1;\n```\n\nLast para";
        let size = Size::new(400.0, 260.0);
        let content = markdown::Content::parse(md);
        let view = || markdown(content.items(), "Copy");
        let mut clip = Clip::default();
        let _ = shot::drag(
            view,
            size,
            (Point::new(1.0, 8.0), Point::new(390.0, 255.0), 1),
            &shot::chord('c'),
            &mut clip,
        );
        assert_eq!(clip.0.len(), 1, "one write: {:?}", clip.0);
        let copied = &clip.0[0];
        for part in [
            "First para here",
            "item one",
            "item two",
            "let x = 1;",
            "Last para",
        ] {
            assert!(copied.contains(part), "{part:?} in {copied:?}");
        }
        assert!(copied.find("item one") < copied.find("let x"), "{copied:?}");
        // a drag from the middle of one paragraph to the middle of the next
        let content = markdown::Content::parse("alpha beta gamma\n\ndelta epsilon zeta");
        let view = || markdown(content.items(), "Copy");
        let mut clip = Clip::default();
        let _ = shot::drag(
            view,
            SIZE,
            (Point::new(45.0, 8.0), Point::new(45.0, 34.0), 1),
            &shot::chord('c'),
            &mut clip,
        );
        assert_eq!(clip.0.len(), 1, "{:?}", clip.0);
        let (a, b) = clip.0[0].split_once('\n').expect("two parts");
        assert!(!a.starts_with("alpha") && a.ends_with("gamma"), "{a:?}");
        assert!(b.starts_with("delta") && !b.ends_with("zeta"), "{b:?}");
    }

    /// A link acts on a click, never at the end of a drag over it.
    #[test]
    fn a_link_opens_on_a_click_not_a_drag() {
        let md = "[a link here](https://a.example) and more";
        let at = Point::new(10.0, 8.0);
        let (_, acts, _) = select(md, at, at, 1);
        assert_eq!(acts, [Act::Link("https://a.example".into())]);
        let (copied, acts, _) = select(md, at, Point::new(150.0, 8.0), 1);
        assert!(acts.is_empty() && !copied.is_empty(), "{acts:?} {copied:?}");
    }

    /// Code is coloured by kind from the theme's tokens; an unknown language
    /// is plain, every character kept.
    #[test]
    fn code_is_coloured_from_the_tokens() {
        let kinds = fixed::CODE_DARK_KINDS;
        let style = settings().style;
        let code = "fn main() {}\n";
        let spans = code_spans(Some("rust"), code, &[], style, kinds);
        let text: String = spans.iter().map(|s| s.text.as_ref()).collect();
        assert_eq!(text, code.trim_end());
        let keyword = kinds[code_colors::Kind::Keyword as usize];
        assert!(
            spans
                .iter()
                .any(|s| s.text == "fn" && s.color == Some(keyword)),
            "{spans:?}"
        );
        let content = markdown::Content::parse("```nolang\na <b> c\n```");
        let Item::CodeBlock { lines, code, .. } = &content.items()[0] else {
            panic!("a code block");
        };
        let plain = code_spans(Some("nolang"), code, lines, style, kinds);
        assert_eq!(plain.len(), 1);
        assert_eq!(
            (plain[0].text.as_ref(), plain[0].color),
            ("a <b> c", Some(kinds[0]))
        );
    }

    /// Code colours read on their surface in both themes (>= 4.5:1).
    #[test]
    fn code_kinds_read_on_their_surface() {
        let over = |(fill, _): (iced::Color, iced::Color), page: iced::Color| {
            let mix = |a: f32, b: f32| a * fill.a + b * (1.0 - fill.a);
            iced::Color::from_rgb(
                mix(fill.r, page.r),
                mix(fill.g, page.g),
                mix(fill.b, page.b),
            )
        };
        for (surface, kinds) in [
            (
                over(fixed::CODE_DARK, iced::Color::from_rgb8(0x1e, 0x22, 0x2c)),
                fixed::CODE_DARK_KINDS,
            ),
            (
                over(fixed::CODE_LIGHT, iced::Color::WHITE),
                fixed::CODE_LIGHT_KINDS,
            ),
        ] {
            for k in kinds {
                let ratio = crate::shared::kit::themes::contrast(k, surface);
                assert!(ratio >= 4.5, "{k:?} on {surface:?}: {ratio:.2}");
            }
        }
    }

    /// The text zoom scales every size of an answer.
    #[test]
    fn the_zoom_scales_the_answer() {
        let plain = settings();
        let big = crate::shared::kit::text::zoomed(1.5, settings);
        assert_eq!(big.text_size.0, plain.text_size.0 * 1.5);
        assert_eq!(
            (big.h1_size.0, big.code_size.0),
            (plain.h1_size.0 * 1.5, plain.code_size.0 * 1.5)
        );
        assert_eq!(
            settings().text_size,
            plain.text_size,
            "only inside the scope"
        );
    }
}
