//! The failure card window (F12): the kit's one error card (what happened,
//! what to do, one fix button, the system's own text folded) in a small floating
//! window, for a capture or a step that could not be made. One at a time; a new
//! failure replaces the open one; Esc or the fix closes it.
use iced::widget::container;
use iced::window::{self, Position};
use iced::{Element, Fill, Size, Task};

use super::{Msg, Windows, appear, close, floating, spoken};
use crate::shared::kit::error_card::{ErrorCard, error_card};
use crate::shared::kit::text;
use crate::shared::kit::themes::c;
use crate::shared::kit::tokens::{Role, radius, size};

/// The window's width (the card's).
const WIDTH: f32 = 400.0;
/// The corners the window is cut to.
pub const RADIUS: f32 = radius::CARD;
/// Around the card's text: its padding, the icon and the gap beside it.
const SIDE: f32 = 2.0 * 12.0 + size::ICON + 10.0;
/// The card without its text lines: padding, headline, buttons, gaps.
const CHROME: f32 = 2.0 * 12.0 + 20.0 + 44.0 + 2.0 * 6.0;
/// One line of the hint and of the folded details.
const LINE: f32 = 18.0;

/// What the card says and what its fix does.
#[derive(Debug, Clone)]
pub struct Failure {
    pub headline: String,
    pub hint: String,
    /// The system's own text ("" = none: no Show details).
    pub detail: String,
    /// The fix button's words and what it runs.
    pub fix: Option<(&'static str, crate::Trigger)>,
}

impl Failure {
    /// The window's size with the details `open` or folded.
    pub fn size(&self, open: bool) -> Size {
        let room = WIDTH - SIDE;
        let (note, face) = (text::size(Role::NOTE), text::font(Role::NOTE));
        let lines = |s: &str| text::lines(s, note, face, room).max(1) as f32;
        let mut h = CHROME + lines(&self.hint) * LINE;
        if open && !self.detail.is_empty() {
            h += lines(&self.detail) * LINE + 2.0 * 6.0 + 6.0;
        }
        Size::new(WIDTH, h.ceil())
    }

    fn view(&self, open: bool) -> Element<'_, Msg> {
        let card = error_card(ErrorCard {
            headline: self.headline.clone(),
            hint: self.hint.clone(),
            detail: self.detail.clone(),
            width: WIDTH,
            open,
            on_details: Msg::FailureDetails,
            fix: self
                .fix
                .as_ref()
                .map(|(words, _)| (*words, Msg::FailureFix)),
            retry: None,
            words: crate::shared::theme::error_words(),
        });
        container(card)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style::default().background(c().elevated))
            .into()
    }
}

/// Snip again: the trigger of the user's own Snip shortcut (`None`: no quick action).
fn snip_again() -> Option<crate::Trigger> {
    use crate::shared::platform::hotkey::QuickId;
    let snip = (crate::shared::config::current().quick_actions.into_iter())
        .find(|q| q.action == QuickId::Snip && !q.live)?;
    Some(crate::Trigger::Quick(std::time::Instant::now(), snip))
}

/// A step that could not be made (`why`: the system's own text).
pub fn step_failed(why: String) -> Failure {
    use crate::t;
    Failure {
        headline: t!(sn_step_failed_h).into(),
        hint: t!(sn_step_failed_d).into(),
        detail: why,
        fix: snip_again().map(|trigger| (t!(sn_step_again), trigger)),
    }
}

/// What to do, in the words of the platform: only Wayland has a desktop that says no.
fn capture_hint() -> &'static str {
    use crate::t;
    #[cfg(target_os = "linux")]
    if crate::shared::platform::sys::session::wayland_session() {
        return t!(cap_failed_wayland);
    }
    t!(cap_failed_d)
}

/// The screen could not be captured (`why`: the system's own text).
pub fn capture_failed(why: String) -> Failure {
    use crate::t;
    Failure {
        headline: t!(cap_failed).into(),
        hint: capture_hint().into(),
        detail: why,
        fix: Some((
            t!(md_try_again),
            crate::Trigger::Capture(std::time::Instant::now()),
        )),
    }
}

impl Windows {
    /// Shows `failure`, folded (a new one replaces the open one).
    pub fn failure(&mut self, failure: Failure) -> Task<Msg> {
        self.show_failure(failure, false)
    }

    /// The card's Show / Hide details: a window of the other size replaces it
    /// (Wayland can't resize one in place).
    pub(super) fn failure_details(&mut self) -> Task<Msg> {
        let Some((_, failure, open)) = self.failure.take() else {
            return Task::none();
        };
        self.show_failure(failure, !open)
    }

    /// The fix: the card goes, then its trigger runs.
    pub(super) fn failure_fix(&mut self) -> Task<Msg> {
        let Some((id, failure, _)) = self.failure.take() else {
            return Task::none();
        };
        let run = failure
            .fix
            .map(|(_, trigger)| Task::done(Msg::Run(trigger)));
        Task::batch([close(id), run.unwrap_or_else(Task::none)])
    }

    /// Esc: the card goes.
    pub(super) fn failure_close(&mut self) -> Task<Msg> {
        match self.failure.take() {
            Some((id, ..)) => close(id),
            None => Task::none(),
        }
    }

    /// The open card's text, folded or not (tests).
    #[cfg(test)]
    pub fn failure_open(&self) -> Option<(&Failure, bool)> {
        self.failure.as_ref().map(|(_, f, open)| (f, *open))
    }

    fn show_failure(&mut self, failure: Failure, open: bool) -> Task<Msg> {
        let old = self.failure.take().map(|(id, ..)| close(id));
        let size = spoken(|| failure.size(open));
        let (id, shown) = floating(
            window::Settings {
                size,
                position: Position::Centered,
                resizable: false,
                ..window::Settings::default()
            },
            RADIUS,
            super::TOAST,
        );
        // (placed by the desktop: centred; shown once cut)
        let shown = shown
            .then(move |id| appear::<()>(id, super::TOAST))
            .discard();
        self.failure = Some((id, failure, open));
        Task::batch([old.unwrap_or_else(Task::none), shown])
    }

    /// The failure card's content for its window (`Windows::view`).
    pub(super) fn failure_view(&self, window: window::Id) -> Option<Element<'_, Msg>> {
        let (_, failure, open) = self.failure.as_ref().filter(|(id, ..)| *id == window)?;
        Some(spoken(|| failure.view(*open)))
    }
}
