//! The AI chat's composer (`ai-chat.js:379-411`): one `.oc-input` box (track,
//! a `rim` edge, radius 16, 10 px in, its parts 8 px apart) holding, top to
//! bottom, an optional line over the field (the attached screen's chip), the
//! message field (13 px, borderless, growing with its text up to `LINES`
//! lines, then scrolling) and its controls row. Enter is `on_enter`;
//! Shift+Enter and Ctrl+Enter break the line. As in the design, focus changes
//! nothing round the box (the caret shows it).
use iced::keyboard::{self, key::Named};
use iced::widget::text_editor::{self, Action, Binding, Content, KeyPress, Status};
use iced::widget::{column, container};
use iced::{Border, Color, Element, Fill};

use super::themes::c;
use super::tokens::{Role, radius};

/// It grows up to this many lines, then scrolls.
pub const LINES: f32 = 5.0;
const LINE_HEIGHT: f32 = 1.5;
/// The field's text room (`px-1.5`), the box's padding (`p-2.5`) and the gap
/// between its parts (`gap-2`).
const PAD: [f32; 2] = [0.0, 6.0];
const BOX_PAD: f32 = 10.0;
const GAP: f32 = 8.0;

/// Its text's height at most (`LINES` lines), logical px.
pub fn max_height() -> f32 {
    Role::CHAT.size * LINE_HEIGHT * LINES
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

/// The composer with `content` in its field (`id`, `placeholder`; `on_action`
/// edits it): `top` over the field, `controls` under it.
pub fn text_area<'a, M: Clone + 'a>(
    id: &'static str,
    placeholder: &'a str,
    content: &'a Content,
    on_action: impl Fn(Action) -> M + 'a,
    on_enter: M,
    top: Option<Element<'a, M>>,
    controls: Element<'a, M>,
) -> Element<'a, M> {
    let field = text_editor::TextEditor::new(content)
        .id(id)
        .placeholder(placeholder)
        .on_action(on_action)
        .key_binding(move |press| binding(press, &on_enter))
        .font(super::text::font(Role::CHAT))
        .size(Role::CHAT.size)
        .line_height(LINE_HEIGHT)
        .padding(PAD)
        .height(iced::Shrink)
        .max_height(max_height())
        .style(|_, _| text_editor::Style {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            placeholder: c().text_muted,
            value: c().text,
            selection: c().accent_soft,
        });
    let parts = column![]
        .push(top)
        .push(field)
        .push(controls)
        .spacing(GAP)
        .width(Fill);
    container(parts)
        .padding(BOX_PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().track.into()),
            border: Border::default()
                .rounded(radius::WINDOW)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
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
        let line = Role::CHAT.size * LINE_HEIGHT;
        assert_eq!(max_height(), 5.0 * line);
    }
}
