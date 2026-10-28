//! An AI answer as Markdown (the design's assistant message, `ai-chat.js`):
//! 13 px paragraphs in `text`, headings, lists, tables, inline `code` on
//! `hover`, links in `accent` that act only on a click, and code blocks on
//! `.oc-code-surface` (radius 12) with a header strip (the language, Copy).
//! iced's own parser and widgets (`markdown` feature) with no syntax
//! highlighter. Right-to-left lines (Arabic) shape and align by their own
//! direction (cosmic-text bidi); the layout stays left to right.
use iced::font::Weight;
use iced::widget::markdown::{self, HeadingLevel, Item, Settings, Style, Text, Viewer};
use iced::widget::{column, container, rich_text, row, text};
use iced::{Border, Element, Fill, Padding, Pixels};

use super::button::{Kind, button, label};
use super::text::{mono, ui_font, weight};
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
    let size = Pixels(Role::CHAT.size);
    Settings {
        text_size: size,
        h1_size: Pixels(HEADINGS[0]),
        h2_size: Pixels(HEADINGS[1]),
        h3_size: Pixels(HEADINGS[2]),
        h4_size: size,
        h5_size: size,
        h6_size: size,
        code_size: Pixels(CODE),
        spacing: Pixels(SPACING),
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

struct Answer;

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
        markdown::table(&Cell, settings, columns, rows)
    }

    fn code_block(
        &self,
        settings: Settings,
        language: Option<&'a str>,
        code: &'a str,
        lines: &'a [Text],
    ) -> Element<'a, Act> {
        let (fill, ink) = match current().light {
            true => fixed::CODE_LIGHT,
            false => fixed::CODE_DARK,
        };
        let name = text(language.filter(|l| !l.is_empty()).unwrap_or("code"))
            .size(STRIP_TEXT)
            .font(mono(Weight::Normal))
            .color(c().text_sub);
        let copy = button(
            label("Copy").size(STRIP_TEXT),
            Kind::Quiet,
            Some(Act::Copy(code.trim_end().to_string())),
        );
        let strip = container(row![container(name).width(Fill), copy])
            .padding(STRIP)
            .style(|_| container::Style::default().background(c().hover));
        let code = column(lines.iter().map(|line| {
            rich_text(line.spans(settings.style))
                .font(settings.style.code_block_font)
                .size(settings.code_size)
                .color(ink)
                .into()
        }));
        let code = super::scroll::across(container(code).padding(CODE_PAD));
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
            .into()
    }
}

/// A paragraph or heading: across the width, wrapped, and laid from the
/// right when it starts in a right-to-left script (Arabic). Arabic in an
/// emphasis stays upright: no Arabic font has an italic, and cosmic-text
/// falls back only to a face of the same style (it would draw boxes).
fn block<'a>(line: &Text, style: Style, size: Pixels) -> Element<'a, Act> {
    let spans = upright(line, style);
    let first = (spans.iter().flat_map(|s| s.text.chars())).find(|c| c.is_alphabetic());
    let block = rich_text(spans)
        .size(size)
        .on_link_click(Answer::on_link_click);
    // As wide as its text, moved to the right edge: iced 0.14 draws a
    // one-line right-to-left text with `align_x(Right)` past the edge.
    match first.is_some_and(rtl) {
        true => container(block).align_right(Fill).into(),
        false => block.width(Fill).into(),
    }
}

/// `line`'s spans, Arabic in an emphasis upright.
fn upright(line: &Text, style: Style) -> Vec<iced::advanced::text::Span<'static, markdown::Uri>> {
    (line.spans(style).iter())
        .map(|span| {
            let mut span = span.clone();
            if let Some(font) = &mut span.font
                && font.style != iced::font::Style::Normal
                && span.text.chars().any(rtl)
            {
                font.style = iced::font::Style::Normal;
            }
            span
        })
        .collect()
}

/// A letter of a right-to-left script (Hebrew, Arabic and their presentation forms).
fn rtl(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// `items` (parsed with `markdown::Content`) in the theme's text colour.
pub fn markdown<'a>(items: &'a [Item]) -> Element<'a, Act> {
    container(markdown::view_with(items, settings(), &Answer))
        .width(Fill)
        .style(|_| container::Style::default().color(c().text))
        .into()
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
        assert!("مرحبا".chars().all(rtl) && "שלום".chars().all(rtl));
        assert!(!"Hello, 123 «»".chars().any(rtl));
    }
}
