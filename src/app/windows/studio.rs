//! OpenCircle Studio's window (`features::snip::screens::studio`): one at a time;
//! opening again brings it forward and opens the picture in it. What its
//! state asks for (a file, the clipboard, Save / Copy, the config) runs here.
use iced::{Task, keyboard, window};

use super::{Kind, Msg, TOAST_MS, Windows, chrome};
use crate::app::background::background;
use crate::features::circle::actions::{Action, Run, copy, save};
use crate::features::snip::export;
use crate::features::snip::screens::steps::Step;
use crate::features::snip::screens::studio::{self, Outcome, Picture, Studio};
use crate::shared::app_language::{S, fmt};
use crate::shared::picture::Crop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a walkthrough's encoder tells the window.
#[derive(Debug, Clone)]
pub enum Walked {
    Progress(usize, usize),
    /// Done: the pill's text; `None`: cancelled.
    End(Option<Result<String, String>>),
}

impl Windows {
    /// A frame encoded, or the end: the sheet's progress, then the pill.
    pub(super) fn on_walked(&mut self, walked: Walked) -> Task<Msg> {
        match walked {
            Walked::Progress(done, total) => self.on_studio(studio::Msg::WalkProgress(done, total)),
            Walked::End(done) => {
                self.walk_cancel = None;
                let closes = matches!(done, Some(Ok(_)));
                let ended = self.on_studio(studio::Msg::WalkEnded(closes));
                match done {
                    Some(done) => Task::batch([ended, self.toast_of(done)]),
                    None => ended,
                }
            }
        }
    }

    fn toast_of(&mut self, done: Result<String, String>) -> Task<Msg> {
        let ok = done.is_ok();
        let text = done.unwrap_or_else(|e| e);
        self.toast(&text, ok, None, TOAST_MS)
    }

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
        s.set_browser(crate::features::ask_ai::app::ai_chat::hands_off());
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

    /// The Studio's window (for asking its monitor).
    pub fn studio_id(&self) -> Option<window::Id> {
        self.studio_window()
    }

    pub fn studio(&self) -> Option<&Studio> {
        self.studio.as_ref().map(|(_, s)| s)
    }

    pub fn studio_mut(&mut self) -> Option<&mut Studio> {
        self.studio.as_mut().map(|(_, s)| s)
    }

    /// The Studio has the keyboard and a picture: the quick-action keys act on it.
    pub fn studio_takes_quick_keys(&self) -> bool {
        self.studio().is_some_and(Studio::takes_quick_keys)
    }

    /// Copy text's words, read: on the clipboard off the UI thread, then the pill.
    pub(super) fn studio_copy_text(
        &mut self,
        epoch: u32,
        read: Result<String, String>,
    ) -> Task<Msg> {
        // (an answer for a closed or replaced picture: no clipboard, no pill)
        match self.studio_mut() {
            Some(s) if s.epoch() == epoch => s.set_busy(None),
            _ => return Task::none(),
        }
        match read {
            Ok(text) => background(move |tx| {
                let _ = tx.unbounded_send(copy::text(text).map(|()| copy::COPIED_PILL.to_string()));
            })
            .map(Msg::StudioToast),
            Err(why) => self.say(&why, false),
        }
    }

    /// A pill with `text` (a picture action's answer).
    pub fn say(&mut self, text: &str, ok: bool) -> Task<Msg> {
        self.toast(text, ok, None, TOAST_MS)
    }

    pub(super) fn studio_window(&self) -> Option<window::Id> {
        self.studio.as_ref().map(|(id, _)| *id)
    }

    pub(super) fn on_studio(&mut self, msg: studio::Msg) -> Task<Msg> {
        let Some((_, s)) = &mut self.studio else {
            return Task::none();
        };
        s.set_browser(crate::features::ask_ai::app::ai_chat::hands_off());
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

    pub(super) fn studio_outcome(&mut self, outcome: Outcome) -> Task<Msg> {
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
            Outcome::Act(row) => Task::done(Msg::StudioAct(row)),
            Outcome::ReadLines => Task::done(Msg::StudioEditText),
            Outcome::FocusField => iced::widget::operation::focus(studio::FIELD),
            Outcome::CutOut => Task::done(Msg::StudioCutOut),
            Outcome::Say(text) => self.say(&text, false),
            Outcome::Download => Task::done(Msg::StudioDownload),
            Outcome::Dismissed => Task::done(Msg::StudioDismiss),
            Outcome::Provider => Task::batch([
                Task::done(Msg::StudioDismiss),
                self.open(super::Kind::Settings),
            ]),
            Outcome::Steps(m) => self.on_steps(m),
            Outcome::OpenStep(i) => {
                let Some(step) = self
                    .steps
                    .as_ref()
                    .and_then(|(_, s)| s.steps().get(i).cloned())
                else {
                    return Task::none();
                };
                background(move |tx| {
                    let _ = tx
                        .unbounded_send(step.crop().ok_or_else(|| crate::t!(st_unreadable).into()));
                })
                .map(Msg::StudioRead)
            }
            Outcome::ExportSteps(how) => {
                let steps: Vec<_> =
                    (self.steps.as_ref()).map_or(Vec::new(), |(_, s)| s.steps().to_vec());
                if steps.is_empty() {
                    return Task::none();
                }
                background(move |tx| {
                    if let Some(done) = export_steps(&steps, how) {
                        let _ = tx.unbounded_send(done);
                    }
                })
                .map(Msg::StudioToast)
            }
            Outcome::ExportWalkthrough(how, frames) => {
                // one encode at a time: a second one never takes the first's stop flag
                if self.walk_cancel.is_some() {
                    return Task::none();
                }
                let steps: Vec<_> =
                    (self.steps.as_ref()).map_or(Vec::new(), |(_, s)| s.steps().to_vec());
                let cancel = std::sync::Arc::new(AtomicBool::new(false));
                self.walk_cancel = Some(cancel.clone());
                background(move |tx| {
                    // where to put it first: the frames are drawn only for a file that is wanted
                    let done = export_walkthrough(frames, steps, how, &cancel, tx);
                    let _ = tx.unbounded_send(Walked::End(done));
                })
                .map(Msg::StudioWalk)
            }
            Outcome::CancelWalkthrough => {
                if let Some(c) = &self.walk_cancel {
                    c.store(true, Ordering::Relaxed);
                }
                Task::none()
            }
            Outcome::ExportPicture(crop, how) => background(move |tx| {
                if let Some(done) = export_picture(crop, how) {
                    let _ = tx.unbounded_send(done);
                }
            })
            .map(Msg::StudioToast),
            Outcome::NewSnip => Task::done(Msg::Run(crate::Trigger::Quick(
                std::time::Instant::now(),
                crate::shared::config::QuickAction {
                    action: "snip".into(),
                    shortcut: String::new(),
                    target: crate::shared::config::Target::Circle,
                    live: false,
                },
            ))),
            Outcome::Settings => self.open(super::Kind::Settings),
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

/// Blocking: where to put the exported picture (the save dialog, in Save's
/// folder), then the file; `None`: the dialog was cancelled. Ok / Err = the pill.
fn export_picture(crop: Crop, how: export::Picture) -> Option<Result<String, String>> {
    let ext = how.format.ext();
    // a size the format can't hold: say so before asking where, never a bare "Not saved"
    match export::problem(crop.width, crop.height, how) {
        Some(export::Problem::WebpTooBig) => {
            return Some(Err(crate::t!(st_export_webp_big).into()));
        }
        Some(export::Problem::PdfTooWide) => {
            return Some(Err(crate::t!(st_export_pdf_wide).into()));
        }
        None => {}
    }
    let mut dialog = rfd::FileDialog::new()
        .set_title(crate::t!(st_export_where))
        .set_file_name(format!("{}.{ext}", save::stamped_name("")))
        .add_filter(how.format.name(), &[ext]);
    let config = crate::shared::config::current();
    if let Some(folder) = save::start_folder(&config.general.save_folder) {
        dialog = dialog.set_directory(folder);
    }
    let path = export::with_ext(dialog.save_file()?, how.format);
    let written = export::encode(crop, how)
        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
    Some(match written {
        Ok(()) => {
            let folder = path.parent().unwrap_or(&path);
            Ok(fmt(S::pill_saved, &[&save::short_path(folder)]))
        }
        Err(e) => {
            crate::log!("[opencircle] export failed: {e}");
            Err(crate::t!(pill_not_saved).to_string())
        }
    })
}

/// Blocking: where to put the walkthrough (the save dialog, in Save's
/// folder), then the file, encoded with a `Walked::Progress` after each frame.
/// `None`: cancelled (the dialog or the encoder). Ok / Err = the pill.
fn export_walkthrough(
    frames: studio::Frames,
    steps: Vec<Arc<Step>>,
    how: export::Walkthrough,
    cancel: &AtomicBool,
    tx: &iced::futures::channel::mpsc::UnboundedSender<Walked>,
) -> Option<Result<String, String>> {
    let ext = how.movie.ext();
    let mut dialog = rfd::FileDialog::new()
        .set_title(crate::t!(st_walk_where))
        .set_file_name(format!("{}.{ext}", save::stamped_name("steps-")))
        .add_filter(how.movie.name(), &[ext]);
    let config = crate::shared::config::current();
    if let Some(folder) = save::start_folder(&config.general.save_folder) {
        dialog = dialog.set_directory(folder);
    }
    let path = dialog.save_file()?.with_extension(ext);
    let frames: Vec<Crop> = match frames {
        studio::Frames::Steps => steps.iter().filter_map(|s| s.crop()).collect(),
        studio::Frames::Edits(mut doc) => export::edit_frames(&mut doc, how),
    };
    let encoded = export::encode_walkthrough(frames, how, &mut |done, total| {
        let _ = tx.unbounded_send(Walked::Progress(done, total));
        !cancel.load(Ordering::Relaxed)
    });
    // stopped (the Cancel, or the Studio closed) while it ran: no file
    let written = match encoded {
        Ok(Some(bytes)) if !cancel.load(Ordering::Relaxed) => {
            std::fs::write(&path, bytes).map_err(|e| e.to_string())
        }
        Ok(_) => return None,
        Err(e) => Err(e),
    };
    Some(match written {
        Ok(()) => Ok(fmt(
            S::pill_saved,
            &[&save::short_path(path.parent().unwrap_or(&path))],
        )),
        Err(e) => {
            crate::log!("[opencircle] walkthrough export failed: {e}");
            Err(crate::t!(pill_not_saved).to_string())
        }
    })
}

/// Blocking: a folder for the steps (the folder dialog, from Save's folder),
/// then a new folder in it with the steps; `None`: cancelled.
fn export_steps(steps: &[Arc<Step>], how: export::Steps) -> Option<Result<String, String>> {
    let mut dialog = rfd::FileDialog::new().set_title(crate::t!(st_export_folder));
    let config = crate::shared::config::current();
    if let Some(folder) = save::start_folder(&config.general.save_folder) {
        dialog = dialog.set_directory(folder);
    }
    let parent = dialog.pick_folder()?;
    let pngs: Vec<&[u8]> = steps.iter().map(|s| s.png()).collect();
    let words = export::Words {
        title: crate::t!(st_steps),
        step: crate::t!(st_step),
        rtl: crate::shared::app_language::rtl(),
    };
    let name = save::stamped_name("steps-");
    Some(
        match export::write_steps(&parent, &name, &pngs, how, &words) {
            Ok(folder) => Ok(exported_steps(steps.len(), &save::short_path(&folder))),
            Err(e) => {
                crate::log!("[opencircle] steps export failed: {e}");
                Err(crate::t!(pill_not_saved).to_string())
            }
        },
    )
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
    // the menu bar: Alt opens it, the arrows walk it (ignored while it is closed)
    if modifiers.is_empty() || modifiers == keyboard::Modifiers::ALT {
        let nav = match key {
            keyboard::Key::Named(Named::Alt) => Some(studio::Nav::Alt),
            keyboard::Key::Named(Named::ArrowLeft) => Some(studio::Nav::Prev),
            keyboard::Key::Named(Named::ArrowRight) => Some(studio::Nav::Next),
            keyboard::Key::Named(Named::ArrowUp) => Some(studio::Nav::Up),
            keyboard::Key::Named(Named::ArrowDown) => Some(studio::Nav::Down),
            keyboard::Key::Named(Named::Enter) => Some(studio::Nav::Enter),
            _ => None,
        };
        if let Some(n) = nav {
            return Some(studio::Msg::MenuKey(n));
        }
    }
    if !modifiers.command() {
        return None;
    }
    Some(match (physical_key, key.to_latin(physical_key)) {
        // panels: Ctrl+B left, Ctrl+Alt+B right, Ctrl+\ both
        (Physical::Code(Code::KeyB), _) if modifiers.alt() => studio::Msg::Panel,
        (Physical::Code(Code::KeyB), _) => studio::Msg::Library,
        (Physical::Code(Code::Backslash), _) => studio::Msg::Both,
        (Physical::Code(Code::Slash), _) => studio::Msg::Shortcuts(true),
        (Physical::Code(Code::Comma), _) => studio::Msg::Settings,
        (Physical::Code(Code::Equal | Code::NumpadAdd), _) => studio::Msg::ZoomIn,
        (Physical::Code(Code::Minus | Code::NumpadSubtract), _) => studio::Msg::ZoomOut,
        (Physical::Code(Code::Digit0 | Code::Numpad0), _) => studio::Msg::Fit,
        (Physical::Code(Code::Digit1 | Code::Numpad1), _) => studio::Msg::Actual,
        (_, Some('o')) => studio::Msg::Open,
        (_, Some('v')) => studio::Msg::Paste,
        (_, Some('w')) => studio::Msg::Caption(crate::shared::kit::header::Caption::Close),
        _ => return None,
    })
}

/// The Studio's own keys (those its editor's canvas didn't take): Ctrl+O, Ctrl+V,
/// the panels (Ctrl+B / Ctrl+Alt+B / Ctrl+\, by physical key), the zoom
/// (Ctrl+= / Ctrl+- / Ctrl+0 / Ctrl+1); and files dragged over or dropped on it.
pub(super) fn studio_events() -> iced::Subscription<Msg> {
    iced::event::listen_with(|event, status, id| {
        let m = match event {
            iced::Event::Window(window::Event::Focused) => studio::Msg::Focus(true),
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

    /// STUDIO_V3 §7: Ctrl+B / Ctrl+Alt+B / Ctrl+\ are the panels, Ctrl+/ the
    /// shortcuts, Alt the menu bar; Ctrl+[ and Ctrl+] are z-order, not panels.
    #[test]
    fn v3_panel_and_menu_keys() {
        let ctrl = keyboard::Modifiers::CTRL;
        let key = |c: &str, code, m| {
            key_msg(&keyboard::Key::Character(c.into()), Physical::Code(code), m)
        };
        assert!(matches!(
            key("b", Code::KeyB, ctrl),
            Some(studio::Msg::Library)
        ));
        let both = ctrl | keyboard::Modifiers::ALT;
        assert!(matches!(
            key("b", Code::KeyB, both),
            Some(studio::Msg::Panel)
        ));
        assert!(matches!(
            key("\\", Code::Backslash, ctrl),
            Some(studio::Msg::Both)
        ));
        assert!(matches!(
            key("/", Code::Slash, ctrl),
            Some(studio::Msg::Shortcuts(true))
        ));
        assert!(key("[", Code::BracketLeft, ctrl).is_none());
        let alt = keyboard::Key::Named(Named::Alt);
        let any = Physical::Code(Code::AltLeft);
        assert!(matches!(
            key_msg(&alt, any, keyboard::Modifiers::ALT),
            Some(studio::Msg::MenuKey(studio::Nav::Alt))
        ));
    }
}

/// "Exported 1 step to <folder>" (and Arabic's forms).
fn exported_steps(n: usize, folder: &str) -> String {
    let count = crate::shared::app_language::count(
        n,
        [
            S::st_steps_one,
            S::st_steps_two,
            S::st_steps_few,
            S::st_steps_many,
        ],
    );
    fmt(S::st_exported_steps, &[&count, &folder])
}

#[cfg(test)]
mod tests {
    use crate::shared::app_language::{Lang, set};

    #[test]
    fn the_exported_steps_message_counts_in_the_apps_language() {
        set(Lang::En);
        assert_eq!(super::exported_steps(1, "d"), "Exported 1 step to d");
        assert_eq!(super::exported_steps(5, "d"), "Exported 5 steps to d");
        set(Lang::Ar);
        let ar = |n| super::exported_steps(n, "d");
        assert!(ar(1).contains("خطوة واحدة") && ar(2).contains("خطوتان"));
        assert!(ar(5).contains("5 خطوات") && ar(12).contains("12 خطوة"));
        set(Lang::En);
    }
}
