//! The one UI kit: tokens, themes, looks and motion (components join it as
//! their first screen is ported). Depends only on `iced`, `std` and itself
//! (`arch_tests::kit_depends_only_on_tokens`).

pub mod looks;
pub mod motion;
pub mod themes;
pub mod tokens;

#[cfg(test)]
mod tests {
    use super::themes::{self, Colours, DAYLIGHT, Mode, SAPPHIRE, THEMES};
    use super::{looks, tokens};
    use iced::Color;

    /// WCAG contrast of `fg` (laid over `bg` by its alpha) against opaque `bg`.
    fn contrast(fg: Color, bg: Color) -> f32 {
        let lum = |c: Color| {
            let f = |v: f32| match v <= 0.04045 {
                true => v / 12.92,
                false => ((v + 0.055) / 1.055).powf(2.4),
            };
            0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
        };
        let mix = |a: f32, b: f32| a * fg.a + b * (1.0 - fg.a);
        let fg = Color::from_rgb(mix(fg.r, bg.r), mix(fg.g, bg.g), mix(fg.b, bg.b));
        let (a, b) = (lum(fg), lum(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn materials(c: &Colours) -> [(&str, Color); 9] {
        [
            ("bg", c.bg),
            ("sidebar", c.sidebar),
            ("widget", c.widget),
            ("dropdown", c.dropdown),
            ("track", c.track),
            ("elevated", c.elevated),
            ("elevated_hover", c.elevated_hover),
            ("card_selected", c.card_selected),
            ("keycap", c.keycap),
        ]
    }

    #[test]
    fn every_surface_is_opaque_in_every_theme() {
        for t in THEMES {
            for (name, c) in materials(&t.c) {
                assert_eq!(c.a, 1.0, "{}.{name}", t.id);
            }
        }
    }

    /// Text reads on every surface it sits on: 4.5:1 (WCAG AA), secondary 3:1.
    #[test]
    fn text_is_readable_on_every_surface_in_every_theme() {
        for t in THEMES {
            for (name, s) in materials(&t.c).into_iter().take(6) {
                let (main, sub) = (contrast(t.c.text, s), contrast(t.c.text_sub, s));
                assert!(main >= 4.5, "{}: text on {name} {main:.2}", t.id);
                assert!(sub >= 3.0, "{}: text_sub on {name} {sub:.2}", t.id);
            }
        }
    }

    #[test]
    fn ids_are_unique_and_light_themes_ring_their_cards() {
        for (i, t) in THEMES.iter().enumerate() {
            assert!(THEMES[..i].iter().all(|o| o.id != t.id), "{}", t.id);
            assert_eq!(t.light, t.c.card_ring.is_some(), "{}", t.id);
        }
        for (i, l) in looks::LOOKS.iter().enumerate() {
            assert!(looks::LOOKS[..i].iter().all(|o| o.id != l.id), "{}", l.id);
        }
        assert_eq!(THEMES.iter().filter(|t| t.light).count(), 3);
    }

    /// The setup's tokens (before the kit) are the kit's Sapphire and Daylight.
    #[test]
    fn sapphire_and_daylight_keep_the_setup_tokens() {
        let rgb = Color::from_rgb8;
        let rgba = Color::from_rgba8;
        let old = [
            (
                &SAPPHIRE.c,
                [
                    rgb(0x0c, 0x17, 0x2e),
                    rgb(0x07, 0x10, 0x21),
                    rgba(4, 10, 24, 0.46),
                    rgb(0x1c, 0x36, 0x64),
                    rgb(0x23, 0x42, 0x7a),
                    Color::TRANSPARENT,
                    rgba(155, 205, 255, 0.17),
                    rgba(155, 205, 255, 0.085),
                    rgba(155, 205, 255, 0.065),
                    rgb(0xf0, 0xf7, 0xff),
                    rgba(198, 222, 255, 0.66),
                    rgba(198, 222, 255, 0.42),
                    rgb(0x38, 0xbd, 0xf8),
                    rgba(56, 189, 248, 0.18),
                ],
            ),
            (
                &DAYLIGHT.c,
                [
                    rgb(0xf5, 0xf7, 0xfa),
                    rgb(0xe2, 0xe7, 0xf0),
                    rgba(15, 23, 42, 0.07),
                    Color::WHITE,
                    rgb(0xf8, 0xfa, 0xfc),
                    rgba(15, 23, 42, 0.08),
                    rgba(15, 23, 42, 0.11),
                    rgba(15, 23, 42, 0.075),
                    rgba(15, 23, 42, 0.045),
                    rgb(0x0f, 0x14, 0x1e),
                    rgba(15, 20, 30, 0.62),
                    rgba(15, 20, 30, 0.42),
                    rgb(0x25, 0x63, 0xeb),
                    rgba(37, 99, 235, 0.12),
                ],
            ),
        ];
        for (c, want) in old {
            let got = [
                c.bg,
                c.track,
                c.track_glass,
                c.elevated,
                c.elevated_hover,
                c.elevated_border,
                c.rim,
                c.divider,
                c.hover,
                c.text,
                c.text_sub,
                c.text_muted,
                c.accent,
                c.accent_soft,
            ];
            assert_eq!(got, want);
        }
        assert_eq!(tokens::fixed::ON_ACCENT, rgb(0x02, 0x06, 0x17));
    }

    #[test]
    fn resolve_picks_by_mode_and_falls_back() {
        let id = |m, os_dark| themes::resolve(m, "emerald", "sandstone", os_dark).id;
        assert_eq!(id(Mode::Dark, false), "emerald");
        assert_eq!(id(Mode::Light, true), "sandstone");
        assert_eq!(id(Mode::System, true), "emerald");
        assert_eq!(id(Mode::System, false), "sandstone");
        // unknown, or a pick of the other mode
        assert_eq!(themes::resolve(Mode::Dark, "nope", "", true).id, "sapphire");
        assert_eq!(
            themes::resolve(Mode::Light, "", "midnight", true).id,
            "daylight"
        );
    }
}
