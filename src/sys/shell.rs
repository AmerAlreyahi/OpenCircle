//! The desktop shell's own full-screen view (GNOME's Activities overview: the app
//! grid and the window overview) is drawn above every app window, ours too, and a
//! new window does not end it: an overlay opened from a shortcut pressed there
//! stayed behind it until a click elsewhere (#173). Before the overlay comes to the
//! front the view is left, as GNOME's own screenshot tool does (GNOME Shell's
//! `OverviewActive` property, the one the shell's D-Bus API offers every app).
//! The capture is already taken by then: it shows what the user saw.
//!
//! Wayland: a window mapped while the overview shows does not get the keyboard
//! (Mutter keeps it with what had it before), and an app cannot take it later
//! without an activation token, which GNOME's custom shortcuts don't give. So the
//! portal capture waits for the overview to be gone before the overlay is made
//! (`leave_overview_and_wait`); X11 focuses the window after (`gain_focus`).
use gtk::gio::{self, prelude::*};
use gtk::glib::{self, Variant};

const SHELL: &str = "org.gnome.Shell";
const PATH: &str = "/org/gnome/Shell";
const PROPS: &str = "org.freedesktop.DBus.Properties";
/// A shell that does not answer in this time is left alone.
const TIMEOUT_MS: i32 = 500;

/// Leaves the shell's overview if it is showing; off the caller's thread (a bus
/// round trip). Nothing happens where no GNOME Shell answers.
pub fn leave_overview() {
    std::thread::spawn(|| match leave() {
        Ok(true) => crate::log!("[opencircle] left the Activities overview"),
        Ok(false) => {}
        Err(e) => crate::log!("[opencircle] could not leave the shell's overview: {e}"),
    });
}

/// Leaves the shell's overview and returns once it is gone (its hide animation,
/// ~0.25 s; at most `GONE_WAIT`), on the caller's thread. At once when none shows.
pub fn leave_overview_and_wait() {
    let left = match leave() {
        Ok(left) => left,
        Err(e) => {
            crate::log!("[opencircle] could not leave the shell's overview: {e}");
            return;
        }
    };
    if !left {
        return;
    }
    let started = std::time::Instant::now();
    // `OverviewActive` turns false with the overview's `hidden` signal: at the
    // end of its animation
    while shown().unwrap_or(false) && started.elapsed() < GONE_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    crate::log!(
        "[opencircle] left the Activities overview ({} ms)",
        started.elapsed().as_millis()
    );
}

/// The longest `leave_overview_and_wait` waits for the overview to go.
const GONE_WAIT: std::time::Duration = std::time::Duration::from_millis(800);

/// `Ok(true)` = the overview was showing and was asked to close.
fn leave() -> Result<bool, glib::Error> {
    let shown = match shown() {
        Ok(shown) => shown,
        Err(e) if not_gnome(&e) => return Ok(false),
        Err(e) => return Err(e),
    };
    if shown {
        call(
            "Set",
            (SHELL, "OverviewActive", false.to_variant()).to_variant(),
        )?;
    }
    Ok(shown)
}

/// Is the overview showing?
fn shown() -> Result<bool, glib::Error> {
    call("Get", (SHELL, "OverviewActive").to_variant()).map(|a| overview_shown(&a))
}

fn call(method: &str, args: Variant) -> Result<Variant, glib::Error> {
    // (gio opens the session bus once per process and shares it)
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    conn.call_sync(
        Some(SHELL),
        PATH,
        PROPS,
        method,
        Some(&args),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        TIMEOUT_MS,
        gio::Cancellable::NONE,
    )
}

/// The answer to `Get(OverviewActive)`: `(v)` holding a boolean.
fn overview_shown(answer: &Variant) -> bool {
    answer
        .try_child_value(0)
        .and_then(|v| v.as_variant())
        .and_then(|v| v.get::<bool>())
        .unwrap_or(false)
}

/// No GNOME Shell on this bus (KDE, wlroots, ...): not an error.
fn not_gnome(e: &glib::Error) -> bool {
    // gio names a remote error in its message: "GDBus.Error:<name>: <text>"
    ["ServiceUnknown", "NameHasNoOwner"].iter().any(|n| {
        e.message()
            .contains(&format!("org.freedesktop.DBus.Error.{n}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shells_answer() {
        let answer = |v: Variant| (v,).to_variant();
        assert!(overview_shown(&answer(true.to_variant())));
        assert!(!overview_shown(&answer(false.to_variant())));
        assert!(!overview_shown(&answer("yes".to_variant())));
        assert!(!overview_shown(&"junk".to_variant()));
        let remote = |name: &str| {
            glib::Error::new(
                gio::IOErrorEnum::DbusError,
                &format!("GDBus.Error:{name}: no"),
            )
        };
        assert!(not_gnome(&remote(
            "org.freedesktop.DBus.Error.ServiceUnknown"
        )));
        assert!(!not_gnome(&remote("org.freedesktop.DBus.Error.NoReply")));
    }
}
