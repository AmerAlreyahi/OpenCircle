//! The update pills (the design's board "Update" u1 / u3 / u7 / u8; owner,
//! 2026-11-10), docked above the tray (`Windows::update_pill`): "OpenCircle
//! <v> is ready" with Later / Restart now (a shield when Windows will ask for
//! permission; "Get it" where the user installs it), "Updating OpenCircle…"
//! with the spinning logo across the restart, and "Updated to <v> · What's
//! new" once after a quiet install. Unlike the other floating widgets they
//! show in screenshots and recordings (owner, 2026-11-10).
use iced::widget::row;
use iced::{Center, Element, Size};

use crate::app_language::{S, fmt};
use crate::t;
use crate::ui::kit::button::{Kind as Button, button, label, labelled, labelled_width};
use crate::ui::kit::icon::{Icon, icon};
use crate::ui::kit::logo::logo;
use crate::ui::kit::spinner::spinner;
use crate::ui::kit::text::{font, width};
use crate::ui::kit::toast::{Tone, banner, banner_size, size_with_action, with_action};
use crate::ui::kit::tokens::{Role, radius, size, space};
use crate::update::How;

/// The corners the window is cut to (`platform::windows::floating`).
pub const RADIUS: f32 = radius::DROPDOWN;
/// Between Later and the main button.
const ACTION_GAP: f32 = space::GAP;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Downloaded and verified (or out, for a copy the user updates).
    Ready { version: String, how: How },
    /// Restarting into the new version.
    Updating,
    /// Installed at a quit or lock: once, at the next start.
    Updated { version: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Later,
    /// Restart now (or Get it).
    Main,
    WhatsNew,
}

/// A pill: what it says (its title made once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pill {
    pub kind: Kind,
    title: String,
}

impl Pill {
    pub fn new(kind: Kind) -> Pill {
        let title = match &kind {
            Kind::Ready { version, how } if *how == How::GetIt => fmt(S::up_out, &[version]),
            Kind::Ready { version, .. } => fmt(S::up_ready, &[version]),
            Kind::Updating => t!(up_updating).into(),
            Kind::Updated { version } => fmt(S::up_updated, &[version]),
        };
        Pill { kind, title }
    }

    fn line(&self) -> &'static str {
        match self.kind {
            Kind::Ready {
                how: How::Restart, ..
            } => t!(up_line_restart),
            Kind::Ready {
                how: How::Setup, ..
            } => t!(up_line_setup),
            Kind::Ready {
                how: How::GetIt, ..
            } => t!(up_line_getit),
            Kind::Updating => t!(up_line_back),
            Kind::Updated { .. } => "",
        }
    }

    /// The main button: its glyph and words.
    fn main(how: How) -> (Icon, &'static str) {
        match how {
            How::Restart => (Icon::Restart, t!(g_restart_now)),
            How::Setup => (Icon::Shield, t!(g_restart_now)),
            How::GetIt => (Icon::External, t!(g_get_it)),
        }
    }

    fn actions_width(&self) -> f32 {
        match self.kind {
            Kind::Ready { how, .. } => {
                let later = width(
                    t!(up_later),
                    crate::ui::kit::text::size(Role::CONTROL),
                    font(Role::CONTROL),
                );
                later + ACTION_GAP + labelled_width(Pill::main(how).1)
            }
            _ => 0.0,
        }
    }

    /// The window's size (logical px, whole).
    pub fn size(&self) -> Size {
        match self.kind {
            Kind::Updated { .. } => size_with_action(&self.title, t!(g_whats_new)),
            _ => banner_size(&self.title, self.line(), self.actions_width()),
        }
    }

    /// The logo spins only on the Updating pill.
    pub fn spins(&self) -> bool {
        self.kind == Kind::Updating
    }
}

/// The pill, corners of `radius`; `turn` = the logo's turn (`kit::spinner`),
/// `still` = reduced motion.
pub fn view(p: &Pill, radius: f32, turn: f32, still: bool) -> Element<'_, Msg> {
    let at = p.size();
    match p.kind {
        Kind::Updated { .. } => {
            with_action(&p.title, Tone::Ok, radius, t!(g_whats_new), Msg::WhatsNew)
        }
        Kind::Updating => banner(
            spinner(logo(32), size::ICON, turn, still),
            &p.title,
            p.line(),
            None,
            radius,
            at,
        ),
        Kind::Ready { how, .. } => {
            let (glyph, words) = Pill::main(how);
            let actions = row![
                button(label(t!(up_later)), Button::Quiet, Some(Msg::Later)),
                labelled(
                    glyph.glyph(),
                    words,
                    labelled_width(words),
                    Button::Accent,
                    false,
                    Some(Msg::Main)
                ),
            ]
            .spacing(ACTION_GAP)
            .align_y(Center);
            let lead = icon(Icon::Download, size::ICON);
            banner(lead, &p.title, p.line(), Some(actions.into()), radius, at)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::{golden, shot};

    fn ready(how: How) -> Pill {
        Pill::new(Kind::Ready {
            version: "0.1.5".into(),
            how,
        })
    }

    /// The pill's words for each way an update installs, and its buttons: Later
    /// and Restart now (Get it where the user installs it).
    #[test]
    fn each_pill_says_what_happens() {
        let texts = |p: &Pill| shot::texts(view(p, RADIUS, 0.0, true));
        let t = texts(&ready(How::Restart));
        for want in [
            "OpenCircle 0.1.5 is ready",
            "Installs when you restart OpenCircle",
            "Later",
            "Restart now",
        ] {
            assert!(t.iter().any(|s| s == want), "{want}: {t:?}");
        }
        let t = texts(&ready(How::Setup));
        assert!(
            t.iter()
                .any(|s| s == "Windows asks once for permission to install")
        );
        let t = texts(&ready(How::GetIt));
        assert!(t.iter().any(|s| s == "OpenCircle 0.1.5 is out"), "{t:?}");
        assert!(t.iter().any(|s| s == "Get it"), "{t:?}");
        let t = texts(&Pill::new(Kind::Updating));
        assert!(t.iter().any(|s| s == "Updating OpenCircle…"), "{t:?}");
        let t = texts(&Pill::new(Kind::Updated {
            version: "0.1.5".into(),
        }));
        assert!(t.iter().any(|s| s == "What's new"), "{t:?}");
        assert!(Pill::new(Kind::Updating).spins() && !ready(How::Restart).spins());
    }

    /// Arabic: each pill mirrored (mark and buttons swap sides), from the table.
    #[test]
    fn every_arabic_pill_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            let _ar = crate::ui::kit::testing::arabic();
            for (name, p) in [
                ("ready", ready(How::Restart)),
                ("setup", ready(How::Setup)),
                ("updating", Pill::new(Kind::Updating)),
                (
                    "updated",
                    Pill::new(Kind::Updated {
                        version: "0.1.5".into(),
                    }),
                ),
            ] {
                let file = format!("update-pill-{name}-ar-{}", theme.id);
                failed.extend(golden::check("screens", &file, p.size(), &[1.0], || {
                    view(&p, RADIUS, 0.0, true)
                }));
            }
        });
    }

    /// Clicks: Later, the main button, What's new.
    #[test]
    fn its_buttons_say_what_they_do() {
        let p = ready(How::Restart);
        let size = p.size();
        let at = |x: f32| {
            shot::click(
                view(&p, RADIUS, 0.0, true),
                size,
                iced::Point::new(x, size.height / 2.0),
            )
        };
        assert!(matches!(at(size.width - 40.0)[..], [Msg::Main]));
        let later = size.width - 12.0 - labelled_width("Restart now") - ACTION_GAP - 10.0;
        assert!(matches!(at(later)[..], [Msg::Later]), "{size:?}");
        let up = Pill::new(Kind::Updated {
            version: "0.1.5".into(),
        });
        let size = up.size();
        let msgs = shot::click(
            view(&up, RADIUS, 0.0, true),
            size,
            iced::Point::new(size.width - 30.0, size.height / 2.0),
        );
        assert!(matches!(msgs[..], [Msg::WhatsNew]), "{msgs:?}");
    }

    /// Every pill in Sapphire and Daylight, at 1x and 2x.
    #[test]
    fn every_update_pill_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            for (name, p) in [
                ("ready", ready(How::Restart)),
                ("setup", ready(How::Setup)),
                ("get-it", ready(How::GetIt)),
                ("updating", Pill::new(Kind::Updating)),
                (
                    "updated",
                    Pill::new(Kind::Updated {
                        version: "0.1.5".into(),
                    }),
                ),
            ] {
                let file = format!("update-pill-{name}-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    p.size(),
                    &[1.0, 2.0],
                    || view(&p, RADIUS, 0.0, true),
                ));
            }
        });
    }
}
