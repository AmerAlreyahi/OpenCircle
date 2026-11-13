//! The desktop's activation token for the window a trigger opens (Linux: Wayland's
//! xdg-activation, X11's startup notification). It comes with the trigger (the CLI's
//! second line, the shortcut portal's `activation_token`) and is taken once by the
//! window that trigger opens; a token older than `FRESH` is dropped (a trigger that
//! opened nothing must not lend it to a later, unrelated window).
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_secs(10);

static SLOT: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Keeps `token` for the next window a trigger opens.
pub fn offer(token: String) {
    *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some((token, Instant::now()));
}

/// The token for the window opening now, once.
pub fn take() -> Option<String> {
    let taken = SLOT.lock().unwrap_or_else(|e| e.into_inner()).take();
    fresh(taken, Instant::now())
}

fn fresh(slot: Option<(String, Instant)>, now: Instant) -> Option<String> {
    slot.filter(|(_, at)| now.saturating_duration_since(*at) <= FRESH)
        .map(|(t, _)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_taken_once_and_only_while_fresh() {
        let at = Instant::now();
        let slot = |t: &str| Some((t.to_string(), at));
        assert_eq!(fresh(slot("a"), at + FRESH), Some("a".into()));
        assert_eq!(
            fresh(slot("a"), at + FRESH + Duration::from_millis(1)),
            None
        );
        assert_eq!(fresh(None, at), None);
        offer("t1".into());
        offer("t2".into()); // the newer trigger's
        assert_eq!(take(), Some("t2".into()));
        assert_eq!(take(), None, "once");
    }
}
