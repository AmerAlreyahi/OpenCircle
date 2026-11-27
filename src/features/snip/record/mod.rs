//! Screen and audio recording (v0.6.0).
//! R-03: audio mixing, mute, timestamp gap silence fill and level metering (`mix`).
//! R-02: the pure state machine (`state`) and the pause-aware clock (`clock`).

pub mod clock;
pub mod fake;
pub mod files;
pub mod mix;
pub mod placement;
pub mod real;
pub mod session;
pub mod state;
pub mod zones;
