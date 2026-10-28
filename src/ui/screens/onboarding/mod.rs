//! First-run onboarding (`widgets/onboarding.js`): a fixed 680x525 window,
//! the step track, one step at a time (`STEPS`: Welcome, Screenshots on
//! Wayland only, Connect AI, Look, Done) and the footer. Every setting it
//! shows is a Settings row (`settings::rows`, the same functions over an
//! embedded Settings state), so the words and controls stay one; Finish
//! writes that state's draft through `config::update_at`.
mod flow;
mod steps;

pub use flow::*;
pub use steps::screenshots::{Access, PROMPT_SIZE, ask, prompt_view};

#[cfg(test)]
mod tests;
