//! The live translate mini-controller (Circle to Search's floating controller):
//! a small, draggable window on the layer's monitor. It takes clicks (the layer
//! doesn't) and, on Windows, is excluded from capture like the layer. Language
//! pair chip (opens a short list of targets), Pause / Resume, Hide (collapses to
//! a dot), Stop. This file is its state and view; `live_layer` owns the window.
use std::sync::Mutex;

use iced::widget::{column, container, mouse_area, row};
use iced::{Center, Element, Size, mouse};

use super::widgets;

/// Offered after the recently used targets.
const COMMON: [&str; 6] = ["ar", "en", "es", "fr", "de", "ja"];
/// How many targets the list offers.
const CHOICES: usize = 6;
/// Targets picked in this run, most recent first (memory only).
static RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Chip sizes (one width per kind, consistency rule) and the drag grip.
const CHIP_HEIGHT: f32 = 32.0;
const SPACING: f32 = 4.0;
const PAIR_WIDTH: f32 = 92.0;
const CHIP_WIDTH: f32 = 76.0;
const CHOICE_WIDTH: f32 = 48.0;
const GRIP_WIDTH: f32 = 20.0;
/// The minimised controller.
const DOT: f32 = 28.0;

#[derive(Debug, Clone)]
pub enum Msg {
    /// Pressed on the grip: move the window with the pointer.
    Drag,
    /// The language chip: open / close the list of targets.
    List,
    Pick(String),
    Pause,
    Minimise,
    Restore,
    Stop,
}

/// What the layer must do after a message.
#[derive(Debug, PartialEq)]
pub enum Out {
    Drag,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    /// The target language code (e.g. "ar").
    pub target: String,
    pub paused: bool,
    pub minimised: bool,
    /// The list of targets is open: (code, label).
    list: Option<Vec<(String, String)>>,
    /// "EN → AR".
    pair: String,
}

impl Controller {
    pub fn new(target: &str) -> Controller {
        Controller {
            target: target.into(),
            paused: false,
            minimised: false,
            list: None,
            pair: pair(target),
        }
    }

    pub fn update(&mut self, msg: Msg) -> Option<Out> {
        match msg {
            Msg::Drag => return Some(Out::Drag),
            Msg::Stop => return Some(Out::Stop),
            Msg::List if self.list.is_some() => self.list = None,
            Msg::List => self.list = Some(choices(&self.target, &recent())),
            Msg::Pick(target) => {
                remember(&target);
                self.pair = pair(&target);
                self.target = target;
                self.list = None;
            }
            Msg::Pause => self.paused = !self.paused,
            Msg::Minimise => {
                self.minimised = true;
                self.list = None;
            }
            Msg::Restore => self.minimised = false,
        }
        None
    }

    /// The status line: "EN → AR", or paused.
    pub fn status(&self) -> String {
        match self.paused {
            true => "Live · paused".into(),
            false => format!("Live · {}", self.pair),
        }
    }

    /// The window's size for what it shows now (logical px).
    pub fn size(&self) -> Size {
        if self.minimised {
            return Size::new(DOT, DOT);
        }
        let pad = 2.0 * widgets::SURFACE_PADDING;
        let list = if self.list.is_some() {
            CHIP_HEIGHT + SPACING
        } else {
            0.0
        };
        Size::new(
            GRIP_WIDTH + PAIR_WIDTH + 3.0 * CHIP_WIDTH + 4.0 * SPACING + pad,
            CHIP_HEIGHT + list + pad,
        )
    }

    pub fn view(&self) -> Element<'_, Msg> {
        if self.minimised {
            return widgets::accent_dot(DOT, Msg::Restore).into();
        }
        let chip = |label: &'static str, msg| {
            widgets::chip(label, msg)
                .width(CHIP_WIDTH)
                .height(CHIP_HEIGHT)
        };
        let grip = mouse_area(
            container(widgets::caption("⋮⋮"))
                .center(GRIP_WIDTH)
                .height(CHIP_HEIGHT),
        )
        .on_press(Msg::Drag)
        .interaction(mouse::Interaction::Grab);
        let controls = row![
            grip,
            widgets::chip(&self.pair, Msg::List)
                .width(PAIR_WIDTH)
                .height(CHIP_HEIGHT),
            chip(if self.paused { "Resume" } else { "Pause" }, Msg::Pause),
            chip("Hide", Msg::Minimise),
            chip("Stop", Msg::Stop),
        ]
        .spacing(SPACING)
        .align_y(Center);
        let mut body = column![controls].spacing(SPACING);
        if let Some(list) = &self.list {
            let picks = list.iter().map(|(code, label)| {
                widgets::chip(label, Msg::Pick(code.clone()))
                    .width(CHOICE_WIDTH)
                    .height(CHIP_HEIGHT)
                    .into()
            });
            let picks = row(picks).spacing(SPACING);
            body = body.push(container(picks).padding([0.0, GRIP_WIDTH + SPACING]));
        }
        widgets::solid_surface(body).into()
    }
}

fn pair(target: &str) -> String {
    format!("EN → {}", target.to_uppercase())
}

fn recent() -> Vec<String> {
    RECENT.lock().map(|r| r.clone()).unwrap_or_default()
}

/// Remembers a picked target, most recent first.
fn remember(target: &str) {
    if let Ok(mut recent) = RECENT.lock() {
        recent.retain(|t| t != target);
        recent.insert(0, target.into());
        recent.truncate(CHOICES);
    }
}

/// The targets to offer: recently used first, then common ones; never the
/// current one, no repeats. (code, label).
fn choices(current: &str, recent: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<String> = Vec::new();
    for t in recent.iter().cloned().chain(COMMON.map(String::from)) {
        if t != current && !out.contains(&t) {
            out.push(t);
        }
    }
    out.truncate(CHOICES);
    out.into_iter()
        .map(|code| {
            let label = code.to_uppercase();
            (code, label)
        })
        .collect()
}

/// Paints `rect` (x, y, w, h; clipped to the frame) black in an RGBA capture:
/// where a capture shows the controller (X11), live mode never reads its labels.
pub fn mask(rgba: &mut [u8], width: u32, height: u32, rect: [u32; 4]) {
    let [x, y, w, h] = rect;
    let (x0, x1) = (x.min(width), x.saturating_add(w).min(width));
    for row in y.min(height)..y.saturating_add(h).min(height) {
        let start = (row * width + x0) as usize * 4;
        let end = (row * width + x1) as usize * 4;
        if let Some(px) = rgba.get_mut(start..end) {
            px.as_chunks_mut::<4>().0.fill([0, 0, 0, 255]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_hide_and_stop() {
        let mut c = Controller::new("ar");
        assert_eq!(c.status(), "Live · EN → AR");
        let full = c.size();
        assert_eq!(c.update(Msg::Pause), None);
        assert!(c.paused);
        assert_eq!(c.status(), "Live · paused");
        assert_eq!(c.update(Msg::Pause), None);
        assert!(!c.paused, "Pause again = Resume");
        // Hide: a dot, and the list closes with it
        c.update(Msg::List);
        c.update(Msg::Minimise);
        assert!(c.minimised && c.list.is_none());
        assert_eq!(c.size(), Size::new(DOT, DOT));
        c.update(Msg::Restore);
        assert_eq!(c.size(), full);
        assert_eq!(c.update(Msg::Drag), Some(Out::Drag));
        assert_eq!(c.update(Msg::Stop), Some(Out::Stop));
    }

    #[test]
    fn the_language_chip_lists_targets_and_a_pick_switches() {
        let mut c = Controller::new("ar");
        let closed = c.size();
        c.update(Msg::List);
        assert!(c.size().height > closed.height, "the list needs room");
        let list = c.list.clone().unwrap();
        assert!(
            list.iter().all(|(code, _)| code != "ar"),
            "not the current one"
        );
        assert!(list.contains(&("en".into(), "EN".into())));
        c.update(Msg::List);
        assert_eq!(c.size(), closed, "the chip closes it again");
        c.update(Msg::List);
        c.update(Msg::Pick("ja".into()));
        assert_eq!((c.target.as_str(), c.list.as_ref()), ("ja", None));
        assert_eq!(c.status(), "Live · EN → JA");
        assert_eq!(c.size(), closed);
    }

    #[test]
    fn recent_targets_come_first_without_repeats() {
        let recent = ["de".to_string(), "ko".into(), "ar".into()];
        let got: Vec<_> = choices("ar", &recent).into_iter().map(|(c, _)| c).collect();
        assert_eq!(got, ["de", "ko", "en", "es", "fr", "ja"]);
        // the view builds in every state
        let mut c = Controller::new("ar");
        let _ = c.view();
        c.update(Msg::List);
        let _ = c.view();
        c.update(Msg::Minimise);
        let _ = c.view();
    }

    #[test]
    fn mask_blanks_only_its_rect_and_clips() {
        let mut rgba = vec![9; 4 * 3 * 4];
        mask(&mut rgba, 4, 3, [2, 1, 10, 10]);
        let black = |x: usize, y: usize| rgba[(y * 4 + x) * 4..][..4] == [0, 0, 0, 255];
        assert!(black(2, 1) && black(3, 2));
        assert!(!black(1, 1) && !black(2, 0));
        mask(&mut rgba, 4, 3, [9, 9, 2, 2]); // off the frame: nothing
    }
}
