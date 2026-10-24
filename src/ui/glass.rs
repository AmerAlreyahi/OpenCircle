//! The glass effect of the `Frost` / `Graphite` widget themes: the frozen screen,
//! blurred once per capture (a 1/4-size copy through the editor's box blur), is
//! cropped under each widget once per position (`Backdrop::under`) and drawn behind
//! the widget's translucent tint. Widgets outside the overlay (popup windows have
//! no alpha) get no backdrop and draw their solid tint instead.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use iced::advanced::image::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, widget::image};

/// The blurred copy is this many times smaller than the screen.
const DOWN: usize = 4;
/// Box-blur radius at that size (x `DOWN` = about 20 px on screen), two passes.
const BLUR_RADIUS: usize = 5;
/// Most crops kept (a bar, a card, a few pills).
const MAX_CROPS: usize = 24;

pub struct Backdrop {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
    /// Device pixels per logical pixel of the frame.
    scale: f32,
    crops: RefCell<HashMap<[i32; 5], image::Handle>>,
}

impl Backdrop {
    /// `frame`: RGBA pixels, `scale` device pixels per logical pixel.
    fn blurred(width: usize, height: usize, rgba: &[u8], scale: f32) -> Option<Backdrop> {
        let (w, h) = (width / DOWN, height / DOWN);
        if w == 0 || h == 0 || rgba.len() < width * height * 4 {
            return None;
        }
        let mut buf = vec![[0.0f32; 3]; w * h];
        for (i, out) in buf.iter_mut().enumerate() {
            let (x0, y0) = ((i % w) * DOWN, (i / w) * DOWN);
            for y in y0..y0 + DOWN {
                for x in x0..x0 + DOWN {
                    let p = &rgba[(y * width + x) * 4..];
                    (0..3).for_each(|c| out[c] += f32::from(p[c]));
                }
            }
            out.iter_mut().for_each(|v| *v /= (DOWN * DOWN) as f32);
        }
        crate::editor::blur::blur2d(&mut buf, w, h, BLUR_RADIUS, 2);
        BLURS.with(|n| n.set(n.get() + 1));
        let px = buf.iter().map(|p| p.map(|v| v.round() as u8)).collect();
        Some(Backdrop {
            w,
            h,
            px,
            scale,
            crops: RefCell::default(),
        })
    }

    /// The blurred screen under `b` (logical pixels), cut to a rounded rectangle of
    /// corner radius `radius`. Cached per position.
    fn under(&self, b: Rectangle, radius: f32) -> image::Handle {
        let key = [
            b.x.round() as i32,
            b.y.round() as i32,
            b.width.round() as i32,
            b.height.round() as i32,
            radius.round() as i32,
        ];
        let mut crops = self.crops.borrow_mut();
        if crops.len() >= MAX_CROPS && !crops.contains_key(&key) {
            crops.clear();
        }
        crops
            .entry(key)
            .or_insert_with(|| self.crop(b, radius))
            .clone()
    }

    fn crop(&self, b: Rectangle, radius: f32) -> image::Handle {
        let s = self.scale;
        let (ow, oh) = (
            (b.width * s).ceil() as usize,
            (b.height * s).ceil() as usize,
        );
        let (hw, hh) = (ow as f32 / 2.0, oh as f32 / 2.0);
        let r = (radius * s).min(hw).min(hh);
        let at = |x: isize, y: isize| {
            self.px[y.clamp(0, self.h as isize - 1) as usize * self.w
                + x.clamp(0, self.w as isize - 1) as usize]
        };
        let mut out = Vec::with_capacity(ow * oh * 4);
        for y in 0..oh {
            for x in 0..ow {
                // Bilinear sample of the small copy at this device pixel.
                let fx = (b.x * s + x as f32 + 0.5) / DOWN as f32 - 0.5;
                let fy = (b.y * s + y as f32 + 0.5) / DOWN as f32 - 0.5;
                let (x0, y0) = (fx.floor(), fy.floor());
                let (tx, ty) = (fx - x0, fy - y0);
                let (x0, y0) = (x0 as isize, y0 as isize);
                let mix = |c: usize| {
                    let top =
                        f32::from(at(x0, y0)[c]) * (1.0 - tx) + f32::from(at(x0 + 1, y0)[c]) * tx;
                    let bot = f32::from(at(x0, y0 + 1)[c]) * (1.0 - tx)
                        + f32::from(at(x0 + 1, y0 + 1)[c]) * tx;
                    (top * (1.0 - ty) + bot * ty).round() as u8
                };
                // Rounded-rectangle coverage (signed distance, 1 px anti-aliasing).
                let (qx, qy) = (
                    (x as f32 + 0.5 - hw).abs() - (hw - r),
                    (y as f32 + 0.5 - hh).abs() - (hh - r),
                );
                let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let a = (0.5 - d).clamp(0.0, 1.0);
                out.extend_from_slice(&[mix(0), mix(1), mix(2), (a * 255.0).round() as u8]);
            }
        }
        image::Handle::from_rgba(ow as u32, oh as u32, out)
    }
}

thread_local! {
    /// The frozen frame of the open overlay and its scale; the blur is made on first use.
    static FRAME: RefCell<Option<(image::Handle, f32)>> = const { RefCell::new(None) };
    static BACKDROP: RefCell<Option<Rc<Backdrop>>> = const { RefCell::new(None) };
    /// The window being built is the overlay (the only one the frame fits).
    static IN_OVERLAY: Cell<bool> = const { Cell::new(false) };
    /// How many blurs were computed (tests).
    static BLURS: Cell<u32> = const { Cell::new(0) };
}

/// A new capture opened (`None`: it closed).
pub fn set_frame(frame: Option<&image::Handle>, scale: f32) {
    BACKDROP.with(|b| *b.borrow_mut() = None);
    FRAME.with(|f| *f.borrow_mut() = frame.map(|h| (h.clone(), scale)));
}

/// `view()` is about to build the overlay's widgets (`true`) or another window's.
pub fn building_overlay(yes: bool) {
    IN_OVERLAY.with(|c| c.set(yes));
}

/// The blurred screen for the widgets being built, if the theme wants glass and
/// they are in the overlay. The blur is computed once per capture.
pub fn backdrop() -> Option<Rc<Backdrop>> {
    let (theme, on) = crate::config::widgets();
    backdrop_for(theme, on)
}

fn backdrop_for(theme: crate::config::WidgetTheme, on: bool) -> Option<Rc<Backdrop>> {
    if theme == crate::config::WidgetTheme::Default || !on || !IN_OVERLAY.with(Cell::get) {
        return None;
    }
    if let Some(b) = BACKDROP.with(|b| b.borrow().clone()) {
        return Some(b);
    }
    let made = FRAME.with(|f| {
        let f = f.borrow();
        let (
            iced::advanced::image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            },
            scale,
        ) = f.as_ref()?
        else {
            return None;
        };
        Backdrop::blurred(*width as usize, *height as usize, pixels, *scale)
    })?;
    let made = Rc::new(made);
    BACKDROP.with(|b| *b.borrow_mut() = Some(made.clone()));
    Some(made)
}

/// `content` (a styled, translucent container) over the blurred screen, cut to `radius`.
pub fn frosted<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    back: Rc<Backdrop>,
    radius: f32,
) -> Element<'a, Message> {
    Element::new(Frost {
        content: content.into(),
        back,
        radius,
    })
}

/// Draws the blur under its content and hands everything else to it.
struct Frost<'a, Message> {
    content: Element<'a, Message>,
    back: Rc<Backdrop>,
    radius: f32,
}

type Theme = iced::Theme;
type Renderer = iced::Renderer;

impl<Message> Widget<Message, Theme, Renderer> for Frost<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(&mut self, tree: &mut Tree, r: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, r, limits)
    }
    fn operate(&mut self, tree: &mut Tree, l: Layout<'_>, r: &Renderer, op: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, l, r, op);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        r: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget_mut()
            .update(tree, event, l, cursor, r, clipboard, shell, viewport);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        r: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, l, cursor, viewport, r)
    }
    fn draw(
        &self,
        tree: &Tree,
        r: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let b = l.bounds();
        if b.intersection(viewport).is_none() || b.width < 1.0 || b.height < 1.0 {
            return;
        }
        // Images are drawn above a layer's quads, and a nested layer above its
        // parent: the blur goes in this layer, the tint and content in a layer on top.
        let crop = self.back.under(b, self.radius);
        r.draw_image(iced::advanced::image::Image::new(crop), b, b);
        iced::advanced::Renderer::with_layer(r, b, |r| {
            self.content
                .as_widget()
                .draw(tree, r, theme, style, l, cursor, viewport);
        });
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        l: Layout<'b>,
        r: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, l, r, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WidgetTheme;

    fn frame() -> image::Handle {
        // Left half white, right half black.
        let (w, h) = (64u32, 32u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < w / 2 {
                    [255; 4]
                } else {
                    [0, 0, 0, 255]
                }
            })
            .collect();
        image::Handle::from_rgba(w, h, rgba)
    }

    fn with_config(
        theme: WidgetTheme,
        glass: bool,
        f: impl FnOnce(&dyn Fn() -> Option<Rc<Backdrop>>),
    ) {
        building_overlay(true);
        set_frame(Some(&frame()), 1.0);
        BLURS.with(|n| n.set(0));
        f(&|| backdrop_for(theme, glass));
        set_frame(None, 1.0);
    }

    #[test]
    fn glass_off_or_default_theme_blurs_nothing() {
        with_config(WidgetTheme::Frost, false, |backdrop| {
            assert!(backdrop().is_none());
            assert_eq!(BLURS.with(Cell::get), 0);
        });
        with_config(WidgetTheme::Default, true, |backdrop| {
            assert!(backdrop().is_none());
            assert_eq!(BLURS.with(Cell::get), 0);
        });
    }

    #[test]
    fn the_blur_and_each_crop_are_computed_once() {
        with_config(WidgetTheme::Frost, true, |backdrop| {
            let a = backdrop().unwrap();
            let b = backdrop().unwrap();
            assert!(Rc::ptr_eq(&a, &b));
            assert_eq!(BLURS.with(Cell::get), 1, "once per capture, not per call");
            let r = Rectangle::new(iced::Point::new(10.0, 4.0), Size::new(20.0, 10.0));
            let (c1, c2) = (a.under(r, 4.0), a.under(r, 4.0));
            assert_eq!(c1.id(), c2.id(), "same position = the same crop");
            assert_eq!(a.crops.borrow().len(), 1);
            // Another capture is blurred again; another window gets nothing.
            set_frame(Some(&frame()), 1.0);
            assert!(!Rc::ptr_eq(&a, &backdrop().unwrap()));
            building_overlay(false);
            assert!(backdrop().is_none());
        });
    }

    #[test]
    fn the_crop_is_blurred_and_rounded() {
        with_config(WidgetTheme::Frost, true, |backdrop| {
            let a = backdrop().unwrap();
            // Over the white/black edge: grey in the middle, transparent corners.
            let r = Rectangle::new(iced::Point::new(16.0, 8.0), Size::new(32.0, 16.0));
            let iced::advanced::image::Handle::Rgba { pixels, width, .. } = a.under(r, 8.0) else {
                panic!()
            };
            let px = |x: usize, y: usize| &pixels[(y * width as usize + x) * 4..][..4];
            assert_eq!(px(0, 0)[3], 0, "corner is cut");
            assert_eq!(px(16, 8)[3], 255);
            assert!(px(16, 8)[0] > 20 && px(16, 8)[0] < 235, "{:?}", px(16, 8));
        });
    }
}
