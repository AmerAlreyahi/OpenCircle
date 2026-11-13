//! A finger or pen as the left mouse button (#153). winit sends touch and Windows pen
//! (WM_POINTER) as `touch::Event`, not mouse events and not a cursor, so each canvas that
//! circles, selects or draws runs its events through [`as_mouse`] first and then handles
//! the result exactly as it handles the mouse. Only the first finger counts.
use iced::mouse::{self, Button};
use iced::touch::{self, Finger};
use iced::{Event, Point};

/// What a touch stands for: a mouse event at a place, or a press the system cancelled.
#[derive(Debug, PartialEq)]
pub(crate) enum Touched {
    Mouse(mouse::Event, Point),
    /// The OS cancelled the contact (palm rejection, a system gesture): not a lift, so
    /// the press ends without completing what it was drawing.
    Cancel,
}

/// The mouse event a touch stands for and where the pointer is. `first` is the finger held
/// (kept by the caller); any other finger is ignored. Not a touch event: `None`.
pub(crate) fn as_mouse(event: &Event, first: &mut Option<Finger>) -> Option<Touched> {
    let Event::Touch(touch) = event else {
        return None;
    };
    match *touch {
        touch::Event::FingerPressed { id, position } if first.is_none() => {
            *first = Some(id);
            Some(Touched::Mouse(
                mouse::Event::ButtonPressed(Button::Left),
                position,
            ))
        }
        touch::Event::FingerMoved { id, position } if *first == Some(id) => Some(Touched::Mouse(
            mouse::Event::CursorMoved { position },
            position,
        )),
        touch::Event::FingerLifted { id, position } if *first == Some(id) => {
            *first = None;
            Some(Touched::Mouse(
                mouse::Event::ButtonReleased(Button::Left),
                position,
            ))
        }
        touch::Event::FingerLost { id, .. } if *first == Some(id) => {
            *first = None;
            Some(Touched::Cancel)
        }
        _ => None,
    }
}

/// The pointer's place for subscriptions that follow it anywhere (a dragged bar or card):
/// a finger moving, and a finger let go.
pub(crate) enum Drag {
    Moved(Point),
    Released,
}

pub(crate) fn drag(event: &Event) -> Option<Drag> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position })
        | Event::Touch(touch::Event::FingerMoved { position, .. }) => Some(Drag::Moved(*position)),
        Event::Mouse(mouse::Event::ButtonReleased(Button::Left))
        | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }) => {
            Some(Drag::Released)
        }
        _ => None,
    }
}

/// The pointer came into the window that gets this event: the mouse entering it, or a
/// finger or pen put down on it (no cursor comes in with a touch). The edge glow follows
/// the last pointer input onto its screen (owner, 2026-11-10, #155).
pub(crate) fn came_in(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::CursorEntered)
            | Event::Touch(touch::Event::FingerPressed { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn only_the_first_finger_acts_as_the_left_button() {
        let (a, b) = (Finger(1), Finger(2));
        let mut first = None;
        let ev = |e| Event::Touch(e);
        let down = as_mouse(
            &ev(touch::Event::FingerPressed {
                id: a,
                position: at(1.0, 2.0),
            }),
            &mut first,
        );
        assert_eq!(
            down,
            Some(Touched::Mouse(
                mouse::Event::ButtonPressed(Button::Left),
                at(1.0, 2.0)
            ))
        );
        // a second finger changes nothing
        let other = touch::Event::FingerPressed {
            id: b,
            position: at(9.0, 9.0),
        };
        assert!(as_mouse(&ev(other), &mut first).is_none());
        let other = touch::Event::FingerMoved {
            id: b,
            position: at(9.0, 9.0),
        };
        assert!(as_mouse(&ev(other), &mut first).is_none());
        let other = touch::Event::FingerLifted {
            id: b,
            position: at(9.0, 9.0),
        };
        assert!(as_mouse(&ev(other), &mut first).is_none() && first == Some(a));
        let moved = touch::Event::FingerMoved {
            id: a,
            position: at(3.0, 4.0),
        };
        assert_eq!(
            as_mouse(&ev(moved), &mut first),
            Some(Touched::Mouse(
                mouse::Event::CursorMoved {
                    position: at(3.0, 4.0)
                },
                at(3.0, 4.0)
            ))
        );
        let up = touch::Event::FingerLifted {
            id: a,
            position: at(3.0, 4.0),
        };
        assert_eq!(
            as_mouse(&ev(up), &mut first),
            Some(Touched::Mouse(
                mouse::Event::ButtonReleased(Button::Left),
                at(3.0, 4.0)
            ))
        );
        assert!(first.is_none());
        // a lost finger cancels: it is not a release
        let down = touch::Event::FingerPressed {
            id: a,
            position: at(1.0, 1.0),
        };
        as_mouse(&ev(down), &mut first);
        let lost = touch::Event::FingerLost {
            id: a,
            position: at(2.0, 2.0),
        };
        assert_eq!(as_mouse(&ev(lost), &mut first), Some(Touched::Cancel));
        assert!(first.is_none());
        // a mouse event is not a touch
        let m = Event::Mouse(mouse::Event::CursorMoved {
            position: at(0.0, 0.0),
        });
        assert!(as_mouse(&m, &mut first).is_none());
    }

    /// #155: a tap or pen press on a screen is pointer input there, like the mouse
    /// coming in; moving on it is not a new arrival.
    #[test]
    fn a_touch_or_the_mouse_comes_in() {
        let pressed = touch::Event::FingerPressed {
            id: Finger(1),
            position: at(5.0, 5.0),
        };
        assert!(came_in(&Event::Touch(pressed)));
        assert!(came_in(&Event::Mouse(mouse::Event::CursorEntered)));
        let moved = touch::Event::FingerMoved {
            id: Finger(1),
            position: at(6.0, 6.0),
        };
        assert!(!came_in(&Event::Touch(moved)));
        let m = mouse::Event::CursorMoved {
            position: at(0.0, 0.0),
        };
        assert!(!came_in(&Event::Mouse(m)));
    }
}
