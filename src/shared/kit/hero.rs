//! A feature's preview at the top of its Settings page (Settings round 2 `.r2 .sec.exp`,
//! `r2.py` `drawer` / `sticky`) and its Variant A maximised panel (`design-v060-preview-max`).
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use iced::advanced::image::{FilterMethod, Image, Renderer as _};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::Tree;
use iced::advanced::{Widget, mouse, renderer};
use iced::widget::image::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{Id, Space, column, container, mouse_area, row, stack};
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

/// Container id for the maximised preview panel (layout tests check its 14 px margin).
pub const MAX_PANEL_ID: &str = "preview-max-panel";

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
    /// Optional "Bigger" maximise button on the clip's top-end corner (no shortcut).
    pub bigger: Option<Message>,
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
    pub bigger: &'a str,
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
            bigger: "Bigger",
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
        let media = media_box(h.frame, h.width, None, h.bigger, w.bigger);
        column![media, info].spacing(12).width(Fill).into()
    } else {
        let media = media_box(
            h.frame,
            h.width,
            Some((settings::DEMO_W, settings::DEMO_H)),
            h.bigger,
            w.bigger,
        );
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

fn media_box<'a, Message: Clone + 'a>(
    frame: Option<Handle>,
    (width, scale, win_h): (Arc<AtomicU32>, f32, f32),
    fixed: Option<(f32, f32)>,
    bigger: Option<Message>,
    bigger_tip: &'a str,
) -> Element<'a, Message> {
    let media: Element<'a, Message> = Element::new(Media {
        frame,
        width,
        scale,
        win_h,
        fixed,
        radius: settings::DEMO_MEDIA_RADIUS,
    });
    let box_ = container(media).style(|_| container::Style {
        background: Some(c().track_glass.into()),
        border: Border::default()
            .rounded(settings::DEMO_MEDIA_RADIUS)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    });
    let base: Element<'a, Message> = match fixed {
        Some((w, h)) => box_.width(w).height(h).into(),
        None => box_.width(Fill).into(),
    };
    let Some(on_bigger) = bigger else {
        return base;
    };
    // Rounded "Bigger" button on the clip's top-end corner (top-start in Arabic RTL), no shortcut
    let btn =
        super::focus::press(
            iced::widget::button(
                container(glyph(Icon::Maximise.glyph(), size::ICON).color(Color::WHITE))
                    .center(Fill),
            )
            .width(settings::PREVIEW_BIGGER_BTN)
            .height(settings::PREVIEW_BIGGER_BTN)
            .padding(0)
            .style(|_, status| {
                let hot = matches!(
                    status,
                    iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
                );
                iced::widget::button::Style {
                    background: Some(
                        Color::from_rgba8(8, 14, 30, if hot { 0.72 } else { 0.46 }).into(),
                    ),
                    border: Border::default().rounded(radius::TRACK).width(1).color(
                        Color::from_rgba8(255, 255, 255, if hot { 0.38 } else { 0.20 }),
                    ),
                    ..iced::widget::button::Style::default()
                }
            }),
            radius::TRACK,
            Some(on_bigger),
        );
    let btn = hint(btn, bigger_tip, Why::Icon, Position::Bottom);
    let overlay = container(super::dir::row([Space::new().width(Fill).into(), btn]))
        .padding(settings::PREVIEW_BIGGER_INSET)
        .width(Fill);
    stack![base, overlay].into()
}

/// Variant A maximised preview panel (over the content card only, 14 px margin, 22 px corners).
pub struct Maximised<'a, Message> {
    pub eyebrow: &'a str,
    pub title: &'a str,
    pub desc: &'a str,
    pub frame: Option<Handle>,
    pub width: (Arc<AtomicU32>, f32, f32),
    pub playing: bool,
    pub reduced_motion: bool,
    pub rm_label: &'a str,
    pub on_play_pause: Option<Message>,
    pub on_close: Message,
    pub pager: Option<(usize, usize, Message, Message, String)>,
    pub prev_tip: &'a str,
    pub next_tip: &'a str,
}

/// Renders Variant A: soft dim over the content card + centred rounded panel (`14 px` margin,
/// `22 px` radius) with the 16:9 clip filling the panel and title + controls overlaid.
pub fn maximised<'a, Message: Clone + 'a>(m: Maximised<'a, Message>) -> Element<'a, Message> {
    let dim = mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| {
        container::Style {
            background: Some(
                if current().light {
                    Color::from_rgba8(15, 23, 42, 0.20)
                } else {
                    Color::from_rgba8(3, 8, 20, 0.46)
                }
                .into(),
            ),
            ..container::Style::default()
        }
    }))
    .on_press(m.on_close.clone()); // mouse only: a press on the dim closes the panel (Esc is the host's)

    let media: Element<'a, Message> = Element::new(Media {
        frame: m.frame,
        width: m.width.0,
        scale: m.width.1,
        win_h: m.width.2,
        fixed: None,
        radius: settings::PREVIEW_MAX_RADIUS,
    });
    let video_layer = container(media).width(Fill).height(Fill).center(Fill);

    let top_text = column![
        iced::widget::text(if super::dir::rtl() {
            m.eyebrow.to_string()
        } else {
            m.eyebrow.to_uppercase()
        })
        .size(super::text::size(Role::SECTION))
        .font(super::text::font(Role::SECTION))
        .color(Color::from_rgb8(0x7d, 0xd3, 0xfc)),
        iced::widget::text(m.title)
            .size(super::text::size(Role::PREVIEW_MAX_TITLE))
            .font(super::text::font(Role::PREVIEW_MAX_TITLE))
            .color(Color::WHITE),
        iced::widget::text(m.desc)
            .size(super::text::size(Role::ROW_TITLE))
            .font(super::text::font(Role::ROW_DESC))
            .color(Color::from_rgba8(255, 255, 255, 0.78)),
    ]
    .spacing(3)
    .width(Fill)
    .align_x(super::dir::start());

    let esc_chip = container(
        iced::widget::text("Esc")
            .size(10.5)
            .font(super::text::MONO)
            .color(Color::from_rgba8(255, 255, 255, 0.82)),
    )
    .padding([3, 7])
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(255, 255, 255, 0.12).into()),
        border: Border::default()
            .rounded(radius::KEYCAP)
            .width(1)
            .color(Color::from_rgba8(255, 255, 255, 0.20)),
        ..container::Style::default()
    });
    let close_btn = super::focus::press(
        iced::widget::button(
            container(glyph(Icon::Close.glyph(), 16.0).color(Color::WHITE)).center(Fill),
        )
        .width(settings::PREVIEW_MAX_CLOSE)
        .height(settings::PREVIEW_MAX_CLOSE)
        .padding(0)
        .style(|_, status| {
            let hot = matches!(
                status,
                iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
            );
            iced::widget::button::Style {
                background: Some(
                    Color::from_rgba8(8, 14, 30, if hot { 0.78 } else { 0.50 }).into(),
                ),
                border: Border::default()
                    .rounded(11.0)
                    .width(1)
                    .color(Color::from_rgba8(
                        255,
                        255,
                        255,
                        if hot { 0.42 } else { 0.22 },
                    )),
                ..iced::widget::button::Style::default()
            }
        }),
        11.0,
        Some(m.on_close),
    );
    let top_bar = container(
        super::dir::row([
            top_text.into(),
            super::dir::row([esc_chip.into(), close_btn.into()])
                .spacing(8)
                .align_y(Center)
                .into(),
        ])
        .spacing(16)
        .align_y(iced::Alignment::Start),
    )
    .width(Fill)
    .padding(iced::Padding::from([20.0, 24.0]))
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(6, 10, 24, 0.62).into()),
        border: Border::default().rounded(
            iced::border::Radius::default()
                .top_left(settings::PREVIEW_MAX_RADIUS)
                .top_right(settings::PREVIEW_MAX_RADIUS),
        ),
        ..container::Style::default()
    });

    let play_glyph = if m.playing { Icon::Stop } else { Icon::Forward };
    let pp_btn = super::focus::press(
        iced::widget::button(
            container(glyph(play_glyph.glyph(), 18.0).color(Color::from_rgb8(0x0b, 0x12, 0x26)))
                .center(Fill),
        )
        .width(settings::PREVIEW_MAX_PLAY)
        .height(settings::PREVIEW_MAX_PLAY)
        .padding(0)
        .style(|_, _| iced::widget::button::Style {
            background: Some(Color::WHITE.into()),
            border: Border::default().rounded(radius::FULL),
            ..iced::widget::button::Style::default()
        }),
        radius::FULL,
        m.on_play_pause.clone(),
    );
    let track = container(progress::bar(if m.playing { 0.62 } else { 1.0 })).width(Fill);
    let mut ctl_items: Vec<Element<'a, Message>> = vec![pp_btn.into(), track.into()];
    if let Some((_, _, prev, next, of_text)) = m.pager {
        let nav_btn = |ic: Icon, tip_text: &'a str, msg: Message| {
            let b = super::focus::press(
                iced::widget::button(
                    container(glyph(ic.directed().glyph(), 14.0).color(Color::WHITE)).center(Fill),
                )
                .width(30.0)
                .height(30.0)
                .padding(0)
                .style(|_, _| iced::widget::button::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.14).into()),
                    border: Border::default().rounded(radius::CONTROL),
                    ..iced::widget::button::Style::default()
                }),
                radius::CONTROL,
                Some(msg),
            );
            hint(b, tip_text, Why::Icon, Position::Top)
        };
        ctl_items.push(
            super::dir::row([
                nav_btn(Icon::Back, m.prev_tip, prev),
                iced::widget::text(of_text)
                    .size(12.0)
                    .font(super::text::MONO)
                    .color(Color::from_rgba8(255, 255, 255, 0.86))
                    .into(),
                nav_btn(Icon::Forward, m.next_tip, next),
            ])
            .spacing(8)
            .align_y(Center)
            .into(),
        );
    }
    let bottom_bar = container(super::dir::row(ctl_items).spacing(14).align_y(Center))
        .width(Fill)
        .padding(iced::Padding::from([16.0, 26.0]))
        .style(|_| container::Style {
            background: Some(Color::from_rgba8(6, 10, 24, 0.72).into()),
            border: Border::default().rounded(
                iced::border::Radius::default()
                    .bottom_left(settings::PREVIEW_MAX_RADIUS)
                    .bottom_right(settings::PREVIEW_MAX_RADIUS),
            ),
            ..container::Style::default()
        });

    let middle: Element<'a, Message> = if m.reduced_motion {
        let rm_chip = container(
            iced::widget::text(m.rm_label)
                .size(12.0)
                .font(super::text::font(Role::CONTROL))
                .color(Color::WHITE),
        )
        .padding([6, 14])
        .style(|_| container::Style {
            background: Some(Color::from_rgba8(8, 14, 30, 0.78).into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(Color::from_rgba8(255, 255, 255, 0.22)),
            ..container::Style::default()
        });
        container(rm_chip).center(Fill).into()
    } else {
        Space::new().height(Fill).into()
    };

    let chrome = column![top_bar, middle, bottom_bar]
        .width(Fill)
        .height(Fill);

    let panel = container(stack![video_layer, chrome])
        .id(Id::new(MAX_PANEL_ID))
        .width(Fill)
        .height(settings::PREVIEW_MAX_H)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(0x0b, 0x12, 0x26).into()),
            border: Border::default()
                .rounded(settings::PREVIEW_MAX_RADIUS)
                .width(1)
                .color(Color::from_rgba8(255, 255, 255, 0.12)),
            ..container::Style::default()
        });

    let framed = container(panel)
        .padding(settings::PREVIEW_MAX_MARGIN)
        .width(Fill)
        .height(Fill)
        .center_y(Fill);

    stack![dim, framed].into()
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

const MAX_W: f32 = 820.0;
const HEIGHT_SHARE: f32 = 0.42;
const MIN_W: f32 = 353.0;
const NATIVE: f32 = 1920.0;

pub fn stage_width(avail: f32, win_h: f32, scale: f32) -> f32 {
    let w = avail
        .min(MAX_W)
        .min(HEIGHT_SHARE * win_h * ASPECT)
        .max(MIN_W)
        .min(NATIVE / scale.max(0.1));
    w.min(avail).round()
}

struct Media {
    frame: Option<Handle>,
    width: Arc<AtomicU32>,
    scale: f32,
    win_h: f32,
    fixed: Option<(f32, f32)>,
    radius: f32,
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
                let w = if self.radius >= settings::PREVIEW_MAX_RADIUS {
                    avail
                } else {
                    stage_width(avail, self.win_h, self.scale)
                };
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
        let rounded = super::thumb::round_handle(frame, bounds.size(), self.radius, false);
        let image = Image {
            filter_method: FilterMethod::Linear,
            border_radius: self.radius.into(),
            snap: true,
            ..Image::new(rounded)
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
