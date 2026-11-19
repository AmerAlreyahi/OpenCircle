//! OpenCircle Studio's window (`features::snip::screens::studio`): one at a time;
//! opening again brings it forward and opens the picture in it. What its
//! state asks for (a file, the clipboard, Save / Copy, the config) runs here.
use iced::{Task, keyboard, window};

use super::{Kind, Msg, TOAST_MS, Windows, chrome};
use crate::app::background::background;
use crate::features::circle::actions::{Action, Run, copy, save};
use crate::features::snip::screens::studio::{self, Outcome, Picture, Studio};
use crate::shared::picture::Crop;

impl Windows {
    /// Opens the Studio (empty, or on `picture`), or brings the open one forward
    /// and opens `picture` there (unsaved drawings ask first).
    pub fn open_studio(&mut self, picture: Option<Picture>) -> Task<Msg> {
        if let Some((id, s)) = &mut self.studio {
            let id = *id;
            if let Some(picture) = picture {
                s.open(picture);
            }
            return window::minimize(id, false).chain(window::gain_focus(id));
        }
        crate::log!("[opencircle] opening OpenCircle Studio");
        #[cfg(target_os = "linux")]
        crate::shared::platform::sys::shell::leave_overview();
        super::activate_next();
        let (id, open) = super::normal(Kind::Studio);
        let mut s = Studio::new(crate::shared::config::current().snip.studio);
        if let Some(picture) = picture {
            s.open(picture);
        }
        self.studio = Some((id, s));
        self.chrome.insert(id, Default::default());
        let to = |m: studio::Msg| Msg::Studio(m);
        let scale = window::scale_factor(id).map(move |f| to(studio::Msg::Scale(f)));
        let size = window::size(id).map(move |s| to(studio::Msg::Resized(s)));
        let recent = background(|tx| {
            let _ = tx.unbounded_send(crate::shared::history::recent::load());
        })
        .map(move |r| to(studio::Msg::Recent(r)));
        Task::batch([
            open.discard().chain(window::gain_focus(id)).chain(scale),
            size,
            recent,
        ])
    }

    /// The tray's Studio or `opencircle studio [file]`: the Studio, then the file read into it.
    pub fn open_studio_file(&mut self, file: Option<std::path::PathBuf>) -> Task<Msg> {
        let open = self.open_studio(None);
        let Some(path) = file else {
            return open;
        };
        let read = background(move |tx| {
            let _ = tx.unbounded_send(read_picture(&path));
        })
        .map(Msg::StudioRead);
        Task::batch([open, read])
    }

    /// Unsaved drawings in the Studio (an update or a quit must not drop them).
    pub fn studio_dirty(&self) -> bool {
        self.studio.as_ref().is_some_and(|(_, s)| s.dirty())
    }

    /// The app is quitting: with unsaved drawings the Studio comes forward and
    /// asks (the same question as its close); `None` = nothing to ask, go.
    pub fn quit_studio(&mut self) -> Option<Task<Msg>> {
        let (id, s) = self.studio.as_mut().filter(|(_, s)| s.dirty())?;
        let id = *id;
        s.quit_requested();
        Some(window::minimize(id, false).chain(window::gain_focus(id)))
    }

    pub(super) fn studio_window(&self) -> Option<window::Id> {
        self.studio.as_ref().map(|(id, _)| *id)
    }

    pub(super) fn on_studio(&mut self, msg: studio::Msg) -> Task<Msg> {
        let Some((_, s)) = &mut self.studio else {
            return Task::none();
        };
        let outcome = s.update(msg);
        self.studio_outcome(outcome)
    }

    /// The Studio asked to close (its x, Alt+F4, the taskbar): unsaved drawings ask first.
    pub(super) fn close_studio(&mut self) -> Task<Msg> {
        let Some((_, s)) = &mut self.studio else {
            return Task::none();
        };
        let outcome = s.close_requested();
        self.studio_outcome(outcome)
    }

    /// A picture read for the Studio (a file, a recent capture).
    pub(super) fn studio_read(&mut self, read: Result<Crop, String>) -> Task<Msg> {
        match (read, &mut self.studio) {
            (Ok(crop), Some((_, s))) => {
                s.open(Picture::Crop(crop));
                Task::none()
            }
            (Ok(_), None) => Task::none(),
            (Err(e), _) => {
                crate::log!("[opencircle] studio: {e}");
                self.toast(crate::t!(st_unreadable), false, None, TOAST_MS)
            }
        }
    }

    /// The clipboard was read for Ctrl+V: a picture opens; else the editor
    /// pastes its text at `at`; else the pill says there is none.
    pub(super) fn studio_pasted(
        &mut self,
        image: Option<Crop>,
        at: Option<iced::Point>,
    ) -> Task<Msg> {
        match (image, &mut self.studio) {
            (Some(crop), _) => self.studio_read(Ok(crop)),
            (None, Some((_, s))) if s.editor_open() => {
                let next = s.paste_text(at);
                self.studio_outcome(next)
            }
            (None, _) => self.toast(crate::t!(st_no_picture), false, None, TOAST_MS),
        }
    }

    /// A Save for the Studio landed: on success what is shown is saved, and
    /// (`then`) what waited on the question goes on.
    pub(super) fn studio_saved(&mut self, saved: Result<String, String>, then: bool) -> Task<Msg> {
        let ok = saved.is_ok();
        let text = saved.unwrap_or_else(|e| e);
        let toast = self.toast(&text, ok, None, TOAST_MS);
        let Some((_, s)) = &mut self.studio else {
            return toast;
        };
        if !ok {
            return toast;
        }
        s.saved();
        let next = match then {
            true => s.proceed(),
            false => Outcome::None,
        };
        Task::batch([toast, self.studio_outcome(next)])
    }

    fn studio_outcome(&mut self, outcome: Outcome) -> Task<Msg> {
        let Some(id) = self.studio_window() else {
            return Task::none();
        };
        match outcome {
            Outcome::None => Task::none(),
            Outcome::Close => window::close(id),
            // answered: the quit goes on (the app asks the Studio again, now clean)
            Outcome::Quit => Task::done(Msg::Run(crate::Trigger::Quit)),
            Outcome::Caption(c) => self.on_chrome(id, chrome::Event::from(c)),
            Outcome::PickFile => background(|tx| {
                let picked = rfd::FileDialog::new()
                    .set_title(crate::t!(st_open_title))
                    .add_filter(
                        crate::t!(st_pictures),
                        &["png", "jpg", "jpeg", "webp", "bmp", "gif"],
                    )
                    .pick_file();
                if let Some(path) = picked {
                    let _ = tx.unbounded_send(path);
                }
            })
            .map(|path| Msg::Studio(studio::Msg::Dropped(path))),
            Outcome::Read(path) => background(move |tx| {
                let _ = tx.unbounded_send(read_picture(&path));
            })
            .map(Msg::StudioRead),
            Outcome::Paste(at) => background(move |tx| {
                let _ = tx.unbounded_send(copy::read_image());
            })
            .map(move |image| Msg::StudioPasted(image, at)),
            Outcome::Copy(crop) => background(move |tx| {
                let _ = tx.unbounded_send(run(&copy::ACTION, crop));
            })
            .map(Msg::StudioToast),
            Outcome::Save(crop) => studio_save(crop, false),
            Outcome::SaveThen(crop) => studio_save(crop, true),
            Outcome::Pin(crop) => self.pin(crop, None, 1.0),
            Outcome::ScrollTo(to) => iced::widget::operation::scroll_to(studio::CANVAS, to),
            Outcome::Panels(panels) => {
                std::thread::spawn(move || {
                    if let Err(e) = crate::shared::config::update(|c| c.snip.studio = panels) {
                        crate::log!("[opencircle] config: {e}");
                    }
                });
                Task::none()
            }
        }
    }
}

/// Save off the UI thread (encoding a big picture takes a while): `Msg::StudioSaved`.
fn studio_save(crop: Crop, then: bool) -> Task<Msg> {
    background(move |tx| {
        let _ = tx.unbounded_send(run(&save::ACTION, crop));
    })
    .map(move |saved| Msg::StudioSaved(saved, then))
}

/// A finishing action of the registry (Copy, Save): its pill's text or error.
fn run(action: &Action, crop: Crop) -> Result<String, String> {
    match action.run {
        Run::Finish(run) => run(crop),
        _ => Err(String::new()),
    }
}

/// Blocking: a picture file, any format the image crate reads here.
fn read_picture(path: &std::path::Path) -> Result<Crop, String> {
    let picture = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .into_rgba8();
    Ok(Crop {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    })
}

/// One key the editor left alone: Esc answers the unsaved question with Cancel (a no-op
/// when none is open); the Ctrl keys are the Studio's own.
fn key_msg(
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<studio::Msg> {
    use keyboard::key::{Code, Named, Physical};
    if *key == keyboard::Key::Named(Named::Escape) && modifiers.is_empty() {
        return Some(studio::Msg::Cancel);
    }
    if !modifiers.command() {
        return None;
    }
    Some(match (physical_key, key.to_latin(physical_key)) {
        (Physical::Code(Code::BracketLeft), _) => studio::Msg::Library,
        (Physical::Code(Code::BracketRight), _) => studio::Msg::Panel,
        (Physical::Code(Code::Backslash), _) => studio::Msg::Both,
        (Physical::Code(Code::Equal | Code::NumpadAdd), _) => studio::Msg::ZoomIn,
        (Physical::Code(Code::Minus | Code::NumpadSubtract), _) => studio::Msg::ZoomOut,
        (Physical::Code(Code::Digit0 | Code::Numpad0), _) => studio::Msg::Fit,
        (Physical::Code(Code::Digit1 | Code::Numpad1), _) => studio::Msg::Actual,
        (_, Some('o')) => studio::Msg::Open,
        (_, Some('v')) => studio::Msg::Paste,
        _ => return None,
    })
}

/// The Studio's own keys (those its editor's canvas didn't take): Ctrl+O, Ctrl+V,
/// the panels (Ctrl+[ / Ctrl+] / Ctrl+\, by physical key), the zoom
/// (Ctrl+= / Ctrl+- / Ctrl+0 / Ctrl+1); and files dragged over or dropped on it.
pub(super) fn studio_events() -> iced::Subscription<Msg> {
    iced::event::listen_with(|event, status, id| {
        let m = match event {
            iced::Event::Window(window::Event::FileHovered(_)) => studio::Msg::Hovered(true),
            iced::Event::Window(window::Event::FilesHoveredLeft) => studio::Msg::Hovered(false),
            iced::Event::Window(window::Event::FileDropped(path)) => studio::Msg::Dropped(path),
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                studio::Msg::Ctrl(m.command())
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) if status == iced::event::Status::Ignored => key_msg(&key, physical_key, modifiers)?,
            _ => return None,
        };
        Some(Msg::StudioEvent(id, m))
    })
}
#[cfg(test)]
mod key_tests {
    use super::*;
    use keyboard::key::{Code, Named, Physical};

    /// Esc on the unsaved question = Cancel; Ctrl+Esc and other keys are not.
    #[test]
    fn esc_is_cancel() {
        let esc = keyboard::Key::Named(Named::Escape);
        let p = Physical::Code(Code::Escape);
        assert!(matches!(
            key_msg(&esc, p, keyboard::Modifiers::empty()),
            Some(studio::Msg::Cancel)
        ));
        assert!(key_msg(&esc, p, keyboard::Modifiers::CTRL).is_none());
        let f = keyboard::Key::Character("f".into());
        assert!(key_msg(&f, Physical::Code(Code::KeyF), keyboard::Modifiers::empty()).is_none());
    }
}
