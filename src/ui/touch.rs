//! A finger or pen as the left mouse button (#153). winit sends touch and Windows pen
//! (WM_POINTER) as `touch::Event`, not mouse events and not a cursor, so each canvas that
//! circles, selects or draws runs its events through [`as_mouse`] first and then handles
//! the result exactly as it handles the mouse. Only the first finger counts.
use iced::mouse::{self, Button};
use iced::touch::{self, Finger};
use iced::{Event, Point};

/// The mouse event a touch stands for and where the pointer is. `first` is the finger held
/// (kept by the caller); any other finger is ignored. Not a touch event: `None`.
pub(crate) fn as_mouse(event: &Event, first: &mut Option<Finger>) -> Option<(mouse::Event, Point)> {
    let Event::Touch(touch) = event else {
        return None;
    };
    match *touch {
        touch::Event::FingerPressed { id, position } if first.is_none() => {
            *first = Some(id);
            Some((mouse::Event::ButtonPressed(Button::Left), position))
        }
        touch::Event::FingerMoved { id, position } if *first == Some(id) => {
            Some((mouse::Event::CursorMoved { position }, position))
        }
        touch::Event::FingerLifted { id, position } | touch::Event::FingerLost { id, position }
            if *first == Some(id) =>
        {
            *first = None;
            Some((mouse::Event::ButtonReleased(Button::Left), position))
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
            Some((mouse::Event::ButtonPressed(Button::Left), at(1.0, 2.0)))
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
            as_mouse(&ev(moved), &mut first).map(|m| m.0),
            Some(mouse::Event::CursorMoved {
                position: at(3.0, 4.0)
            })
        );
        let up = touch::Event::FingerLifted {
            id: a,
            position: at(3.0, 4.0),
        };
        assert_eq!(
            as_mouse(&ev(up), &mut first).map(|m| m.0),
            Some(mouse::Event::ButtonReleased(Button::Left))
        );
        assert!(first.is_none());
        // a mouse event is not a touch
        let m = Event::Mouse(mouse::Event::CursorMoved {
            position: at(0.0, 0.0),
        });
        assert!(as_mouse(&m, &mut first).is_none());
    }
}
