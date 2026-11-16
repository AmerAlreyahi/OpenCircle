//! Widgets drawn off screen: an `Element` painted on a picture (Snip's frame
//! as the user sees it: the bar and the AI chat drawn into it), and the tests'
//! renders (the tests' `testing::shot` renders the same way). The CPU renderer the windows use, so the pixels
//! are the window's.
use crate::shared::kit;
use iced::advanced::{graphics::Viewport, mouse, renderer};
use iced::{Color, Element, Rectangle, Size, Theme, widget::image};
use iced_runtime::user_interface::{Cache, UserInterface};

/// `view` (laid out in `width` x `height` physical px over `scale`) drawn on
/// `background`: RGBA, premultiplied by its alpha (a transparent background
/// leaves what the view doesn't cover clear).
pub fn paint<Message>(
    view: Element<'_, Message>,
    renderer: &mut iced::Renderer,
    (width, height): (u32, u32),
    scale: f32,
    background: Color,
) -> Vec<u8> {
    let logical = Size::new(width as f32 / scale, height as f32 / scale);
    let mut ui = UserInterface::build(view, logical, Cache::default(), renderer);
    let style = renderer::Style {
        text_color: Color::WHITE,
    };
    ui.draw(renderer, &Theme::Dark, &style, mouse::Cursor::Unavailable);
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    let mut mask = tiny_skia::Mask::new(width, height).unwrap();
    let viewport = Viewport::with_physical_size(Size::new(width, height), scale);
    let all = Rectangle::with_size(Size::new(width as f32, height as f32));
    renderer.draw(
        &mut pixmap.as_mut(),
        &mut mask,
        &viewport,
        &[all],
        background,
    );
    // iced draws B, G, R, A.
    (pixmap.data().as_chunks::<4>().0.iter())
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect()
}

/// `frame` with `layers` (laid out at its logical size) drawn over it.
/// `None` if it isn't an RGBA picture.
pub fn over<Message>(
    frame: &image::Handle,
    scale: f32,
    layers: Element<'_, Message>,
) -> Option<image::Handle> {
    let image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = frame
    else {
        return None;
    };
    kit::text::load();
    let mut renderer = iced::Renderer::new(kit::text::ui_font(), iced::Pixels(16.0));
    let top = paint(
        layers,
        &mut renderer,
        (*width, *height),
        scale,
        Color::TRANSPARENT,
    );
    let mut out = pixels.to_vec();
    for (under, top) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(top.as_chunks::<4>().0)
    {
        let a = top[3] as u32;
        if a == 0 {
            continue;
        }
        // `top` is premultiplied: over an opaque frame, top + under * (1 - a)
        for c in 0..3 {
            under[c] = (top[c] as u32 + (under[c] as u32 * (255 - a) + 127) / 255).min(255) as u8;
        }
    }
    Some(image::Handle::from_rgba(*width, *height, out))
}
