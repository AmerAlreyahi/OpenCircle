//! Work off the UI thread: `background` (a thread whose messages arrive as a `Task`)
//! and the shortcut triggers' stream.

use std::sync::Mutex;

use iced::Task;
use iced::futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, Stream, StreamExt};

use crate::Trigger;

/// Handed from `run` to the subscription (a subscription builder can't capture).
static TRIGGERS: Mutex<Option<UnboundedReceiver<Trigger>>> = Mutex::new(None);

/// Hands `run`'s trigger receiver to the subscription.
pub(in crate::ui) fn set_triggers(rx: UnboundedReceiver<Trigger>) {
    *TRIGGERS.lock().unwrap() = Some(rx);
}

/// Runs `work` on its own thread; what it sends arrives as messages, so the UI
/// never waits on it. Dropping (aborting) the task disconnects `work`'s sender.
pub(in crate::ui) fn background<T: Send + 'static>(
    work: impl FnOnce(&UnboundedSender<T>) + Send + 'static,
) -> Task<T> {
    let (tx, rx) = mpsc::unbounded();
    // (tests: the test's name, so what it copies is its own, `copy::copied`)
    let name = match cfg!(test) {
        true => std::thread::current().name().unwrap_or_default().into(),
        false => "background".into(),
    };
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || work(&tx));
    if let Err(e) = spawned {
        crate::log!("cannot start a thread: {e}");
    }
    Task::stream(rx)
}

pub(in crate::ui) fn triggers() -> impl Stream<Item = Trigger> {
    stream::iter(TRIGGERS.lock().unwrap().take()).flatten()
}
