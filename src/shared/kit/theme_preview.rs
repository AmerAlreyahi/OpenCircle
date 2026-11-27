//! Settings round 2 (Appearance, option 1): a theme drawn as a tiny window (window dots, sidebar
//! stubs, a panel with lines and an accent button) and the cards that hold it, plus the screen-border
//! look mini-preview cards. `mode_card` = Light / Dark / System (System cuts diagonally from
//! top-right to bottom-left, light upper-left and dark lower-right); `theme_mini_card` = one colour
//! theme of a mode; `look_mini_card` = one screen-border glow preview in a 5-column grid.
//! Both mini-windows and look previews draw true rounded contours (`radius::CONTROL`) so corners
//! stay soft on the CPU renderer.
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{Space, button, column, container};
use iced::{Border, Color, Element, Fill, Padding, Point, Rectangle, Size};

use super::icon::glyph as icon_glyph;
use super::text::{sub, text};
use super::themes::{Theme, c, on};
use super::tokens::{Role, radius, settings};

const MODE_PAD: f32 = 10.0;

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn with_alpha(col: Color, a: f32) -> Color {
    Color { a, ..col }
}

/// Which half of the diagonal `(w, 0) -> (0, h)` to keep when drawing a mini-window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Half {
    Full,
    UpperLeft,
    LowerRight,
}

/// Samples a rounded rectangle into a smooth convex polygon (8 steps per corner arc).
fn rounded_rect_poly(r: Rectangle, rad: f32) -> Vec<Point> {
    let rad = rad.min(r.width * 0.5).min(r.height * 0.5);
    if rad <= 0.0 {
        return vec![
            Point::new(r.x, r.y),
            Point::new(r.x + r.width, r.y),
            Point::new(r.x + r.width, r.y + r.height),
            Point::new(r.x, r.y + r.height),
        ];
    }
    let steps = 8;
    let mut pts = Vec::with_capacity(4 * (steps + 1));
    let corners = [
        (
            r.x + r.width - rad,
            r.y + rad,
            -std::f32::consts::FRAC_PI_2,
            0.0,
        ),
        (
            r.x + r.width - rad,
            r.y + r.height - rad,
            0.0,
            std::f32::consts::FRAC_PI_2,
        ),
        (
            r.x + rad,
            r.y + r.height - rad,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        ),
        (
            r.x + rad,
            r.y + rad,
            std::f32::consts::PI,
            1.5 * std::f32::consts::PI,
        ),
    ];
    for (cx, cy, a0, a1) in corners {
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * (i as f32 / steps as f32);
            pts.push(Point::new(cx + rad * a.cos(), cy + rad * a.sin()));
        }
    }
    pts
}

/// Clips a convex polygon against the diagonal half-plane `x/w + y/h <= 1` (`UpperLeft`)
/// or `x/w + y/h >= 1` (`LowerRight`).
fn clip_poly(pts: &[Point], w: f32, h: f32, half: Half) -> Vec<Point> {
    if half == Half::Full || w <= 0.0 || h <= 0.0 {
        return pts.to_vec();
    }
    let val = |p: Point| {
        let d = p.x / w + p.y / h - 1.0;
        match half {
            Half::UpperLeft => -d,
            Half::LowerRight => d,
            Half::Full => 1.0,
        }
    };
    let mut out = Vec::with_capacity(pts.len() + 2);
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let va = val(a);
        let vb = val(b);
        if va >= 0.0 {
            out.push(a);
        }
        if (va >= 0.0) != (vb >= 0.0) {
            let t = va / (va - vb);
            out.push(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
    }
    out
}

fn fill_clipped(frame: &mut Frame, r: Rectangle, w: f32, h: f32, half: Half, col: Color, rad: f32) {
    if half == Half::Full {
        if rad > 0.0 {
            frame.fill(
                &Path::rounded_rectangle(r.position(), r.size(), rad.into()),
                col,
            );
        } else {
            frame.fill_rectangle(r.position(), r.size(), col);
        }
        return;
    }
    let poly = clip_poly(&rounded_rect_poly(r, rad), w, h, half);
    if poly.len() < 3 {
        return;
    }
    let path = Path::new(|b| {
        b.move_to(poly[0]);
        for &p in &poly[1..] {
            b.line_to(p);
        }
        b.close();
    });
    frame.fill(&path, col);
}

fn draw_theme_window(frame: &mut Frame, size: Size, theme: &'static Theme, half: Half) {
    let w = size.width;
    let h = size.height;
    let pane = if theme.light {
        mix(theme.c.bg, Color::from_rgb8(0xb8, 0xc4, 0xd6), 0.22)
    } else {
        mix(theme.c.bg, Color::BLACK, 0.38)
    };
    let card = if theme.light {
        theme.c.widget
    } else {
        mix(theme.c.bg, theme.c.elevated, 0.26)
    };
    let line = if theme.light {
        Color::from_rgb8(0x64, 0x74, 0x8b)
    } else {
        Color::from_rgb8(0x9f, 0xb4, 0xd8)
    };

    // outer pane (soft 9 px corners, clipped cleanly on both halves of System split)
    fill_clipped(
        frame,
        Rectangle::new(Point::ORIGIN, size),
        w,
        h,
        half,
        pane,
        radius::CONTROL,
    );

    // top-left window dots (.mt: left 6%, top 6%, 4px dots, 3px gap)
    let dot = 4.0_f32.min(h * 0.055).max(2.5);
    let dot_gap = 3.0_f32.min(w * 0.025).max(2.0);
    for i in 0..3 {
        let r = Rectangle::new(
            Point::new(w * 0.06 + i as f32 * (dot + dot_gap), h * 0.06),
            Size::new(dot, dot),
        );
        fill_clipped(frame, r, w, h, half, with_alpha(line, 0.55), dot * 0.5);
    }

    // sidebar stubs (.mside: left 6%, top 22%, width 20%, 4 bars)
    let sb_h = (h * 0.045).clamp(3.0, 4.5);
    let sb_gap = (h * 0.065).clamp(4.0, 6.5);
    let sb_w = w * 0.20;
    for i in 0..4 {
        let col = if i == 0 {
            theme.c.accent
        } else {
            with_alpha(line, 0.30)
        };
        let r = Rectangle::new(
            Point::new(w * 0.06, h * 0.22 + i as f32 * (sb_h + sb_gap)),
            Size::new(sb_w, sb_h),
        );
        fill_clipped(frame, r, w, h, half, col, sb_h * 0.5);
    }

    // content card (.mcard: left 30%, top 12%, right 4%, bottom 8%)
    let cx = w * 0.30;
    let cy = h * 0.12;
    let cw = w * 0.66;
    let ch = h * 0.80;
    fill_clipped(
        frame,
        Rectangle::new(Point::new(cx, cy), Size::new(cw, ch)),
        w,
        h,
        half,
        card,
        6.0,
    );

    // inside .mcard (padding 8%)
    let ix = cx + cw * 0.08;
    let iw = cw * 0.84;
    let iy = cy + ch * 0.10;
    let lh = (h * 0.045).clamp(3.0, 4.5);
    let lg = (ch * 0.10).clamp(4.0, 9.0);
    // heading bar u.h (46%)
    fill_clipped(
        frame,
        Rectangle::new(Point::new(ix, iy), Size::new(iw * 0.46, lh + 1.0)),
        w,
        h,
        half,
        with_alpha(line, 0.80),
        2.0,
    );
    // full line bar u (100%)
    fill_clipped(
        frame,
        Rectangle::new(Point::new(ix, iy + lh + 1.0 + lg), Size::new(iw, lh)),
        w,
        h,
        half,
        with_alpha(line, 0.45),
        2.0,
    );
    // short line bar u.s (62%)
    fill_clipped(
        frame,
        Rectangle::new(
            Point::new(ix, iy + (lh + lg) * 2.0 + 1.0),
            Size::new(iw * 0.62, lh),
        ),
        w,
        h,
        half,
        with_alpha(line, 0.45),
        2.0,
    );
    // accent button .mbt at bottom (34% wide, 7px tall)
    let bh = (h * 0.08).clamp(5.0, 7.5);
    fill_clipped(
        frame,
        Rectangle::new(
            Point::new(ix, cy + ch - ch * 0.10 - bh),
            Size::new(iw * 0.34, bh),
        ),
        w,
        h,
        half,
        theme.c.accent,
        bh * 0.5,
    );
}

struct MiniWindow {
    primary: &'static Theme,
    split: Option<&'static Theme>,
}

impl<Message> canvas::Program<Message> for MiniWindow {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        match self.split {
            Some(dark) => {
                draw_theme_window(&mut frame, bounds.size(), self.primary, Half::UpperLeft);
                draw_theme_window(&mut frame, bounds.size(), dark, Half::LowerRight);
            }
            None => {
                draw_theme_window(&mut frame, bounds.size(), self.primary, Half::Full);
            }
        }
        let border = Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(
                (bounds.width - 1.0).max(0.0),
                (bounds.height - 1.0).max(0.0),
            ),
            (radius::CONTROL - 0.5).max(0.0).into(),
        );
        frame.stroke(
            &border,
            Stroke::default()
                .with_color(Color::from_rgba8(100, 116, 139, 0.28))
                .with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}

/// A tiny window: single theme when `side.id == panel.id`, or diagonally split (light upper-left,
/// dark lower-right) for System.
pub fn mini_window<'a, Message: 'a>(
    side: &'static Theme,
    panel: &'static Theme,
    h: f32,
) -> Element<'a, Message> {
    let split = (side.id != panel.id).then_some(panel);
    iced::widget::canvas(MiniWindow {
        primary: side,
        split,
    })
    .width(Fill)
    .height(h)
    .into()
}

struct LookMini {
    a: Color,
    b: Color,
}

impl<Message> canvas::Program<Message> for LookMini {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        // dark desktop backdrop (.mw.lk, soft 9 px corners)
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), radius::CONTROL.into()),
            Color::from_rgb8(0x0d, 0x15, 0x26),
        );
        // soft inset glow (concentric rings fading inward in `b`)
        let steps = 6;
        for i in 0..steps {
            let inset = 2.0 + i as f32 * 2.2;
            let alpha = 0.42 * (1.0 - i as f32 / steps as f32);
            let rw = (w - 2.0 * inset).max(0.0);
            let rh = (h - 2.0 * inset).max(0.0);
            if rw > 1.0 && rh > 1.0 {
                let p = Path::rounded_rectangle(
                    Point::new(inset, inset),
                    Size::new(rw, rh),
                    (radius::CONTROL - inset * 0.4).max(2.0).into(),
                );
                frame.stroke(
                    &p,
                    Stroke::default()
                        .with_color(with_alpha(self.b, alpha))
                        .with_width(2.5),
                );
            }
        }
        // crisp 2px inset border in `a`
        let border = Path::rounded_rectangle(
            Point::new(1.0, 1.0),
            Size::new((w - 2.0).max(0.0), (h - 2.0).max(0.0)),
            (radius::CONTROL - 1.0).into(),
        );
        frame.stroke(
            &border,
            Stroke::default().with_color(self.a).with_width(2.0),
        );

        // inner mini light window (.lkw: 18% left/right, 20% top/bottom)
        let wx = w * 0.18;
        let wy = h * 0.20;
        let ww = w * 0.64;
        let wh = h * 0.60;
        frame.fill(
            &Path::rounded_rectangle(Point::new(wx, wy), Size::new(ww, wh), 5.0.into()),
            Color::from_rgb8(0xe8, 0xed, 0xf6),
        );
        // two grey bars inside .lkw
        let pad_x = ww * 0.10;
        let pad_y = wh * 0.16;
        let bar_h = (wh * 0.10).clamp(3.0, 4.5);
        let bar_col = Color::from_rgb8(0x9a, 0xa7, 0xbd);
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(wx + pad_x, wy + pad_y),
                Size::new(ww - 2.0 * pad_x, bar_h),
                2.0.into(),
            ),
            bar_col,
        );
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(wx + pad_x, wy + pad_y + bar_h + wh * 0.12),
                Size::new((ww - 2.0 * pad_x) * 0.55, bar_h),
                2.0.into(),
            ),
            bar_col,
        );
        vec![frame.into_geometry()]
    }
}

/// A screen-border look's mini desktop preview (`.mw.lk`).
pub fn look_mini_window<'a, Message: 'a>(a: Color, b: Color, h: f32) -> Element<'a, Message> {
    iced::widget::canvas(LookMini { a, b })
        .width(Fill)
        .height(h)
        .into()
}

fn ring<'a, Message: 'a>(chosen: bool) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 11.0).color(on(c().accent)).into(),
        false => Space::new().into(),
    };
    container(inner)
        .center(20.0)
        .style(move |_| container::Style {
            background: chosen.then(|| c().accent.into()),
            border: Border::default()
                .rounded(20.0)
                .width(if chosen { 0.0 } else { 1.5 })
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

fn frame<'a, Message: Clone + 'a>(
    body: Element<'a, Message>,
    pad: Padding,
    quiet_idle: bool,
    chosen: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    super::focus::press(
        button(body)
            .width(Fill)
            .padding(pad)
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: match (chosen, hot, quiet_idle) {
                        (true, _, _) => Some(c().accent_soft.into()),
                        (false, true, _) => Some(c().hover.into()),
                        (false, false, true) => None,
                        (false, false, false) => Some(c().card_idle.into()),
                    },
                    border: Border::default()
                        .rounded(radius::CARD)
                        .width(if chosen { 1.5 } else { 1.0 })
                        .color(if chosen {
                            c().accent
                        } else if quiet_idle {
                            Color::TRANSPARENT
                        } else {
                            c().divider
                        }),
                    ..button::Style::default()
                }
            }),
        radius::CARD,
        on_press,
    )
    .into()
}

/// A mode card: its tiny window (`side` / `panel` themes), then the name and its line, the ring at the end.
pub fn mode_card<'a, Message: Clone + 'a>(
    (side, panel): (&'static Theme, &'static Theme),
    name: &'a str,
    desc: &'a str,
    chosen: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let words = container(
        super::dir::row([
            text(Role::ROW_TITLE, name).into(),
            container(sub(Role::ROW_DESC, desc)).width(Fill).into(),
            ring(chosen),
        ])
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding(Padding::from([0.0, 4.0]).bottom(2.0));
    frame(
        column![mini_window(side, panel, settings::MINI_MODE_H), words]
            .spacing(10)
            .into(),
        Padding::new(MODE_PAD),
        false,
        chosen,
        Some(on_press),
    )
}

/// A colour theme's card: its tiny window and its name, a check when chosen.
pub fn theme_mini_card<'a, Message: Clone + 'a>(
    theme: &'static Theme,
    name: &'a str,
    chosen: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let mark: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 13.0).color(c().accent).into(),
        false => Space::new().into(),
    };
    let words = container(
        super::dir::row([
            container(text(Role::CARD_TITLE, name)).width(Fill).into(),
            mark,
        ])
        .spacing(4)
        .align_y(iced::Center),
    )
    .padding([0.0, 3.0]);
    frame(
        column![mini_window(theme, theme, settings::MINI_THEME_H), words]
            .spacing(settings::MINI_CARD_GAP)
            .into(),
        Padding::new(settings::MINI_CARD_PAD).bottom(settings::MINI_CARD_PAD_BOTTOM),
        true,
        chosen,
        Some(on_press),
    )
}

/// A screen-border look's card: its mini desktop glow preview and its name, a check when chosen.
pub fn look_mini_card<'a, Message: Clone + 'a>(
    colours: &[Color],
    name: &'a str,
    chosen: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let a = colours.first().copied().unwrap_or(c().accent);
    let b = colours.last().copied().unwrap_or(a);
    let mark: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 13.0).color(c().accent).into(),
        false => Space::new().into(),
    };
    let words = container(
        super::dir::row([
            container(text(Role::CARD_TITLE, name)).width(Fill).into(),
            mark,
        ])
        .spacing(4)
        .align_y(iced::Center),
    )
    .padding([0.0, 3.0]);
    frame(
        column![look_mini_window(a, b, settings::MINI_THEME_H), words]
            .spacing(settings::MINI_CARD_GAP)
            .into(),
        Padding::new(settings::MINI_CARD_PAD).bottom(settings::MINI_CARD_PAD_BOTTOM),
        true,
        chosen,
        on_press,
    )
}
