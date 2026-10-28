//! `.oc-input` over several lines (the AI chat's composer): the one-line field's
//! look (`field::input`: 12 px on `track`, radius 10, a `rim` edge that turns
//! `accent` while focused), growing with its text up to `LINES` lines, then
//! scrolling. Enter is `on_enter`; Shift+Enter and Ctrl+Enter break the line.
use iced::keyboard::{self, key::Named};
use iced::widget::text_editor::{self, Action, Binding, Content, KeyPress, Status};
use iced::{Border, Element};

use super::themes::c;
use super::tokens::{Role, radius};

/// It grows up to this many lines, then scrolls.
pub const LINES: f32 = 5.0;
const LINE_HEIGHT: f32 = 1.5;
const PAD: [f32; 2] = [6.0, 11.0];

/// Its text's height at most (`LINES` lines), logical px; the field adds its
/// padding round it.
pub fn max_height() -> f32 {
    Role::NOTE.size * LINE_HEIGHT * LINES
}

/// What a key does in it: Enter (no Shift / Ctrl) = `on_enter`, Shift / Ctrl +
/// Enter = a new line, the rest as any text field.
pub fn binding<M: Clone>(press: KeyPress, on_enter: &M) -> Option<Binding<M>> {
    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }
    let m = press.modifiers;
    match press.key.as_ref() {
        keyboard::Key::Named(Named::Enter) if !m.shift() && !m.control() => {
            Some(Binding::Custom(on_enter.clone()))
        }
        keyboard::Key::Named(Named::Enter) => Some(Binding::Enter),
        _ => Binding::from_key_press(press),
    }
}

/// The field with `content`, its `id`, `placeholder`; `on_action` edits it.
pub fn text_area<'a, M: Clone + 'a>(
    id: &'static str,
    placeholder: &'a str,
    content: &'a Content,
    on_action: impl Fn(Action) -> M + 'a,
    on_enter: M,
) -> Element<'a, M> {
    text_editor::TextEditor::new(content)
        .id(id)
        .placeholder(placeholder)
        .on_action(on_action)
        .key_binding(move |press| binding(press, &on_enter))
        .font(super::text::font(Role::NOTE))
        .size(Role::NOTE.size)
        .line_height(LINE_HEIGHT)
        .padding(PAD)
        .height(iced::Shrink)
        .max_height(max_height())
        .style(|_, status| text_editor::Style {
            background: c().track.into(),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(match status {
                    Status::Focused { .. } => c().accent,
                    _ => c().rim,
                }),
            placeholder: c().text_muted,
            value: c().text,
            selection: c().accent_soft,
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Modifiers, key::Physical};

    fn press(key: Named, modifiers: Modifiers) -> KeyPress {
        KeyPress {
            key: keyboard::Key::Named(key),
            modified_key: keyboard::Key::Named(key),
            physical_key: Physical::Code(keyboard::key::Code::Enter),
            modifiers,
            text: None,
            status: Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn enter_sends_and_shift_or_ctrl_enter_break_the_line() {
        let send = |m| binding(press(Named::Enter, m), &"send");
        assert!(matches!(
            send(Modifiers::empty()),
            Some(Binding::Custom("send"))
        ));
        assert!(matches!(send(Modifiers::SHIFT), Some(Binding::Enter)));
        assert!(matches!(send(Modifiers::CTRL), Some(Binding::Enter)));
        let mut unfocused = press(Named::Enter, Modifiers::empty());
        unfocused.status = Status::Active;
        assert!(binding(unfocused, &"send").is_none(), "only while focused");
    }

    #[test]
    fn it_grows_to_five_lines_then_scrolls() {
        let line = Role::NOTE.size * LINE_HEIGHT;
        assert_eq!(max_height(), 5.0 * line);
    }
}
