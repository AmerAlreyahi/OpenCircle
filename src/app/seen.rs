//! Snip's picture as the user sees it (owner, 2026-11-16): the overlay's own bar
//! and its open AI chat drawn into the frozen frame, the way "Snip the
//! translated page" puts the translation in (`Session::seen`). Drawn by the same
//! views the windows use (`bottom_bar`, the chat panel), off screen
//! (`app::offscreen`): no new OS capture, so nothing asks again on Wayland.
//! The edge glow, the selection brackets and Snip's own parts stay out.

use iced::widget::{image, pin, stack};
use iced::{Element, Point, Rectangle, Size, Vector};

use super::message::{Message, Purpose};
use super::{App, SLIDE};
use crate::features::circle::screens::overlay::View;
use crate::features::snip::screens::pick::{OWN, Pick, Place};

/// What one screen's window draws of ours: its logical rects.
type Parts = Vec<Rectangle>;

impl App {
    /// Draws the bar and the open AI chat into every screen's frame, and lists
    /// where they are as picks (physical desktop px, like the windows').
    /// Only in main mode: nothing else of ours is on the overlay's frame.
    pub(crate) fn draw_own_ui(&mut self) -> Vec<Pick> {
        if !matches!(self.o.purpose, Purpose::Search) || self.o.edit.is_some() {
            return Vec::new();
        }
        let start = self.place();
        let mut found: Vec<(Place, Rectangle)> = Vec::new();
        let first = self.o.frame.as_ref().and_then(|frame| {
            self.paint_own(self.overlay, frame, start.scale, self.o.screen, None)
        });
        if let Some((frame, parts)) = first {
            self.o.frame = Some(frame);
            found.extend(parts.into_iter().map(|r| (start, r)));
        }
        let others: Vec<_> = (self.o.screens.iter().enumerate())
            .filter_map(|(i, s)| {
                let at = self.paint_own(
                    s.window,
                    &s.frame,
                    s.place.scale,
                    s.place.size(),
                    Some(s.view),
                )?;
                Some((i, s.place, at))
            })
            .collect();
        for (i, place, (frame, parts)) in others {
            self.o.screens[i].frame = frame;
            found.extend(parts.into_iter().map(|r| (place, r)));
        }
        found
            .into_iter()
            .map(|(place, r)| {
                let (x, y) = place.origin;
                let at = Point::new(x as f32 + r.x * place.scale, y as f32 + r.y * place.scale);
                Pick::window(
                    Rectangle::new(at, r.size() * place.scale),
                    "OpenCircle",
                    OWN,
                )
            })
            .collect()
    }

    /// `frame` (the screen drawn in `window`, `size` logical px at `scale`) with
    /// the bar and the chat that window draws, and where they are (its logical
    /// px). `view`: an other screen's map from the starting screen's px.
    fn paint_own(
        &self,
        window: iced::window::Id,
        frame: &image::Handle,
        scale: f32,
        size: Size,
        view: Option<View>,
    ) -> Option<(image::Handle, Parts)> {
        let to_screen = |r: Rectangle| view.map_or(r, |v| v.rect_to_screen(r));
        let mut layers: Vec<Element<'_, Message>> = Vec::new();
        let mut parts: Parts = Vec::new();
        if self.bottom_bar_shown() && self.bar_window() == window {
            // where the user sees it: still sliding into place as the overlay opens
            let rise = (1.0 - self.appeared()) * SLIDE;
            let at = to_screen(Rectangle::new(
                self.bar_at() + Vector::new(0.0, rise),
                self.dock_size(),
            ));
            layers.push(pin(self.bottom_bar()).position(at.position()).into());
            parts.push(at);
        }
        if let Some((id, panel)) = &self.o.ai_chat
            && self.chat_window() == window
        {
            let id = *id;
            let at = to_screen(self.chat_card_at());
            let card = panel.view(at, size, self.o.chat_hover);
            layers.push(card.map(move |msg| Message::AiChat(id, msg)));
            parts.push(at);
        }
        if layers.is_empty() {
            return None;
        }
        let drawn = super::offscreen::over(frame, scale, stack(layers).into())?;
        Some((drawn, parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::open_app;
    use crate::features::ask_ai::screens::ai_chat::AiChat;
    use crate::shared::kit::themes::{self, DAYLIGHT, SAPPHIRE};
    use crate::shared::picture::Crop;

    fn pixels(frame: &Option<image::Handle>) -> Vec<u8> {
        match frame {
            Some(image::Handle::Rgba { pixels, .. }) => pixels.to_vec(),
            _ => panic!("an rgba frame"),
        }
    }

    /// The pixels of `a` and `b` that differ inside (or outside) `rects`.
    fn differing(a: &[u8], b: &[u8], width: usize, rects: &[Rectangle], inside: bool) -> usize {
        (0..a.len() / 4)
            .filter(|&i| {
                let p = Point::new((i % width) as f32 + 0.5, (i / width) as f32 + 0.5);
                rects.iter().any(|r| r.contains(p)) == inside
                    && a[i * 4..i * 4 + 4] != b[i * 4..i * 4 + 4]
            })
            .count()
    }

    /// Snip pressed on the open overlay takes the picture as the user sees it:
    /// the bar and the chat in it (and as picks), the rest of the frame as it
    /// was; English and Arabic, both themes, 1x and 2x.
    #[test]
    fn the_picture_has_the_bar_and_the_chat_in_it() {
        for arabic in [false, true] {
            let _ar = arabic.then(crate::shared::kit::testing::arabic);
            for theme in [&SAPPHIRE, &DAYLIGHT] {
                for scale in [1.0, 2.0] {
                    themes::set(theme);
                    let (w, h) = (1280usize, 800u32);
                    let mut app = open_app(w as u32, h, scale);
                    let crop = Crop {
                        width: 8,
                        height: 8,
                        rgba: vec![200; 8 * 8 * 4],
                    };
                    app.o.ai_chat = Some((1, AiChat::open(crop, None, false).0));
                    let before = pixels(&app.o.frame);
                    let bar = Rectangle::new(app.bar_at(), app.dock_size()) * scale;
                    let card = app.chat_card_at() * scale;
                    let picks = app.draw_own_ui();
                    let after = pixels(&app.o.frame);
                    let name = format!("ar {arabic} {} {scale}x", theme.id);
                    assert!(
                        differing(&before, &after, w, &[bar], true) > 100,
                        "bar: {name}"
                    );
                    assert!(
                        differing(&before, &after, w, &[card], true) > 100,
                        "chat: {name}"
                    );
                    let strays = differing(&before, &after, w, &[bar, card], false);
                    assert_eq!(strays, 0, "the rest of the frame is as it was: {name}");
                    let rects: Vec<_> = picks.iter().map(|p| (p.label.as_str(), p.rect)).collect();
                    assert_eq!(
                        rects,
                        [("Window: OpenCircle", bar), ("Window: OpenCircle", card)],
                        "{name}"
                    );
                }
            }
        }
        themes::set(&SAPPHIRE);
    }

    /// The entry every Snip start goes through (`snip_here`) starts from the
    /// frame with the bar in it.
    #[test]
    fn snip_here_starts_from_the_composed_frame() {
        let mut app = open_app(1280, 800, 1.0);
        let before = pixels(&app.o.frame);
        let _ = app.snip_here(None);
        assert!(pixels(&app.o.frame) != before, "the bar is in the picture");
    }
}
