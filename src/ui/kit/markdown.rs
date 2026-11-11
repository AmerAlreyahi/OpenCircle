//! An AI answer as Markdown (the design's assistant message, `ai-chat.js`):
//! 13 px paragraphs in `text`, headings, lists, tables, inline `code` on
//! `hover`, links in `accent` that act only on a click, and code blocks on
//! `.oc-code-surface` (radius 12) with a header strip (the language, Copy).
//! iced's own parser and widgets (`markdown` feature) with no syntax
//! highlighter. Every paragraph, list item and quote has the direction of its
//! own first letter (an English answer stays left to right in an Arabic UI and
//! the other way round: bullets and quote bars at the side the text starts);
//! code is always left to right.
use iced::font::Weight;
use iced::widget::markdown::{self, Bullet, HeadingLevel, Item, Settings, Style, Text, Viewer};
use iced::widget::{column, container, rich_text, row, text};
use iced::{Border, Element, Fill, Padding, Pixels};

use super::button::{Kind, button, label};
use super::dir::{first_strong_rtl, rtl_letter};
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
    let size = Pixels(super::text::size(Role::CHAT));
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
        let (fill, ink) = match current().light {
            true => fixed::CODE_LIGHT,
            false => fixed::CODE_DARK,
        };
        let name = text(language.filter(|l| !l.is_empty()).unwrap_or("code"))
            .size(STRIP_TEXT)
            .font(mono(Weight::Normal))
            .color(c().text_sub);
        let copy = button(
            label(self.copy).size(STRIP_TEXT),
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
            .into()
    }
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
        .on_link_click(Answer::on_link_click);
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

/// `items` (parsed with `markdown::Content`) in the theme's text colour.
pub fn markdown<'a>(items: &'a [Item], copy: &'static str) -> Element<'a, Act> {
    container(markdown::view_with(items, settings(), &Answer { copy }))
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
        assert!("مرحبا".chars().all(rtl_letter) && "שלום".chars().all(rtl_letter));
        assert!(!"Hello, 123 «»".chars().any(rtl_letter));
    }
}
