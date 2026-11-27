//! A feature's preview at the top of its Settings page (Settings round 2 `.r2 .sec.exp`,
//! `r2.py` `drawer` / `sticky`): open by default with the bundled still frame, "How it works"
//! eyebrow, title, description, shortcut keycaps, Play/Replay button with progress track, clip
//! pager (`<` dots `>` + "1 of N") when there are multiple clips, and a calm note. The clip
//! itself downloads only when Play is pressed.
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use iced::advanced::image::{FilterMethod, Image, Renderer as _};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::Tree;
use iced::advanced::{Widget, mouse, renderer};
use iced::widget::image::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, row};
use iced::{Border, Center, Color, Element, Fill, Length, Rectangle, Size};

use super::button::{Kind, button, label};
use super::icon::{Icon, glyph};
use super::icon_button::{self, icon_button};
use super::keycap::keycap;
use super::progress;
use super::text::{accent, muted, paragraph, sub, text};
use super::themes::{c, current};
use super::tokens::{Role, radius, settings, size, space};
use super::tooltip::{Why, hint};

/// A keycap's sides round its label.
const CAP_PAD: f32 = 16.0;
/// The clips' shape (1920x1080).
const ASPECT: f32 = 16.0 / 9.0;
/// Active / inactive pager dot sizes (`.clips i`, `.clips i.on`).
const DOT: f32 = 6.0;
const DOT_ON: f32 = 16.0;

/// What a preview shows.
pub struct Hero<'a, Message> {
    pub title: &'a str,
    pub desc: &'a str,
    /// A Labs feature: "Labs" after the name.
    pub lab: bool,
    /// The feature's shortcut, one label per key (empty = not set).
    pub keys: Vec<String>,
    /// The stage is shown.
    pub shown: bool,
    /// Hide / Show preview; `None` = no clip yet.
    pub toggle: Option<Message>,
    /// Play / Replay / Downloading button: `(label, on_press)`.
    pub play: Option<(&'a str, Option<Message>)>,
    /// Whether the preview sits in the narrow sticky side column (`beside`).
    pub beside: bool,
    /// One calm line under the controls (a download that failed, or the default resting note).
    pub note: Option<&'a str>,
    /// The frame now (`None`: not decoded yet, the stage stays dark).
    pub frame: Option<Handle>,
    /// Where the stage tells its width in px (the frames are made that size),
    /// the window's scale factor and its height (logical px).
    pub width: (Arc<AtomicU32>, f32, f32),
    /// Several clips: `(the one showing, how many, previous, next, "1 of N" text)`.
    pub pager: Option<(usize, usize, Message, Message, String)>,
    /// The hero's own words, in the app's language.
    pub words: Words<'a>,
}

/// What the hero writes itself.
#[derive(Debug, Clone, Copy)]
pub struct Words<'a> {
    pub eyebrow: &'a str,
    pub labs: &'a str,
    pub no_shortcut: &'a str,
    pub previous: &'a str,
    pub next: &'a str,
    pub default_note: &'a str,
}

impl Default for Words<'_> {
    fn default() -> Self {
        Words {
            eyebrow: "How it works",
            labs: "Labs",
            no_shortcut: "Shortcut not set",
            previous: "Previous preview",
            next: "Next preview",
            default_note: "Plays once when opened, then rests on the last frame. A still picture if you reduce motion.",
        }
    }
}

pub fn hero<'a, Message: Clone + 'a>(h: Hero<'a, Message>) -> Element<'a, Message> {
    let w = h.words;
    let has_clip = h.toggle.is_some();

    let mut name_row = vec![text(Role::DEMO_TITLE, h.title).into()];
    if h.lab {
        name_row.push(sub(Role::CARD_DESC, w.labs).into());
    }
    let title_line = super::dir::row(name_row)
        .spacing(space::GAP)
        .align_y(iced::Bottom);

    // (the key caps keep their order: a shortcut reads left to right)
    let keys: Element<'a, Message> = match h.keys.is_empty() {
        true => sub(Role::CARD_DESC, w.no_shortcut).into(),
        false => row(h.keys.into_iter().map(|k| {
            let kw = super::text::width(
                &k,
                super::text::size(Role::KEYCAP),
                super::text::font(Role::KEYCAP),
            );
            container(keycap(k)).width(kw + CAP_PAD).into()
        }))
        .spacing(space::TRACK_PAD)
        .into(),
    };

    let play_btn = h.play.map(|(l, m)| {
        let icon_el = glyph(Icon::Refresh.glyph(), size::ICON_SMALL);
        button(
            super::dir::row([icon_el.into(), label(l).into()])
                .spacing(6)
                .align_y(Center),
            Kind::Elevated,
            m,
        )
    });

    let mut info = column![
        accent(Role::SECTION, w.eyebrow),
        title_line,
        paragraph(sub(Role::ROW_DESC, h.desc)),
        container(keys).padding(iced::Padding::ZERO.top(4).bottom(2)),
    ]
    .spacing(6)
    .width(Fill)
    .align_x(super::dir::start());

    if let Some(btn) = play_btn {
        let bar = container(progress::bar(if h.shown && h.frame.is_some() {
            1.0
        } else {
            0.0
        }))
        .width(Fill);
        info = info.push(
            super::dir::row([btn, bar.into()])
                .spacing(10)
                .align_y(Center),
        );
    }

    if let Some((at, n, prev, next, of_text)) = h.pager {
        info = info.push(pager_row(at, n, prev, next, of_text, w));
    }

    if has_clip {
        let note_text = h.note.unwrap_or(w.default_note);
        info = info.push(paragraph(muted(Role::ROW_DESC, note_text)));
    }

    let body: Element<'a, Message> = if !has_clip {
        info.into()
    } else if h.beside {
        let media = media_box(h.frame, h.width, None);
        column![media, info].spacing(12).width(Fill).into()
    } else {
        let media = media_box(h.frame, h.width, Some((settings::DEMO_W, settings::DEMO_H)));
        super::dir::row([media, info.into()])
            .spacing(settings::DEMO_GAP)
            .align_y(Center)
            .into()
    };

    container(body)
        .padding(settings::DEMO_PAD)
        .width(Fill)
        .style(|_| {
            let bg = if current().light {
                c().bg
            } else {
                Color::from_rgba8(5, 12, 28, 0.35)
            };
            container::Style {
                background: Some(bg.into()),
                border: Border::default()
                    .rounded(settings::SECTION_RADIUS)
                    .width(1)
                    .color(c().divider),
                ..container::Style::default()
            }
        })
        .into()
}

fn media_box<'a, Message: 'a>(
    frame: Option<Handle>,
    (width, scale, win_h): (Arc<AtomicU32>, f32, f32),
    fixed: Option<(f32, f32)>,
) -> Element<'a, Message> {
    let media: Element<'a, Message> = Element::new(Media {
        frame,
        width,
        scale,
        win_h,
        fixed,
    });
    let box_ = container(media).clip(true).style(|_| container::Style {
        background: Some(c().track_glass.into()),
        border: Border::default()
            .rounded(radius::CARD)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    });
    match fixed {
        Some((w, h)) => box_.width(w).height(h).into(),
        None => box_.width(Fill).into(),
    }
}

/// The arrows, dots (active 16 px pill in accent, inactive 6 px in rim) and "1 of N" count.
fn pager_row<'a, Message: Clone + 'a>(
    at: usize,
    n: usize,
    prev: Message,
    next: Message,
    of_text: String,
    w: Words<'a>,
) -> Element<'a, Message> {
    let arrow = |glyph_ic: Icon, tip_label: &'a str, m: Message| {
        let b = icon_button(
            glyph_ic.directed().glyph(),
            icon_button::State::default(),
            Some(m),
        );
        hint(b, tip_label, Why::Icon, Position::Top)
    };
    let dots = row((0..n).map(|i| {
        let on = i == at;
        container(Space::new())
            .width(if on { DOT_ON } else { DOT })
            .height(DOT)
            .style(move |_| container::Style {
                background: Some(if on { c().accent } else { c().rim }.into()),
                border: Border::default().rounded(radius::FULL),
                ..container::Style::default()
            })
            .into()
    }))
    .spacing(space::GAP);
    super::dir::row([
        arrow(Icon::Back, w.previous, prev),
        dots.into(),
        arrow(Icon::Forward, w.next, next),
        sub(Role::META, of_text).into(),
    ])
    .spacing(space::GAP)
    .align_y(Center)
    .into()
}

/// Largest stage width (logical px) and the share of the window's height it
/// may take, so the rows under it stay in view (owner, 2026-11-04).
const MAX_W: f32 = 820.0;
const HEIGHT_SHARE: f32 = 0.42;
/// The smallest stage (the old 353x200).
const MIN_W: f32 = 353.0;
/// The clips' native width in px.
const NATIVE: f32 = 1920.0;

/// The frame's width in logical px: the page's content width `avail`, at most
/// 820, at most 42 % of the window height `win_h` x 16/9, at least 353 (when
/// the page is that wide), never above the clip's native px at `scale`.
pub fn stage_width(avail: f32, win_h: f32, scale: f32) -> f32 {
    let w = avail
        .min(MAX_W)
        .min(HEIGHT_SHARE * win_h * ASPECT)
        .max(MIN_W)
        .min(NATIVE / scale.max(0.1));
    w.min(avail).round()
}

/// The frame: at the stage's smart size (or fixed `DEMO_W x DEMO_H` in the drawer), 16:9;
/// the width it gets laid out at, in px, goes to `width` for the decoder.
struct Media {
    frame: Option<Handle>,
    width: Arc<AtomicU32>,
    scale: f32,
    win_h: f32,
    fixed: Option<(f32, f32)>,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Media {
    fn size(&self) -> Size<Length> {
        match self.fixed {
            Some((w, h)) => Size::new(Length::Fixed(w), Length::Fixed(h)),
            None => Size::new(Length::Fill, Length::Shrink),
        }
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let (w, h) = match self.fixed {
            Some((fw, fh)) => (fw.min(limits.max().width), fh),
            None => {
                let avail = limits.max().width;
                let avail = if avail.is_finite() { avail } else { 340.0 };
                let w = stage_width(avail, self.win_h, self.scale);
                (w, (w / ASPECT).round())
            }
        };
        let px = (w * self.scale).round().max(1.0) as u32;
        self.width.store(px, Ordering::Relaxed);
        layout::Node::new(Size::new(w, h))
    }

    fn draw(
        &self,
        _: &Tree,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let Some(frame) = &self.frame else {
            return;
        };
        let bounds = layout.bounds();
        let image = Image {
            filter_method: FilterMethod::Linear,
            border_radius: radius::CARD.into(),
            snap: true,
            ..Image::new(frame.clone())
        };
        renderer.draw_image(image, bounds, bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::stage_width;

    #[test]
    fn the_stage_has_a_smart_size() {
        for (win_w, win_h, want) in [
            (800.0, 600.0, 448.0),
            (800.0, 400.0, 353.0),
            (1280.0, 720.0, 538.0),
            (1920.0, 1080.0, 806.0),
            (3440.0, 1440.0, 820.0),
        ] {
            let w = stage_width(win_w - 160.0, win_h, 1.0);
            assert_eq!(w, f32::min(want, win_w - 160.0), "{win_w}x{win_h}");
        }
        assert_eq!(stage_width(1760.0, 1080.0, 1.5), 806.0);
        assert_eq!(stage_width(3000.0, 3000.0, 3.0), 640.0);
        assert_eq!(stage_width(300.0, 600.0, 1.0), 300.0);
    }
}
