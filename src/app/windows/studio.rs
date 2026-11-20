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
        // the list and each file's size and age: read once, off the UI thread
        let recent = background(|tx| {
            let list = crate::shared::history::recent::load();
            let info: Vec<String> = list.iter().map(|(p, _)| studio::recent_line(p)).collect();
            let _ = tx.unbounded_send((list, info));
        })
        .map(move |(r, info)| to(studio::Msg::Recent(r, info)));
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
        if let Some((_, s)) = &mut self.studio {
            s.next_name(&path);
        }
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

    /// Pictures read for the Steps: each joins the strip as the next step.
    pub(super) fn studio_add_pictures(&mut self, crops: Vec<Crop>) -> Task<Msg> {
        if crops.is_empty() {
            return self.toast(crate::t!(st_unreadable), false, None, TOAST_MS);
        }
        let tasks: Vec<_> = crops
            .into_iter()
            .map(|crop| self.studio_outcome(Outcome::AddStep(crop)))
            .collect();
        Task::batch(tasks)
    }

    /// The clipboard was read for Ctrl+V: a picture opens; else the editor
    /// pastes its text at `at`; else the pill says there is none.
    pub(super) fn studio_pasted(
        &mut self,
        image: Option<Crop>,
        at: Option<iced::Point>,
    ) -> Task<Msg> {
        match (image, &mut self.studio) {
            // a picture is open: our own copied objects, or a movable pasted picture
            (Some(crop), Some((_, s))) if s.editor_open() => {
                let next = s.paste_objects(crop);
                self.studio_outcome(next)
            }
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
            Outcome::PickFiles => background(|tx| {
                let picked = rfd::FileDialog::new()
                    .set_title(crate::t!(st_open_title))
                    .add_filter(
                        crate::t!(st_pictures),
                        &["png", "jpg", "jpeg", "webp", "bmp", "gif"],
                    )
                    .pick_files();
                if let Some(paths) = picked {
                    let _ = tx.unbounded_send(paths);
                }
            })
            .map(|paths| Msg::Studio(studio::Msg::AddFiles(paths))),
            Outcome::AddPictures(paths) => background(move |tx| {
                let crops: Vec<Crop> = paths
                    .iter()
                    .filter_map(|p| match read_picture(p) {
                        Ok(crop) => Some(crop),
                        Err(e) => {
                            crate::log!("[opencircle] studio: {e}");
                            None
                        }
                    })
                    .collect();
                let _ = tx.unbounded_send(crops);
            })
            .map(Msg::StudioAddPictures),
            Outcome::PasteStep => background(move |tx| {
                let _ = tx.unbounded_send(copy::read_image().into_iter().collect::<Vec<_>>());
            })
            .map(Msg::StudioAddPictures),
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
            Outcome::AddStep(crop) => {
                // the strip opens on the first screen when there is none yet
                let place = crate::shared::platform::monitor::places()
                    .into_iter()
                    .next();
                match place {
                    Some(place) => {
                        let inset = crate::shared::platform::monitor::taskbar_inset(
                            place.logical_origin(),
                            place.size(),
                            place.scale,
                        );
                        self.add_step(crop, None, place, inset)
                    }
                    None => Task::none(),
                }
            }
            Outcome::KeepCopy(job) => {
                // encode and write off the UI thread; a discard in between voids it
                std::thread::spawn(move || {
                    let png = crate::shared::picture::encode_png(job.width, job.height, &job.rgba);
                    if let Ok(png) = png {
                        let _ =
                            studio::recovery::write_if_current(job.ticket, true, &png, &job.notes);
                    }
                });
                Task::none()
            }
            Outcome::ClearRecent => {
                std::thread::spawn(crate::shared::history::recent::clear);
                Task::none()
            }
            Outcome::Prefs(p) => {
                std::thread::spawn(move || {
                    if let Err(e) = crate::shared::config::update(|c| c.studio = p) {
                        crate::log!("[opencircle] config: {e}");
                    }
                });
                Task::none()
            }
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

/// Settings > Studio > Saving with the dialog the user has (Save as, "Ask where
/// to save"). Blocking; `None`: the dialog was cancelled. Ok / Err = the pill.
fn save_file(crop: Crop) -> Option<Result<String, String>> {
    let config = crate::shared::config::current();
    save_to(
        crop,
        &config.studio,
        config.snip.max_edge,
        |folder, name, how| {
            rfd::FileDialog::new()
                .set_title(crate::t!(st_export_where))
                .set_directory(folder)
                .set_file_name(name)
                .add_filter(how.format.name(), &[how.format.ext()])
                .save_file()
        },
    )
}

/// The Studio's Save: the folder, the name pattern (`{date}` `{time}`), the format
/// and quality, "Ask where to save" (`ask` is the dialog: the folder, the name with
/// its extension, the format; `None` = cancelled) and "Copy the file path". It
/// never writes over a file the user did not confirm: a taken name gets `-2`, `-3`
/// (created exclusively, so a file made meanwhile is safe too).
fn save_to(
    crop: Crop,
    prefs: &crate::shared::config::StudioPrefs,
    max_edge: u32,
    ask: impl FnOnce(&std::path::Path, &str, export::Picture) -> Option<std::path::PathBuf>,
) -> Option<Result<String, String>> {
    use crate::shared::config::Format;
    let not_saved = || crate::t!(pill_not_saved).to_string();
    let how = export::Picture {
        format: match prefs.format {
            Format::Png => export::Format::Png,
            Format::Jpeg => export::Format::Jpg,
        },
        quality: prefs.quality,
        max_edge,
    };
    let ext = how.format.ext();
    let (date, time) = save::date_time();
    let stem = prefs
        .file_name
        .replace("{date}", &date)
        .replace("{time}", &time);
    let stem = match stem.trim().is_empty() {
        true => save::stamped_name(""),
        false => stem,
    };
    let Some(folder) = save::start_folder(&prefs.folder) else {
        return Some(Err(not_saved()));
    };
    let bytes = match export::encode(crop, how) {
        Ok(b) => b,
        Err(e) => {
            crate::log!("[opencircle] save failed: {e}");
            return Some(Err(not_saved()));
        }
    };
    // the name the dialog starts with has its extension already; what the user
    // confirms there is what may be overwritten, nothing else
    let (path, confirmed) = match prefs.ask_where {
        true => {
            let picked = ask(&folder, &format!("{stem}.{ext}"), how)?;
            let named = export::with_ext(picked.clone(), how.format);
            let same = named == picked;
            (named, same)
        }
        false => (folder.join(format!("{stem}.{ext}")), false),
    };
    let written = match write_file(&path, &bytes, confirmed) {
        Ok(p) => p,
        Err(e) => {
            crate::log!("[opencircle] save failed: {e}");
            return Some(Err(not_saved()));
        }
    };
    if prefs.copy_path {
        let _ = copy::text(written.display().to_string());
    }
    let shown = written.parent().unwrap_or(&written);
    Some(Ok(fmt(S::pill_saved, &[&save::short_path(shown)])))
}

/// `bytes` into `path`, or into `<name>-2.<ext>`, `-3` ... when it is taken: created
/// exclusively. `confirmed`: the user was asked about this very file (the dialog's
/// overwrite question): it is written over. Returns where it went.
fn write_file(
    path: &std::path::Path,
    bytes: &[u8],
    confirmed: bool,
) -> std::io::Result<std::path::PathBuf> {
    write_file_with(path, confirmed, |file| {
        use std::io::Write;
        file.write_all(bytes)
    })
}

/// `write_file` with the writing given: a file made new that then fails to be written
/// (a full disk) is removed, never left cut off.
fn write_file_with(
    path: &std::path::Path,
    confirmed: bool,
    put: impl Fn(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<std::path::PathBuf> {
    if confirmed {
        let mut file = std::fs::File::create(path)?;
        if let Err(e) = put(&mut file) {
            drop(file);
            let _ = std::fs::remove_file(path); // (the user chose it: a cut-off file is no better than none)
            return Err(e);
        }
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1.. {
        let name = match (n, &ext) {
            (1, _) => path.to_path_buf(),
            (n, Some(ext)) => path.with_file_name(format!("{stem}-{n}.{ext}")),
            (n, None) => path.with_file_name(format!("{stem}-{n}")),
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&name)
        {
            Ok(mut file) => {
                if let Err(e) = put(&mut file) {
                    drop(file);
                    let _ = std::fs::remove_file(&name);
                    return Err(e);
                }
                return Ok(name);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// Save off the UI thread (encoding a big picture takes a while): `Msg::StudioSaved`.
fn studio_save(crop: Crop, then: bool) -> Task<Msg> {
    background(move |tx| {
        // (a cancelled Save dialog says nothing)
        if let Some(saved) = save_file(crop) {
            let _ = tx.unbounded_send(saved);
        }
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
    // the menu bar: Alt opens it on its release (the Alt key itself is the
    // closure's), the arrows walk it (ignored while it is closed)
    if modifiers.is_empty() || modifiers == keyboard::Modifiers::ALT {
        let nav = match key {
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
    if *key == keyboard::Key::Named(Named::Space) && modifiers.is_empty() {
        return Some(studio::Msg::Space(true));
    }
    // Shift+1: the whole picture in the stage (Fit)
    if modifiers == keyboard::Modifiers::SHIFT
        && matches!(physical_key, Physical::Code(Code::Digit1 | Code::Numpad1))
    {
        return Some(studio::Msg::Fit);
    }
    if !modifiers.command() {
        return None;
    }
    Some(match (physical_key, key.to_latin(physical_key)) {
        // the Steps sidebar: Ctrl+B
        (_, Some('s')) if modifiers.shift() => studio::Msg::Export,
        (Physical::Code(Code::KeyB), _) => studio::Msg::Library,
        (Physical::Code(Code::Slash), _) => studio::Msg::Shortcuts(true),
        (Physical::Code(Code::Comma), _) => studio::Msg::Settings,
        // the Studio's UI scale (as in the AI chat); with Shift, the picture's zoom
        (Physical::Code(Code::Equal | Code::NumpadAdd), _) if modifiers.shift() => {
            studio::Msg::ZoomIn
        }
        (Physical::Code(Code::Minus | Code::NumpadSubtract), _) if modifiers.shift() => {
            studio::Msg::ZoomOut
        }
        (Physical::Code(Code::Equal | Code::NumpadAdd), _) => studio::Msg::UiScale(1),
        (Physical::Code(Code::Minus | Code::NumpadSubtract), _) => studio::Msg::UiScale(-1),
        (Physical::Code(Code::Digit0 | Code::Numpad0), _) => studio::Msg::UiScale(0),
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
pub(super) fn studio_events(waiting: bool) -> iced::Subscription<Msg> {
    // the unsaved copy's debounce runs on a clock, only while an edit waits for its copy
    let tick = match waiting {
        true => iced::Subscription::run(recovery_ticks).map(|()| Msg::Studio(studio::Msg::Tick)),
        false => iced::Subscription::none(),
    };
    iced::Subscription::batch([tick, keys()])
}

/// One message a second while the unsaved copy is kept (a thread on a sleep; a
/// tick the app is busy with is dropped).
fn recovery_ticks() -> impl iced::futures::Stream<Item = ()> {
    iced::stream::channel(1, async |mut out| {
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if out.try_send(()).is_err_and(|e| e.is_disconnected()) {
                    return;
                }
            }
        });
        std::future::pending::<()>().await;
    })
}

fn keys() -> iced::Subscription<Msg> {
    iced::event::listen_with(|event, status, id| {
        let m = match event {
            iced::Event::Window(window::Event::Focused) => studio::Msg::Focus(true),
            iced::Event::Window(window::Event::FileHovered(_)) => studio::Msg::Hovered(true),
            iced::Event::Window(window::Event::FilesHoveredLeft) => studio::Msg::Hovered(false),
            iced::Event::Window(window::Event::FileDropped(path)) => studio::Msg::Dropped(path),
            iced::Event::Keyboard(keyboard::Event::KeyReleased {
                key: keyboard::Key::Named(keyboard::key::Named::Space),
                ..
            }) => studio::Msg::Space(false),
            iced::Event::Keyboard(keyboard::Event::KeyReleased {
                key: keyboard::Key::Named(keyboard::key::Named::Alt),
                ..
            }) => studio::Msg::AltUp,
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Alt),
                repeat,
                ..
            }) => match repeat {
                true => return None,
                false => studio::Msg::AltDown,
            },
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                studio::Msg::Mods(m.command(), m.shift())
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => match (status, key_msg(&key, physical_key, modifiers)) {
                (iced::event::Status::Ignored, Some(m)) => m,
                // (any other key ends an Alt press: its release opens nothing)
                _ => studio::Msg::AltBreak,
            },
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
        // v3.1: one sidebar; Ctrl+\ is no key any more
        assert!(key("\\", Code::Backslash, ctrl).is_none());
        assert!(matches!(
            key("/", Code::Slash, ctrl),
            Some(studio::Msg::Shortcuts(true))
        ));
        assert!(key("[", Code::BracketLeft, ctrl).is_none());
        // Alt is the subscription's own (down / up); here it is no key
        let alt = keyboard::Key::Named(Named::Alt);
        let any = Physical::Code(Code::AltLeft);
        assert!(key_msg(&alt, any, keyboard::Modifiers::ALT).is_none());
    }

    /// The key names the menus, the sheet and the tooltips show (`studio::keys`)
    /// are the keys that act: each window-level chord maps to its message.
    #[test]
    fn v3_the_labelled_keys_are_the_keys_that_act() {
        use studio::keys as k;
        let chord = |name: &str| {
            let mut m = keyboard::Modifiers::empty();
            let mut last = "";
            for part in name.split('+') {
                match part {
                    "Ctrl" => m |= keyboard::Modifiers::CTRL,
                    "Shift" => m |= keyboard::Modifiers::SHIFT,
                    "Alt" => m |= keyboard::Modifiers::ALT,
                    other => last = other,
                }
            }
            let (c, code) = match last {
                "O" | "o" => ("o", Code::KeyO),
                "S" => ("s", Code::KeyS),
                "V" => ("v", Code::KeyV),
                "W" => ("w", Code::KeyW),
                "B" => ("b", Code::KeyB),
                "," => (",", Code::Comma),
                "/" => ("/", Code::Slash),
                "\\" => ("\\", Code::Backslash),
                "=" => ("=", Code::Equal),
                "-" => ("-", Code::Minus),
                "0" => ("0", Code::Digit0),
                "1" => ("1", Code::Digit1),
                other => panic!("{other}"),
            };
            key_msg(&keyboard::Key::Character(c.into()), Physical::Code(code), m)
        };
        let is = |name: &str, ok: fn(&studio::Msg) -> bool| {
            let got = chord(name);
            assert!(got.as_ref().is_some_and(ok), "{:?}", (name, &got));
        };
        is(k::OPEN, |m| matches!(m, studio::Msg::Open));
        is(k::SAVE_AS, |m| matches!(m, studio::Msg::Export));
        is(k::PASTE, |m| matches!(m, studio::Msg::Paste));
        is(k::CLOSE, |m| matches!(m, studio::Msg::Caption(_)));
        is(k::STEPS_SIDEBAR, |m| matches!(m, studio::Msg::Library));
        is(k::UI_LARGER, |m| matches!(m, studio::Msg::UiScale(1)));
        is(k::UI_SMALLER, |m| matches!(m, studio::Msg::UiScale(-1)));
        is(k::UI_RESET, |m| matches!(m, studio::Msg::UiScale(0)));
        is(k::SETTINGS, |m| matches!(m, studio::Msg::Settings));
        is(k::SHORTCUTS, |m| matches!(m, studio::Msg::Shortcuts(true)));
        is(k::ZOOM_IN, |m| matches!(m, studio::Msg::ZoomIn));
        is(k::ZOOM_OUT, |m| matches!(m, studio::Msg::ZoomOut));
        is(k::FIT, |m| matches!(m, studio::Msg::Fit));
        is(k::ACTUAL, |m| matches!(m, studio::Msg::Actual));
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

    fn picture() -> crate::shared::picture::Crop {
        crate::shared::picture::Crop {
            width: 8,
            height: 6,
            rgba: vec![255; 8 * 6 * 4],
        }
    }

    fn prefs(folder: &std::path::Path) -> crate::shared::config::StudioPrefs {
        crate::shared::config::StudioPrefs {
            folder: folder.display().to_string(),
            file_name: "shot {date} {time}".into(),
            ..Default::default()
        }
    }

    /// Settings > Studio > Saving: the folder and the name pattern are used; a second
    /// save never overwrites the first (`-2`); the path goes on the clipboard when
    /// asked for.
    #[test]
    fn save_uses_the_folder_and_pattern_and_never_overwrites() {
        let dir = crate::shared::paths::test_dir("w87-save-plain");
        let mut p = prefs(&dir);
        p.copy_path = true;
        let say = |p: &crate::shared::config::StudioPrefs| {
            super::save_to(picture(), p, 0, |_, _, _| panic!("no dialog"))
                .unwrap()
                .unwrap()
        };
        let _ = say(&p);
        let _ = say(&p);
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(
            names
                .iter()
                .all(|n| n.starts_with("shot 20") && n.ends_with(".png")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.contains("-2.")),
            "the second got its own name: {names:?}"
        );
        let copied = crate::features::circle::actions::copy::copied::text().unwrap();
        assert!(copied.starts_with(&dir.display().to_string()), "{copied}");
        p.copy_path = false;
        p.format = crate::shared::config::Format::Jpeg;
        let _ = say(&p);
        assert!(
            std::fs::read_dir(&dir).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".jpg"))
        );
    }

    /// "Ask where to save": the dialog starts in the folder with the extension on
    /// its name; what it returns is written (and may be overwritten: the dialog asked);
    /// cancelled = nothing written; a name typed without the extension is never written
    /// over an existing file the dialog did not ask about.
    #[test]
    fn save_as_asks_where_and_overwrites_only_what_was_confirmed() {
        let dir = crate::shared::paths::test_dir("w87-save-ask");
        let mut p = prefs(&dir);
        p.ask_where = true;
        p.file_name = "mine".into();
        let target = dir.join("chosen.png");
        std::fs::write(&target, b"old").unwrap();
        let seen = std::cell::RefCell::new(None);
        let out = super::save_to(picture(), &p, 0, |folder, name, _| {
            *seen.borrow_mut() = Some((folder.to_path_buf(), name.to_string()));
            Some(target.clone())
        });
        assert!(out.unwrap().is_ok());
        assert_eq!(
            seen.into_inner().unwrap(),
            (dir.clone(), "mine.png".to_string())
        );
        assert_ne!(
            std::fs::read(&target).unwrap(),
            b"old",
            "confirmed: overwritten"
        );
        // cancelled: nothing
        assert!(super::save_to(picture(), &p, 0, |_, _, _| None).is_none());
        // typed without the extension, and that file exists: not overwritten
        std::fs::write(dir.join("typed.png"), b"keep").unwrap();
        let out = super::save_to(picture(), &p, 0, |_, _, _| Some(dir.join("typed")));
        assert!(out.unwrap().is_ok());
        assert_eq!(std::fs::read(dir.join("typed.png")).unwrap(), b"keep");
        assert!(dir.join("typed-2.png").exists());
    }

    /// A write that fails after the file was made leaves no cut-off file.
    #[test]
    fn a_failed_write_leaves_no_file() {
        let dir = crate::shared::paths::test_dir("w87-write-fail");
        let path = dir.join("a.png");
        let fail = |_: &mut std::fs::File| Err(std::io::Error::other("disk full"));
        assert!(super::write_file_with(&path, false, fail).is_err());
        assert!(!path.exists(), "no cut-off file");
        std::fs::write(&path, b"old").unwrap();
        assert!(super::write_file_with(&path, true, fail).is_err());
        assert!(
            !path.exists(),
            "a confirmed overwrite that fails is removed too"
        );
    }

    /// Exclusive creation: a name that appears meanwhile is never written over.
    #[test]
    fn write_file_takes_the_next_free_name() {
        let dir = crate::shared::paths::test_dir("w87-write-new");
        let path = dir.join("a.png");
        std::fs::write(&path, b"1").unwrap();
        std::fs::write(dir.join("a-2.png"), b"2").unwrap();
        let got = super::write_file(&path, b"new", false).unwrap();
        assert_eq!(got, dir.join("a-3.png"));
        assert_eq!(std::fs::read(&path).unwrap(), b"1");
    }

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
