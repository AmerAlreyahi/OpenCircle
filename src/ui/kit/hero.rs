//! A feature's preview at the top of its Settings page (`settings-window.js`
//! `featureHero`, `.oc-feature-*`): the feature's name and one line, its
//! shortcut as keycaps, Hide / Show preview, and the stage: a "Preview" chip,
//! the "Play preview" button while the clips aren't on this PC (owner,
//! 2026-11-04), and the frame (a still, or the clips playing, by the caller),
//! 16:9, as wide as the page. Without a clip (`toggle: None`; owner,
//! 2026-10-30) there is only the heading: no Hide / Show button and no stage.
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use iced::advanced::image::{FilterMethod, Image, Renderer as _};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::Tree;
use iced::advanced::{Widget, mouse, renderer};
use iced::widget::image::Handle;
use iced::widget::{Space, column, container, row, text};
use iced::{Border, Center, Element, Fill, Length, Rectangle, Size};

use super::button::{Kind, button, label};
use super::icon::Icon;
use super::icon_button::{self, icon_button};
use super::keycap::keycap;
use super::text::sub;
use super::themes::c;
use super::tokens::{Role, radius, space};
use super::tooltip::{Why, hint};
use iced::widget::tooltip::Position;

/// The card's inside (`padding: 10px`) and the stage's (`8px`).
const PAD: f32 = 10.0;
const STAGE_PAD: f32 = 8.0;
/// The chip's type (9 px).
const SMALL: f32 = 9.0;
/// A keycap's sides round its label.
const CAP_PAD: f32 = 16.0;
/// The clips' shape (1920x1080).
const ASPECT: f32 = 16.0 / 9.0;

/// What a preview shows.
pub struct Hero<'a, Message> {
    pub title: &'a str,
    pub desc: &'a str,
    /// A Labs feature: "Labs" after the name.
    pub lab: bool,
    /// The feature's shortcut, one label per key (empty = not set).
    pub keys: Vec<String>,
    /// The stage is shown (else only the heading and Show preview).
    pub shown: bool,
    /// Hide / Show preview; `None` = no clip yet: no button, no stage.
    pub toggle: Option<Message>,
    /// The stage's button (its label; `None` message = greyed, e.g. while
    /// downloading); `None` = no button.
    pub play: Option<(&'a str, Option<Message>)>,
    /// One calm line under the stage (a download that failed).
    pub note: Option<&'a str>,
    /// The frame now (`None`: not decoded yet, the stage stays empty).
    pub frame: Option<Handle>,
    /// Where the stage tells its width in px (the frames are made that size),
    /// the window's scale factor and its height (logical px; sets the stage's
    /// smart size, `stage_width`).
    pub width: (Arc<AtomicU32>, f32, f32),
    /// Several clips: (the one showing, how many, previous, next); the stage
    /// gets arrows and dots. `None` = one clip: nothing.
    pub pager: Option<(usize, usize, Message, Message)>,
}

pub fn hero<'a, Message: Clone + 'a>(h: Hero<'a, Message>) -> Element<'a, Message> {
    let mut name = row![super::text::text(Role::PAGE_TITLE, h.title)]
        .spacing(space::GAP)
        .align_y(iced::Bottom);
    if h.lab {
        name = name.push(sub(Role::CARD_DESC, "Labs"));
    }
    let keys: Element<'a, Message> = match h.keys.is_empty() {
        true => sub(Role::CARD_DESC, "Shortcut not set").into(),
        false => row(h.keys.into_iter().map(|k| {
            let w = super::text::width(&k, Role::KEYCAP.size, super::text::font(Role::KEYCAP));
            container(keycap(k)).width(w + CAP_PAD).into()
        }))
        .spacing(space::TRACK_PAD)
        .into(),
    };
    let collapse = match h.shown {
        true => "Hide preview",
        false => "Show preview",
    };
    // one compact line, shown or hidden: the name and line left; Hide / Show
    // preview, then the keycaps, right
    let has_clip = h.toggle.is_some();
    let heading = row![
        column![name, sub(Role::CARD_DESC, h.desc)]
            .spacing(space::TRACK_GAP)
            .width(Fill),
    ]
    .push(
        h.toggle
            .map(|m| button(label(collapse), Kind::Elevated, Some(m))),
    )
    .push(keys)
    .spacing(space::GAP * 2.0)
    .align_y(Center);
    let mut body = column![heading].spacing(STAGE_PAD);
    if has_clip && h.shown {
        body = body.push(stage(h.frame, h.play, h.width, h.pager));
        if let Some(note) = h.note {
            body = body.push(sub(Role::CARD_DESC, note));
        }
    }
    container(body)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

fn stage<'a, Message: Clone + 'a>(
    frame: Option<Handle>,
    play: Option<(&'a str, Option<Message>)>,
    (width, scale, win_h): (Arc<AtomicU32>, f32, f32),
    pager: Option<(usize, usize, Message, Message)>,
) -> Element<'a, Message> {
    let chip = container(
        text("Preview")
            .size(SMALL)
            .font(super::text::font(Role::CARD_DESC))
            .color(c().accent),
    )
    .padding([2, 8])
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    let play = play.map(|(l, m)| button(label(l), Kind::Elevated, m));
    let top = row![chip, Space::new().width(Fill)]
        .push(play)
        .width(Fill)
        .align_y(Center);
    let media = Element::new(Media {
        frame,
        width,
        scale,
        win_h,
    });
    let mut inside = column![top, media]
        .spacing(space::TRACK_PAD)
        .align_x(Center);
    if let Some((at, n, prev, next)) = pager {
        inside = inside.push(pager_row(at, n, prev, next));
    }
    container(inside)
        .padding(STAGE_PAD)
        .width(Fill)
        .clip(true)
        .style(|_| container::Style {
            background: Some(c().track_glass.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// A dot of the position marker.
const DOT: f32 = 6.0;

/// The arrows with a dot per clip, the showing one in the accent.
fn pager_row<'a, Message: Clone + 'a>(
    at: usize,
    n: usize,
    prev: Message,
    next: Message,
) -> Element<'a, Message> {
    let arrow = |glyph: Icon, label: &'a str, m: Message| {
        let b = icon_button(glyph.glyph(), icon_button::State::default(), Some(m));
        hint(b, label, Why::Icon, Position::Top)
    };
    let dots = row((0..n).map(|i| {
        container(Space::new())
            .width(DOT)
            .height(DOT)
            .style(move |_| container::Style {
                background: Some(if i == at { c().accent } else { c().divider }.into()),
                border: Border::default().rounded(radius::FULL),
                ..container::Style::default()
            })
            .into()
    }))
    .spacing(space::GAP);
    row![
        arrow(Icon::Back, "Previous preview", prev),
        dots,
        arrow(Icon::Forward, "Next preview", next)
    ]
    .spacing(space::GAP * 2.0)
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

/// The frame: at the stage's smart size, 16:9 (it scales with the window); the
/// width it gets laid out at, in px, goes to `width` for the decoder.
struct Media {
    frame: Option<Handle>,
    width: Arc<AtomicU32>,
    scale: f32,
    win_h: f32,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Media {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let avail = limits.max().width;
        let avail = if avail.is_finite() { avail } else { 600.0 };
        let w = stage_width(avail, self.win_h, self.scale);
        let px = (w * self.scale).round().max(1.0) as u32;
        self.width.store(px, Ordering::Relaxed);
        layout::Node::new(Size::new(w, (w / ASPECT).round()))
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
            border_radius: radius::MENU.into(),
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
        // window size; the page is the window minus the sidebar and paddings
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
        // 150 %: the logical size is the same; never above native px
        assert_eq!(stage_width(1760.0, 1080.0, 1.5), 806.0);
        assert_eq!(stage_width(3000.0, 3000.0, 3.0), 640.0);
        // a page narrower than the minimum is never overflowed
        assert_eq!(stage_width(300.0, 600.0, 1.0), 300.0);
    }
}
