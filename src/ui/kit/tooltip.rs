//! A control's name and, when it adds something, a visible tooltip: the
//! design's are native `title`s (DESIGN_BEHAVIOR §2, "Tooltips"), drawn here
//! as the one hint pill (`surface::hint`) beside the control.
use iced::Element;
use iced::widget::tooltip::Position;

/// The pill's distance from its control.
const GAP: f32 = 10.0;

/// Why a control shows a visible tooltip at all. A tooltip that only repeats
/// what the control already says is noise, so these are the only three reasons.
#[derive(Debug, Clone, Copy)]
pub enum Why<'a> {
    /// Nothing but a glyph on it: the label is its only name.
    Icon,
    /// The shortcut is not written on the control: "Label · Ctrl+C".
    Keys(&'a str),
    /// Its text is cut short: the tooltip is the whole text.
    Cut(&'a str),
}

/// A control's accessible name (screen readers, automation, the unique-label
/// test) with a visible tooltip for one of the `Why` reasons.
pub fn hint<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    why: Why<'a>,
    side: Position,
) -> Element<'a, Message> {
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut content = content.into();
    let shown = match why {
        Why::Icon => label.to_string(),
        Why::Keys(keys) => format!("{label} · {keys}"),
        Why::Cut(full) => full.to_string(),
    };
    #[cfg(test)]
    tests_support::record_shown(&mut content, label, why, &shown);
    iced::widget::tooltip(content, super::surface::hint(shown), side)
        .gap(GAP)
        .into()
}

/// A control's accessible name (iced has no a11y tree): what automation calls it,
/// and what the test registry checks is unique per screen. It shows NO tooltip,
/// since the control's own text says it; `hint` is for icon-only, cut or
/// shortcut-carrying controls. `side` stays so a call can become a `hint` in place.
pub fn tip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    _side: Position,
) -> Element<'a, Message> {
    #[cfg(test)]
    TIPS.with(|t| t.borrow_mut().push(label.to_string()));
    #[cfg(not(test))]
    let _ = label;
    content.into()
}

#[cfg(test)]
thread_local! {
    pub static TIPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Every visible tooltip built (its text, why, and the texts its control shows).
    pub static SHOWN: std::cell::RefCell<Vec<tests_support::Shown>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The tooltip policy as a test: what a screen's visible tooltips say must add
/// something to what their controls already show.
#[cfg(test)]
pub mod tests_support {
    use super::{SHOWN, TIPS, Why};
    use iced::advanced::layout::{Layout, Limits};
    use iced::advanced::widget::{Id, Operation, Tree};
    use iced::{Element, Font, Pixels, Rectangle, Size};

    pub struct Shown {
        pub label: String,
        pub why: String,
        pub tip: String,
        pub visible: Vec<String>,
    }

    /// The texts `content` shows (labels and typed text, glyphs included).
    fn texts<Message>(content: &mut Element<'_, Message>) -> Vec<String> {
        struct Collect(Vec<String>);
        impl Operation for Collect {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
                self.0.push(text.to_string());
            }
        }
        let renderer = iced::Renderer::new(Font::DEFAULT, Pixels(16.0));
        let mut tree = Tree::new(content.as_widget());
        let limits = Limits::new(Size::ZERO, Size::new(900.0, 700.0));
        let node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut collect = Collect(Vec::new());
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut collect);
        collect.0
    }

    pub fn record_shown<Message>(
        content: &mut Element<'_, Message>,
        label: &str,
        why: Why<'_>,
        tip: &str,
    ) {
        TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        let visible = texts(content);
        SHOWN.with(|s| {
            s.borrow_mut().push(Shown {
                label: label.to_string(),
                why: format!("{why:?}"),
                tip: tip.to_string(),
                visible,
            })
        });
    }

    /// The visible tooltips built since the last check (their texts).
    pub fn shown() -> Vec<String> {
        SHOWN.with(|s| s.borrow().iter().map(|s| s.tip.clone()).collect())
    }

    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// Asserts that every visible tooltip built since the last call says something
    /// its control does not: not its text again, not its shortcut again, and an
    /// icon-only control has no words on it. Clears the record.
    pub fn assert_tips_add_something(screen: &str) {
        let shown = SHOWN.with(|s| std::mem::take(&mut *s.borrow_mut()));
        for s in shown {
            let (tip, label) = (norm(&s.tip), norm(&s.label));
            for v in s.visible.iter().map(|v| norm(v)).filter(|v| !v.is_empty()) {
                assert!(
                    v != tip && v != label,
                    "{screen}: the tooltip {:?} repeats {:?} on its control",
                    s.tip,
                    s.visible
                );
                if s.why.starts_with("Keys") {
                    assert!(
                        !tip.ends_with(&v),
                        "{screen}: {:?} already shows its shortcut",
                        s.tip
                    );
                }
            }
            if s.why == "Icon" {
                let words = s
                    .visible
                    .iter()
                    .any(|v| v.chars().any(char::is_alphanumeric));
                assert!(!words, "{screen}: {:?} has words, so no tooltip", s.tip);
            }
        }
    }
}
