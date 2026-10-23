//! Shared floating components. Every floating widget is built here, styled only
//! from `theme.rs`; screens never style a container or button themselves.
//!
//! Glass later: blur the frozen frame once per overlay and paint the blurred crop
//! behind each surface (no nested/live blur).
use iced::widget::{Button, Container, Tooltip, button, container, row, space, text, tooltip};
use iced::{Background, Border, Color, Element, Font, Shadow, Vector, gradient};

use super::theme;

/// The icon font (Lucide subset, see `assets/lucide/`); load it once at start.
pub const ICON_FONT_BYTES: &[u8] = include_bytes!("../../assets/lucide/lucide-subset.ttf");
const ICON_FONT: Font = Font::with_name("lucide");

/// Every button is this tall (DESIGN.md: 44 px controls).
pub const CONTROL: f32 = 44.0;
/// Text buttons share one width (consistency rule), so bars have a known size.
pub const TEXT_BUTTON_WIDTH: f32 = 136.0;
/// Inner padding of a `surface`.
pub const SURFACE_PADDING: f32 = 6.0;

/// Fully rounded glass pill with one line of text (overlay hint, tooltips).
pub fn pill<'a, Message: 'a>(label: &'a str) -> Container<'a, Message> {
    let palette = theme::current();
    container(text(label).size(14).color(palette.text))
        .padding([8, 18])
        .style(|_| glass(999.0))
}

/// Glass panel (20 px radius): action bar, toolbars, popovers, side panels.
pub fn surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content)
        .padding(SURFACE_PADDING)
        .style(|_| glass(20.0))
}

/// Icon + label + key hint. `primary` = accent gradient (the default action),
/// otherwise quiet (transparent until hovered).
pub fn text_button<'a, Message: Clone + 'a>(
    icon: char,
    label: &'a str,
    key_hint: String,
    primary: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::current();
    let (fg, hint) = if primary {
        (theme::ON_ACCENT, theme::ON_ACCENT)
    } else {
        (palette.text, palette.muted)
    };
    let content = row![
        text(icon).font(ICON_FONT).size(16).color(fg),
        text(label).size(14).color(fg),
        space::horizontal(),
        text(key_hint).size(11).color(hint),
    ]
    .spacing(8)
    .height(iced::Fill)
    .align_y(iced::Center);
    button(content)
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .padding([0, 12])
        .on_press(on_press)
        .style(move |_, status| {
            let background = match (primary, status) {
                (true, _) => Some(accent(status)),
                (false, button::Status::Hovered) => Some(palette.hover.into()),
                (false, button::Status::Pressed) => Some(palette.pressed.into()),
                (false, _) => None,
            };
            button::Style {
                background,
                text_color: fg,
                border: Border::default().rounded(14),
                ..button::Style::default()
            }
        })
}

/// Shows `tip` in a pill beside `content` on hover; also the control's description.
pub fn with_tip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    tip: &'a str,
    side: tooltip::Position,
) -> Tooltip<'a, Message> {
    tooltip(content, pill(tip), side).gap(10)
}

/// Square 44x44 icon button (toolbar tools, back, undo/redo): quiet, hover,
/// `active` = accent gradient.
pub fn icon_button<'a, Message: Clone + 'a>(
    glyph: char,
    active: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let fg = if active {
        theme::ON_ACCENT
    } else {
        theme::current().text
    };
    square_button(
        text(glyph).font(ICON_FONT).size(18).color(fg),
        active,
        on_press,
    )
}

/// Thin vertical line between toolbar groups.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(space::horizontal())
        .width(1)
        .height(24)
        .style(|_| container::Style {
            background: Some(theme::current().border.into()),
            ..container::Style::default()
        })
        .into()
}

/// The ONE colour control: a 44x44 button showing the current colour; it opens
/// `colour_popover`.
pub fn colour_button<'a, Message: Clone + 'a>(
    colour: Color,
    open: bool,
    on_press: Message,
) -> Button<'a, Message> {
    square_button(dot(colour, 22.0, true), open, on_press)
}

/// The colour picker popover: the colour swatches, stroke widths (thin/medium/
/// thick, `width` = the chosen one) and a custom colour as hex.
#[allow(clippy::too_many_arguments)]
pub fn colour_popover<'a, Message: Clone + 'a>(
    colours: &[Color],
    current: Color,
    width: usize,
    hex: &'a str,
    on_colour: impl Fn(Color) -> Message,
    on_width: impl Fn(usize) -> Message,
    on_hex: impl Fn(String) -> Message + 'a,
) -> Container<'a, Message> {
    let palette = theme::current();
    let swatch = |&c: &Color| -> Element<'a, Message> {
        let ring = if c == current {
            palette.text
        } else {
            palette.border
        };
        button(dot(c, 26.0, true))
            .padding(3)
            .on_press(on_colour(c))
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered).then(|| palette.hover.into()),
                border: Border {
                    color: ring,
                    width: if c == current { 2.0 } else { 0.0 },
                    radius: 999.0.into(),
                },
                ..button::Style::default()
            })
            .into()
    };
    let rows = colours
        .chunks(6)
        .map(|chunk| row(chunk.iter().map(swatch)).spacing(6).into());
    let widths = row((0..3).map(|i| {
        let size = [4.0, 8.0, 12.0][i];
        square_button(dot(palette.text, size, false), i == width, on_width(i)).into()
    }))
    .spacing(4);
    use iced::widget::{column, text_input};
    let custom = text_input("#RRGGBB", hex)
        .on_input(on_hex)
        .size(14)
        .padding([8, 12])
        .style(|_, _| text_input::Style {
            background: palette.hover.into(),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            icon: palette.muted,
            placeholder: palette.muted,
            value: palette.text,
            selection: theme::ACCENT_START.scale_alpha(0.4),
        });
    surface(
        column![column(rows).spacing(6), widths, custom]
            .spacing(10)
            .padding(4)
            .width(6.0 * 32.0 + 5.0 * 6.0 + 8.0),
    )
}

/// A filled circle; `ring` adds a thin outline (so white reads on a light panel).
fn dot<'a, Message: 'a>(colour: Color, size: f32, ring: bool) -> Element<'a, Message> {
    let border = theme::current().border;
    container(space::horizontal())
        .width(size)
        .height(size)
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border {
                color: border,
                width: if ring { 1.5 } else { 0.0 },
                radius: 999.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// 44x44 button around centred `content`, styled like a quiet `text_button`
/// (accent gradient when `active`).
fn square_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    active: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::current();
    button(container(content).center(iced::Fill))
        .width(CONTROL)
        .height(CONTROL)
        .padding(0)
        .on_press(on_press)
        .style(move |_, status| {
            let background = match (active, status) {
                (true, _) => Some(accent(status)),
                (false, button::Status::Hovered) => Some(palette.hover.into()),
                (false, button::Status::Pressed) => Some(palette.pressed.into()),
                (false, _) => None,
            };
            button::Style {
                background,
                text_color: palette.text,
                border: Border::default().rounded(14),
                ..button::Style::default()
            }
        })
}

/// The accent gradient; hover/press brighten it a little.
fn accent(status: button::Status) -> Background {
    let lift = match status {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.14,
        _ => 0.0,
    };
    let mix = |c: Color| Color {
        r: c.r + (1.0 - c.r) * lift,
        g: c.g + (1.0 - c.g) * lift,
        b: c.b + (1.0 - c.b) * lift,
        a: c.a,
    };
    gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, mix(theme::ACCENT_START))
        .add_stop(1.0, mix(theme::ACCENT_END))
        .into()
}

/// The one glass style: panel colour, 1 px border, soft shadow.
fn glass(radius: f32) -> container::Style {
    let palette = theme::current();
    container::Style {
        background: Some(palette.panel.into()),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

// ponytail: the allow goes (with the glob re-export's) once ui/mod.rs opens Settings and
// Onboarding and every form widget below is used.
#[allow(dead_code)]
mod forms {
    use iced::widget::{
        Button, Container, PickList, Text, TextInput, button, column, container, pick_list, row,
        space, text, text_input, tooltip,
    };
    use iced::{Border, Color, Element, Length, Shadow, Vector};

    use super::{CONTROL, TEXT_BUTTON_WIDTH, accent, theme, with_tip};

    // ---- Forms: Settings and Onboarding. Every form control is CONTROL tall and
    // FORM_WIDTH wide (segmented, text field, dropdown, shortcut field), so rows align. ----

    pub const FORM_WIDTH: f32 = 320.0;
    const FIELD_RADIUS: f32 = 14.0;
    /// One line of text inside a field; padding (CONTROL - this) / 2 makes the field CONTROL tall.
    const FIELD_LINE: f32 = 18.0;
    const FIELD_PAD: f32 = (CONTROL - FIELD_LINE) / 2.0;

    /// Screen title text.
    pub fn title<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
        text(label).size(22).color(theme::current().text)
    }

    /// Paragraph text under a title.
    pub fn body<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
        text(label).size(14).color(theme::current().muted)
    }

    /// Muted small text (captions, hints).
    pub fn caption<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
        text(label).size(12).color(theme::current().muted)
    }

    /// Error text, same size as a caption.
    pub fn error_text<'a>(label: impl text::IntoFragment<'a>) -> Text<'a> {
        text(label).size(12).color(theme::current().danger)
    }

    /// The window background of Settings / Onboarding.
    pub fn window<'a, Message: 'a>(
        content: impl Into<Element<'a, Message>>,
    ) -> Container<'a, Message> {
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(theme::current().window.into()),
                text_color: Some(theme::current().text),
                ..container::Style::default()
            })
    }

    /// 1 px horizontal rule.
    pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
        container(space::vertical())
            .width(Length::Fill)
            .height(1)
            .style(|_| container::Style {
                background: Some(theme::current().border.into()),
                ..container::Style::default()
            })
            .into()
    }

    /// One key of a shortcut, e.g. "Ctrl".
    pub fn key_cap<'a, Message: 'a>(label: String) -> Container<'a, Message> {
        let palette = theme::current();
        container(text(label).size(12).color(palette.text))
            .padding([3, 8])
            .style(|_| container::Style {
                background: Some(theme::current().keycap.into()),
                border: Border {
                    color: theme::current().border,
                    width: 1.0,
                    radius: 7.0.into(),
                },
                ..container::Style::default()
            })
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FieldState {
        Idle,
        /// Waiting for the user to press the keys.
        Recording,
        Invalid,
    }

    /// A shortcut: fixed width, one `key_cap` per key, right-aligned. Empty `keys` =
    /// "Not set"; `Recording` shows `recording_hint` instead of the keys.
    pub fn shortcut_field<'a, Message: Clone + 'a>(
        keys: Vec<String>,
        state: FieldState,
        recording_hint: &'a str,
        on_press: Message,
    ) -> Button<'a, Message> {
        let palette = theme::current();
        let inner: Element<'a, Message> = match state {
            FieldState::Recording => text(recording_hint).size(13).color(palette.muted).into(),
            _ if keys.is_empty() => text("Not set").size(13).color(palette.muted).into(),
            _ => row(keys.into_iter().map(|k| key_cap(k).into()))
                .spacing(4)
                .into(),
        };
        button(
            row![space::horizontal(), inner]
                .height(Length::Fill)
                .align_y(iced::Center),
        )
        .width(FORM_WIDTH)
        .height(CONTROL)
        .padding([0, 12])
        .on_press(on_press)
        .style(move |_, status| field_button(state, status))
    }

    fn field_button(state: FieldState, status: button::Status) -> button::Style {
        let palette = theme::current();
        let soft = state == FieldState::Recording || status == button::Status::Hovered;
        button::Style {
            background: Some(
                if soft {
                    palette.field_focus
                } else {
                    palette.field
                }
                .into(),
            ),
            text_color: palette.text,
            border: Border {
                color: if state == FieldState::Invalid {
                    palette.danger
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: FIELD_RADIUS.into(),
            },
            ..button::Style::default()
        }
    }

    /// Pick one of a few options; the selected one is the accent gradient. Each option
    /// is `(value, label, tooltip)`; the whole control is `FORM_WIDTH` wide.
    pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
        options: &[(T, &'a str, &'a str)],
        selected: T,
        on_select: impl Fn(T) -> Message,
    ) -> Element<'a, Message> {
        let segments = options.iter().map(|&(value, label, hint)| {
            let on = value == selected;
            let palette = theme::current();
            let b = button(text(label).size(14).center())
                .width(Length::Fill)
                .height(CONTROL - 8.0)
                .on_press(on_select(value))
                .style(move |_, status| button::Style {
                    background: match (on, status) {
                        (true, _) => Some(accent(status)),
                        (false, button::Status::Hovered) => Some(palette.hover.into()),
                        (false, button::Status::Pressed) => Some(palette.pressed.into()),
                        _ => None,
                    },
                    text_color: if on { theme::ON_ACCENT } else { palette.text },
                    border: Border::default().rounded(FIELD_RADIUS - 4.0),
                    ..button::Style::default()
                });
            tip(b, hint, tooltip::Position::Top)
        });
        container(row(segments).spacing(2))
            .padding(4)
            .width(FORM_WIDTH)
            .height(CONTROL)
            .style(|_| container::Style {
                background: Some(theme::current().field.into()),
                border: Border::default().rounded(FIELD_RADIUS),
                ..container::Style::default()
            })
            .into()
    }

    /// Single-line text field. Focus (and hover) = a softer fill, never a ring.
    pub fn text_field<'a, Message: Clone + 'a>(
        placeholder: &str,
        value: &str,
        invalid: bool,
        on_input: impl Fn(String) -> Message + 'a,
    ) -> TextInput<'a, Message> {
        text_input(placeholder, value)
            .on_input(on_input)
            .size(14)
            .line_height(text::LineHeight::Absolute(FIELD_LINE.into()))
            .padding([FIELD_PAD, 14.0])
            .width(FORM_WIDTH)
            .style(move |_, status| {
                let palette = theme::current();
                let soft = matches!(
                    status,
                    text_input::Status::Hovered | text_input::Status::Focused { .. }
                );
                text_input::Style {
                    background: if soft {
                        palette.field_focus
                    } else {
                        palette.field
                    }
                    .into(),
                    border: Border {
                        color: if invalid {
                            palette.danger
                        } else {
                            Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: FIELD_RADIUS.into(),
                    },
                    icon: palette.muted,
                    placeholder: palette.muted,
                    value: palette.text,
                    selection: Color {
                        a: 0.35,
                        ..theme::ACCENT_START
                    },
                }
            })
    }

    /// Dropdown list, same size and fill as the other form controls.
    pub fn dropdown<'a, Message: Clone + 'a, T: ToString + PartialEq + Clone + 'a>(
        options: &'a [T],
        selected: Option<T>,
        placeholder: &str,
        on_select: impl Fn(T) -> Message + 'a,
    ) -> PickList<'a, T, &'a [T], T, Message> {
        pick_list(options, selected, on_select)
            .placeholder(placeholder)
            .text_size(14)
            .text_line_height(text::LineHeight::Absolute(FIELD_LINE.into()))
            .padding([FIELD_PAD, 14.0])
            .width(FORM_WIDTH)
            .style(|_, status| {
                let palette = theme::current();
                let soft = !matches!(status, pick_list::Status::Active);
                pick_list::Style {
                    text_color: palette.text,
                    placeholder_color: palette.muted,
                    handle_color: palette.muted,
                    background: if soft {
                        palette.field_focus
                    } else {
                        palette.field
                    }
                    .into(),
                    border: Border::default().rounded(FIELD_RADIUS),
                }
            })
            .menu_style(|_| {
                let palette = theme::current();
                iced::overlay::menu::Style {
                    background: opaque(palette.panel, palette.window).into(),
                    border: Border {
                        color: palette.border,
                        width: 1.0,
                        radius: FIELD_RADIUS.into(),
                    },
                    text_color: palette.text,
                    selected_text_color: theme::ON_ACCENT,
                    selected_background: accent(button::Status::Active),
                    shadow: Shadow {
                        color: palette.shadow,
                        offset: Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                }
            })
    }

    /// `panel` laid over `under`, as one opaque colour (a menu can't show what is behind it).
    fn opaque(panel: Color, under: Color) -> Color {
        let mix = |p: f32, u: f32| p * panel.a + u * (1.0 - panel.a);
        Color::from_rgb(
            mix(panel.r, under.r),
            mix(panel.g, under.g),
            mix(panel.b, under.b),
        )
    }

    /// Text-only button, as big as `text_button`. `on_press: None` = disabled (quiet, dimmed).
    pub fn label_button<'a, Message: Clone + 'a>(
        label: &'a str,
        primary: bool,
        on_press: Option<Message>,
    ) -> Button<'a, Message> {
        let palette = theme::current();
        let live = on_press.is_some();
        let accented = primary && live;
        let fg = match (accented, live) {
            (true, _) => theme::ON_ACCENT,
            (false, true) => palette.text,
            (false, false) => palette.muted,
        };
        button(text(label).size(14).center())
            .width(TEXT_BUTTON_WIDTH)
            .height(CONTROL)
            .on_press_maybe(on_press)
            .style(move |_, status| button::Style {
                background: match (accented, status) {
                    (true, _) => Some(accent(status)),
                    (false, button::Status::Hovered) => Some(palette.field_focus.into()),
                    (false, button::Status::Pressed) => Some(palette.pressed.into()),
                    (false, _) => Some(palette.field.into()),
                },
                text_color: fg,
                border: Border::default().rounded(FIELD_RADIUS),
                ..button::Style::default()
            })
    }

    /// Sidebar entry (Settings sections).
    pub fn nav_item<'a, Message: Clone + 'a>(
        label: &'a str,
        selected: bool,
        on_press: Message,
    ) -> Button<'a, Message> {
        button(
            text(label)
                .size(14)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Center),
        )
        .width(Length::Fill)
        .height(CONTROL)
        .padding([0, 14])
        .on_press(on_press)
        .style(move |_, status| {
            let palette = theme::current();
            button::Style {
                background: match (selected, status) {
                    (true, _) => Some(palette.field_focus.into()),
                    (false, button::Status::Hovered) => Some(palette.hover.into()),
                    _ => None,
                },
                text_color: palette.text,
                border: Border::default().rounded(FIELD_RADIUS),
                ..button::Style::default()
            }
        })
    }

    /// One settings row: label (+ caption) on the left, the control on the right,
    /// an optional error under the control.
    pub fn form_row<'a, Message: 'a>(
        label: &'a str,
        hint: &'a str,
        control: impl Into<Element<'a, Message>>,
        error: Option<String>,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        let mut right = column![control.into()].spacing(6).width(FORM_WIDTH);
        if let Some(e) = error {
            right = right.push(error_text(e));
        }
        let mut left = column![text(label).size(14).color(palette.text)].spacing(2);
        if !hint.is_empty() {
            left = left.push(caption(hint));
        }
        row![left.width(Length::Fill), right]
            .spacing(24)
            .padding([12, 0])
            .into()
    }

    /// `with_tip` that also records the label under test, so a test can check that
    /// every control of a screen has its own (the accessible name; iced has no a11y tree).
    pub fn tip<'a, Message: 'a>(
        content: impl Into<Element<'a, Message>>,
        label: &'a str,
        side: tooltip::Position,
    ) -> Element<'a, Message> {
        #[cfg(test)]
        super::TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        with_tip(content, label, side).into()
    }

    /// 1 px vertical rule.
    pub fn vertical_divider<'a, Message: 'a>() -> Element<'a, Message> {
        container(space::horizontal())
            .width(1)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(theme::current().border.into()),
                ..container::Style::default()
            })
            .into()
    }
}
#[allow(unused_imports)]
pub use forms::*;

#[cfg(test)]
thread_local! {
    pub static TIPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

// ponytail: until ui/mod.rs declares `mod settings; mod onboarding;`, tests compile the
// two screens from here (they only use `crate::ui::..` paths and `super::settings`, so
// both layouts work). Delete these four lines when mod.rs declares them.
#[cfg(test)]
#[path = "onboarding.rs"]
pub mod onboarding;
#[cfg(test)]
#[path = "settings.rs"]
pub mod settings;

/// Headless renders for tests: an `Element` to RGBA pixels, and to a PNG when
/// `OPENCIRCLE_RENDER_DIR` is set.
#[cfg(test)]
pub mod shot {
    use iced::advanced::{graphics::Viewport, mouse, renderer};
    use iced::{Color, Element, Font, Pixels, Rectangle, Size, Theme};
    use iced_runtime::user_interface::{Cache, UserInterface};

    pub fn render<Message>(view: Element<'_, Message>, width: u32, height: u32) -> Vec<u8> {
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(super::ICON_FONT_BYTES.into());
        let mut renderer = iced::Renderer::new(Font::DEFAULT, Pixels(16.0));
        let size = Size::new(width as f32, height as f32);
        let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
        let style = renderer::Style {
            text_color: Color::WHITE,
        };
        ui.draw(
            &mut renderer,
            &Theme::Dark,
            &style,
            mouse::Cursor::Unavailable,
        );
        let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
        renderer.draw(
            &mut pixmap.as_mut(),
            &mut tiny_skia::Mask::new(width, height).unwrap(),
            &Viewport::with_physical_size(Size::new(width, height), 1.0),
            &[Rectangle::with_size(size)],
            super::theme::current().window,
        );
        // iced draws B, G, R, A.
        pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    }

    /// Every text a view holds: labels, and what each text field shows or has typed.
    pub fn texts<Message>(view: Element<'_, Message>) -> Vec<String> {
        use iced::advanced::widget::{Id, Operation, operation::TextInput};
        struct Collect(Vec<String>);
        impl Operation for Collect {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
                self.0.push(text.to_string());
            }
            fn text_input(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn TextInput) {
                self.0.push(state.text().to_string());
            }
        }
        let mut renderer = iced::Renderer::new(Font::DEFAULT, Pixels(16.0));
        let size = Size::new(900.0, 700.0);
        let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
        let mut collect = Collect(Vec::new());
        ui.operate(&renderer, &mut collect);
        collect.0
    }

    /// Render and, when `OPENCIRCLE_RENDER_DIR` is set, save `<dir>/<name>.png`.
    /// Returns the pixels either way.
    pub fn save<Message>(name: &str, view: Element<'_, Message>, size: Size) -> Vec<u8> {
        let (w, h) = (size.width as u32, size.height as u32);
        let rgba = render(view, w, h);
        if let Some(dir) = std::env::var_os("OPENCIRCLE_RENDER_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            crate::actions::write_png(&dir.join(format!("{name}.png")), w, h, &rgba).unwrap();
        }
        rgba
    }
}
