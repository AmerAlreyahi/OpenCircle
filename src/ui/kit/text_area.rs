//! The AI chat's composer (`ai-chat.js:379-411`): one `.oc-input` box (track,
//! a `rim` edge, radius 16, 10 px in, its parts 8 px apart) holding, top to
//! bottom, an optional line over the field (the attached screen's chip), the
//! message field (13 px, borderless, growing with its text up to `LINES`
//! lines, then scrolling) and its controls row. Enter is `on_enter`;
//! Shift+Enter and Ctrl+Enter break the line. As in the design, focus changes
//! nothing round the box (the caret shows it).
use iced::keyboard::{self, key::Named};
use iced::widget::text_editor::{self, Action, Binding, Content, KeyPress, Status};
use iced::widget::{column, container, mouse_area, stack};
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
    super::text::size(Role::CHAT) * LINE_HEIGHT * LINES
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
/// edits it): `top` over the field, `controls` under it. A press anywhere in the
/// box that no part takes (its padding, beside the line) is `on_focus`: the caller
/// focuses the field (`operation::focus(id)`), so the whole box is the field.
#[allow(clippy::too_many_arguments)]
pub fn text_area<'a, M: Clone + 'a>(
    id: &'static str,
    placeholder: &'a str,
    content: &'a Content,
    on_action: impl Fn(Action) -> M + 'a,
    on_enter: M,
    top: Option<Element<'a, M>>,
    controls: Element<'a, M>,
    on_focus: M,
) -> Element<'a, M> {
    let field = text_editor::TextEditor::new(content)
        .id(id)
        // (right to left the editor's own placeholder would sit at the left:
        // an overlay below draws it from the start edge)
        .placeholder(if super::dir::rtl() { "" } else { placeholder })
        .on_action(on_action)
        .key_binding(move |press| binding(press, &on_enter))
        .font(super::text::font(Role::CHAT))
        .size(super::text::size(Role::CHAT))
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
    // the field, with the placeholder over it from the start edge while it is empty (right to left)
    let field: Element<'a, M> = match super::dir::rtl() && content.is_empty() {
        true => stack![
            field,
            container(
                super::text::paragraph(
                    super::text::muted(Role::CHAT, placeholder).line_height(LINE_HEIGHT)
                )
                .width(Fill)
            )
            .padding(PAD)
        ]
        .into(),
        false => field.into(),
    };
    let parts = column![]
        .push(top)
        .push(field)
        .push(controls)
        .spacing(GAP)
        .width(Fill);
    let boxed = container(parts)
        .padding(BOX_PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().track.into()),
            border: Border::default()
                .rounded(radius::WINDOW)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        });
    mouse_area(boxed)
        .on_press(on_focus)
        .interaction(iced::mouse::Interaction::Text)
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
    fn a_press_anywhere_in_the_box_focuses_the_field() {
        // X11 (#172): a click above or beside the text line did nothing
        let content = Content::with_text("hi");
        let view = || {
            let controls = iced::widget::space::horizontal().height(28).into();
            text_area(
                "t",
                "",
                &content,
                |_| "edit",
                "send",
                None,
                controls,
                "focus",
            )
        };
        let size = iced::Size::new(400.0, 120.0);
        let pad = crate::ui::kit::testing::shot::click(view(), size, iced::Point::new(4.0, 4.0));
        assert_eq!(pad, ["focus"], "the box's padding focuses the field");
        let right =
            crate::ui::kit::testing::shot::click(view(), size, iced::Point::new(390.0, 40.0));
        assert_eq!(right, ["focus"], "beside the controls too");
    }

    /// Right to left (the Arabic UI) the empty field's placeholder starts at the
    /// right edge, like the text typed in it (iced's editor has no alignment of its own).
    #[test]
    fn the_placeholder_starts_at_the_right_edge_in_arabic() {
        let ink = |lang| {
            crate::app_language::set(lang);
            let content = Content::new();
            let controls = iced::widget::space::horizontal().height(28).into();
            let v = text_area("t", "اسأل الذكاء", &content, |_| "e", "s", None, controls, "f");
            let px = crate::ui::kit::testing::shot::render(v, 400, 120);
            let bg = px[(60 * 400 + 2) * 4];
            let xs: Vec<usize> = (12..40)
                .flat_map(|y| (14..386).map(move |x| (x, y)))
                .filter(|&(x, y)| (px[(y * 400 + x) * 4] as i32 - bg as i32).abs() > 40)
                .map(|(x, _)| x)
                .collect();
            (xs.iter().min().copied().unwrap(), xs.iter().max().copied().unwrap())
        };
        let (lo, hi) = ink(crate::app_language::Lang::Ar);
        assert!(lo > 200 && hi > 360, "at the right: {lo}..{hi}");
        crate::app_language::set(crate::app_language::Lang::En);
    }

    #[test]
    fn it_grows_to_five_lines_then_scrolls() {
        let line = Role::CHAT.size * LINE_HEIGHT;
        assert_eq!(max_height(), 5.0 * line);
    }
}
