//! Glossaries (GLOSSARY_SPEC.md "UI"): the Glossary row of Settings >
//! Translate (a dropdown of the glossaries and an Edit button), and the
//! editor, a sub-page of Translate (breadcrumb "Translate / Glossaries", no
//! new window): the glossary dropdown and More, the window titles that pick
//! it, search and Add term, and a two-column table: On screen | In <target>.
//! An empty translation keeps the term as it is (grey placeholder). Every
//! change is written to the glossary's file at once (it is small).
use std::path::PathBuf;

use iced::widget::{Space, column, container, mouse_area};
use iced::{Element, Fill};

use super::super::state::{Message, Named, State, TARGETS};
use crate::features::translate::glossary_store as store;
use crate::features::translate::{Glossary, Term};
use crate::shared::app_language::{S, fmt};
use crate::shared::kit::button::{Kind, button, label};
use crate::shared::kit::dropdown::{dropdown, menu_button};
use crate::shared::kit::field::{input, search};
use crate::shared::kit::icon::{Icon, icon};
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::table;
use crate::shared::kit::text::{muted, sub};
use crate::shared::kit::tokens::{Role, size, space};
use crate::t;

/// The Settings row's "New glossary" choice (never a file name).
const NEW: &str = "\u{1}new";
/// Rows drawn at most (the rest: search to narrow).
const SHOWN: usize = 100;

/// One choice of a glossary dropdown: what it sets, and its words.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    slug: String,
    label: String,
}

/// "1 term", "7 terms" (and Arabic's forms).
fn terms(n: usize) -> String {
    crate::shared::app_language::count(
        n,
        [
            S::gl_terms_one,
            S::gl_terms_two,
            S::gl_terms_few,
            S::gl_terms_many,
        ],
    )
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The row's dropdown: a glossary's slug, "" = none, or `NEW`.
    Active(String),
    /// The row's Edit button / the editor's Back.
    Edit,
    Back,
    /// The editor's glossary dropdown.
    Pick(String),
    New,
    /// The pointer is over this row of the table (`None` = left it).
    Hover(Option<usize>),
    /// Rename: open the name field (true) / the field's text.
    Renaming(bool),
    Name(String),
    AskDelete(bool),
    Delete,
    Import,
    /// The picked file's text (`None` = cancelled; an error says why).
    Imported(Option<Result<String, String>>),
    Export,
    Exported(Result<(), String>),
    Lang(Named),
    Windows(String),
    Query(String),
    Add,
    Text(usize, String),
    Trans(usize, String),
    Remove(usize),
    Undo,
}

/// A file dialog for the shell (it runs it off the UI thread and answers
/// with `run`'s message).
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Import,
    Export { name: String, text: String },
}

impl Job {
    pub fn run(self) -> Msg {
        match self {
            Job::Import => Msg::Imported(
                rfd::FileDialog::new()
                    .set_title(t!(dlg_gl_open))
                    .add_filter("toml", &["toml"])
                    .pick_file()
                    .map(|p| {
                        let size = std::fs::metadata(&p).map_or(0, |m| m.len());
                        match size as usize > store::MAX_BYTES {
                            true => Err(store::Error::TooBig.to_string()),
                            false => std::fs::read_to_string(p).map_err(|e| e.to_string()),
                        }
                    }),
            ),
            Job::Export { name, text } => {
                let file = rfd::FileDialog::new()
                    .set_title(t!(dlg_gl_save))
                    .set_file_name(format!("{}.toml", store::slug(&name)))
                    .add_filter("toml", &["toml"])
                    .save_file();
                Msg::Exported(match file {
                    Some(p) => write(p, &text),
                    None => Ok(()),
                })
            }
        }
    }
}

fn write(path: PathBuf, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// What a message asks of the window.
#[derive(Debug, Default)]
pub struct Reply {
    pub job: Option<Job>,
    /// The active glossary changed (`translate.glossary`).
    pub active: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Page {
    dir: PathBuf,
    all: Vec<(String, Glossary)>,
    /// The editor is open, on this glossary's slug.
    editing: Option<String>,
    shown: String,
    query: String,
    hover: Option<usize>,
    /// The name field (Rename / New), its text.
    renaming: Option<String>,
    confirm_delete: bool,
    /// What the last import merged into a glossary of the same name (one line).
    summary: Option<String>,
    /// The last removed term (its index), for Undo.
    undo: Option<(usize, Term)>,
    problem: Option<String>,
    choices: Vec<Choice>,
    /// `choices` and "New glossary" (the row's dropdown).
    options: Vec<Choice>,
}

impl Page {
    pub fn load(target: &str) -> Page {
        Page::at(store::dir(), target)
    }

    pub(crate) fn at(dir: PathBuf, target: &str) -> Page {
        let mut p = Page {
            all: Vec::new(),
            dir,
            editing: None,
            shown: target.to_string(),
            query: String::new(),
            hover: None,
            renaming: None,
            confirm_delete: false,
            summary: None,
            undo: None,
            problem: None,
            choices: Vec::new(),
            options: Vec::new(),
        };
        p.reload();
        p
    }

    fn reload(&mut self) {
        self.all = store::load_all(&self.dir);
        self.words();
    }

    /// The dropdown's choices in the app's language.
    pub(crate) fn words(&mut self) {
        let mut c = vec![Choice {
            slug: String::new(),
            label: t!(gl_none).into(),
        }];
        c.extend(self.all.iter().map(|(s, g)| Choice {
            slug: s.clone(),
            label: fmt(S::gl_choice, &[&g.name, &terms(g.terms.len())]),
        }));
        self.options = c.clone();
        self.options.push(Choice {
            slug: NEW.into(),
            label: t!(gl_new).into(),
        });
        self.choices = c;
    }

    /// What the row's dropdown lists (tests).
    #[cfg(test)]
    pub fn labels(&self) -> Vec<String> {
        self.options.iter().map(|c| c.label.clone()).collect()
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    fn cur(&mut self) -> Option<&mut Glossary> {
        let slug = self.editing.as_deref()?;
        self.all.iter_mut().find(|(s, _)| s == slug).map(|(_, g)| g)
    }

    /// Writes the glossary being edited.
    // ponytail: written at every change (a small file, off no thread); a
    // 5 000-term file per keystroke is the ceiling: debounce when it shows.
    fn save(&mut self) {
        let Some(slug) = self.editing.clone() else {
            return;
        };
        if let Some((_, g)) = self.all.iter().find(|(s, _)| *s == slug)
            && let Err(e) = store::save(&self.dir, &slug, g)
        {
            self.problem = Some(fmt(S::gl_failed, &[&e]));
        }
        self.words();
    }

    fn open(&mut self, slug: &str) {
        self.editing = Some(slug.to_string());
        self.query.clear();
        self.hover = None;
        self.renaming = None;
        self.confirm_delete = false;
        self.undo = None;
        self.problem = None;
        self.summary = None;
    }

    fn create(&mut self, name: &str) -> String {
        let slug = store::free_slug(&self.dir, name);
        let g = Glossary {
            name: name.to_string(),
            ..Glossary::default()
        };
        if let Err(e) = store::save(&self.dir, &slug, &g) {
            self.problem = Some(fmt(S::gl_failed, &[&e]));
        }
        self.reload();
        slug
    }

    pub fn update(&mut self, msg: Msg, target: &str) -> Reply {
        let mut reply = Reply::default();
        match msg {
            Msg::Active(slug) if slug == NEW => {
                let s = self.create(t!(gl_default_name));
                self.open(&s);
                self.renaming = Some(t!(gl_default_name).into());
                reply.active = Some(s);
            }
            Msg::Active(slug) => reply.active = Some(slug),
            Msg::Edit => {
                self.reload();
                self.shown = target.to_string();
                let first = self.all.first().map(|(s, _)| s.clone());
                match first {
                    Some(s) => self.open(&s),
                    None => {
                        let s = self.create(t!(gl_default_name));
                        self.open(&s);
                    }
                }
            }
            Msg::Back => {
                self.editing = None;
                self.reload();
            }
            Msg::Pick(slug) => self.open(&slug),
            Msg::Hover(row) => self.hover = row,
            Msg::New => {
                let s = self.create(t!(gl_default_name));
                self.open(&s);
                self.renaming = Some(t!(gl_default_name).into());
            }
            Msg::Renaming(on) => {
                self.renaming = on.then(|| self.cur().map(|g| g.name.clone()).unwrap_or_default());
            }
            Msg::Name(name) => {
                if let Some(g) = self.cur() {
                    // (the file name stays: only the name shown changes)
                    g.name = name.clone();
                }
                self.renaming = Some(name);
                self.save();
            }
            Msg::AskDelete(on) => {
                self.confirm_delete = on;
                self.renaming = None;
            }
            Msg::Delete => {
                if let Some(slug) = self.editing.take() {
                    store::delete(&self.dir, &slug);
                    self.reload();
                    match self.all.first().map(|(s, _)| s.clone()) {
                        Some(s) => self.open(&s),
                        None => self.editing = None,
                    }
                    // (a deleted glossary is none for the pill)
                    reply.active = Some(String::new());
                }
                self.confirm_delete = false;
            }
            Msg::Import => reply.job = Some(Job::Import),
            Msg::Imported(None) => {}
            Msg::Imported(Some(Err(e))) => self.problem = Some(fmt(S::gl_failed, &[&e])),
            Msg::Imported(Some(Ok(text))) => match store::parse(&text) {
                Err(e) => self.problem = Some(fmt(S::gl_failed, &[&e])),
                Ok(g) => self.import(g),
            },
            Msg::Export => {
                if let Some(g) = self.cur() {
                    reply.job = Some(Job::Export {
                        name: g.name.clone(),
                        text: store::render(g),
                    });
                }
            }
            Msg::Exported(Err(e)) => self.problem = Some(fmt(S::gl_failed, &[&e])),
            Msg::Exported(Ok(())) => {}
            Msg::Lang(l) => self.shown = l.0.code.to_string(),
            Msg::Windows(s) => {
                if let Some(g) = self.cur() {
                    g.windows = (s.split(',').map(|w| w.trim().to_string()))
                        .filter(|w| !w.is_empty())
                        .collect();
                    self.save();
                }
            }
            Msg::Query(q) => self.query = q,
            Msg::Add => {
                self.query.clear();
                if let Some(g) = self.cur() {
                    g.terms.push(Term::default());
                    self.save();
                }
            }
            Msg::Text(i, text) => {
                if let Some(t) = self.cur().and_then(|g| g.terms.get_mut(i)) {
                    t.text = text;
                    self.save();
                }
            }
            Msg::Trans(i, text) => {
                let code = code_of(&self.shown);
                if let (Some(code), Some(t)) = (code, self.cur().and_then(|g| g.terms.get_mut(i))) {
                    match text.is_empty() {
                        true => t.translations.remove(code),
                        false => t.translations.insert(code.to_string(), text),
                    };
                    self.save();
                }
            }
            Msg::Remove(i) => {
                let removed = self
                    .cur()
                    .filter(|g| i < g.terms.len())
                    .map(|g| g.terms.remove(i));
                if let Some(t) = removed {
                    if let Some(g) = self.cur() {
                        g.removed.push(t.key());
                    }
                    self.undo = Some((i, t));
                    self.save();
                }
            }
            Msg::Undo => {
                if let Some((i, t)) = self.undo.take()
                    && let Some(g) = self.cur()
                {
                    g.removed.retain(|r| *r != t.key());
                    g.terms.insert(i.min(g.terms.len()), t);
                    self.save();
                }
            }
        }
        reply
    }

    /// A file for a game that already has a glossary (the same name) merges into
    /// it (`Glossary::merge`: yours win, deleted terms stay deleted); else it is
    /// a new glossary.
    fn import(&mut self, g: Glossary) {
        let name = g.name.to_lowercase();
        let same = (self.all.iter()).find(|(_, o)| o.name.to_lowercase() == name);
        let Some((slug, mut mine)) = same.cloned() else {
            return self.add_imported(g);
        };
        let done = mine.merge(g);
        // over a limit: refused, the file as it was (it would not load again)
        if store::check(&mine).is_err() {
            self.problem = Some(t!(gl_too_big).to_string());
            return;
        }
        if let Err(e) = store::save(&self.dir, &slug, &mine) {
            self.problem = Some(fmt(S::gl_failed, &[&e]));
        }
        self.reload();
        self.open(&slug);
        self.summary = Some(fmt(
            S::gl_merged,
            &[
                &done.added.to_string(),
                &done.kept_yours.to_string(),
                &done.duplicates.to_string(),
            ],
        ));
    }

    fn add_imported(&mut self, g: Glossary) {
        let slug = store::free_slug(&self.dir, &g.name);
        if let Err(e) = store::save(&self.dir, &slug, &g) {
            self.problem = Some(fmt(S::gl_failed, &[&e]));
        }
        self.reload();
        self.open(&slug);
    }
}

/// The Glossary row of Settings > Translate: the dropdown (None, each
/// glossary with its term count, New glossary) and Edit.
pub fn row(state: &State) -> Element<'_, Message> {
    let page = &state.glossaries;
    let active = &state.draft.translate.glossary;
    let chosen = (page.choices.iter())
        .find(|c| c.slug == *active)
        .cloned()
        .or_else(|| page.choices.first().cloned());
    let pick = dropdown(&page.options, chosen, t!(gl_none), |c: Choice| {
        Message::Glossaries(Msg::Active(c.slug))
    });
    let edit = button(
        label(t!(gl_edit_btn)),
        Kind::Elevated,
        Some(Message::Glossaries(Msg::Edit)),
    );
    setting_row::row(
        title(t!(gl_row)),
        t!(gl_row_desc),
        setting_row::field_with(pick, [edit]),
    )
}

/// Whether the OS tells which window is in front (Windows, X11): the
/// "Use for windows named" row shows only then.
fn titles() -> bool {
    crate::shared::platform::capture::window_rect::titles_available()
}

/// The editor, in place of Translate's rows.
pub fn editor(state: &State) -> Element<'_, Message> {
    let page = &state.glossaries;
    let m = |x: Msg| Message::Glossaries(x);
    let Some((slug, g)) =
        (page.editing.as_deref()).and_then(|e| page.all.iter().find(|(s, _)| s == e))
    else {
        return column![].into();
    };
    let back = button(
        crate::dir_row![
            icon(Icon::Back.directed(), size::ICON_SMALL),
            label(crate::shared::app_language::tr(S::nav_translate))
        ]
        .spacing(space::GAP)
        .align_y(iced::Center),
        Kind::Quiet,
        Some(m(Msg::Back)),
    );
    let crumb = crate::dir_row![
        back,
        sub(Role::ROW_DESC, "/"),
        crate::shared::kit::text::text(Role::ROW_TITLE, t!(gl_crumb))
    ]
    .spacing(space::GAP)
    .align_y(iced::Center);
    let mut rows = column![container(crumb).padding([space::ROW_Y, 0.0])];
    // the glossary and More
    let pick = dropdown(
        &page.choices[1..],
        page.choices.iter().find(|c| c.slug == *slug).cloned(),
        "",
        move |c: Choice| m(Msg::Pick(c.slug)),
    );
    let more = menu_button(
        vec![
            (t!(gl_new).into(), false, m(Msg::New)),
            (t!(gl_rename).into(), false, m(Msg::Renaming(true))),
            (t!(gl_import).into(), false, m(Msg::Import)),
            (t!(gl_export).into(), false, m(Msg::Export)),
            (t!(gl_delete_glossary).into(), true, m(Msg::AskDelete(true))),
        ],
        MENU_W,
    );
    rows = rows.push(setting_row::row(
        title(t!(gl_row)),
        "",
        setting_row::field_with(pick, [more]),
    ));
    let b = |s: &'static str, on: Msg| button(label(s), Kind::Quiet, Some(m(on)));
    let line: Option<Element<'_, Message>> = match (&page.renaming, page.confirm_delete) {
        (Some(name), _) => Some(
            crate::dir_row![
                input(t!(gl_name), name, false, name.trim().is_empty())
                    .on_input(move |s| m(Msg::Name(s)))
                    .on_submit(m(Msg::Renaming(false)))
                    .width(Fill),
                b(t!(c_save), Msg::Renaming(false))
            ]
            .spacing(space::GAP)
            .into(),
        ),
        (None, true) => Some(
            crate::dir_row![
                sub(Role::ROW_DESC, fmt(S::gl_delete_q, &[&g.name])),
                Space::new().width(Fill),
                b(t!(c_cancel), Msg::AskDelete(false)),
                button(label(t!(gl_delete)), Kind::Remove, Some(m(Msg::Delete)))
            ]
            .spacing(space::GAP)
            .align_y(iced::Center)
            .into(),
        ),
        (None, false) => None,
    };
    if let Some(line) = line {
        rows = rows.push(container(line).padding([space::ROW_Y, 0.0]));
    }
    if let Some(done) = &page.summary {
        rows =
            rows.push(container(muted(Role::ROW_DESC, done.as_str())).padding([space::ROW_Y, 0.0]));
    }
    if let Some(p) = &page.problem {
        rows = rows.push(crate::shared::kit::text::error(Role::ROW_DESC, p.as_str()));
    }
    if titles() {
        let field = input("", &g.windows.join(", "), false, false)
            .on_input(move |s| m(Msg::Windows(s)))
            .width(Fill);
        rows = rows.push(setting_row::row(
            title(t!(gl_windows)),
            t!(gl_windows_desc),
            field,
        ));
    }
    // search and Add term
    let find = search(t!(gl_search), &page.query)
        .on_input(move |s| m(Msg::Query(s)))
        .width(Fill);
    let add = button(label(t!(gl_add)), Kind::Elevated, Some(m(Msg::Add)));
    let mut line: Vec<Element<'_, Message>> = vec![find.into(), add];
    if page.undo.is_some() {
        line.push(muted(Role::ROW_DESC, t!(gl_removed)).into());
        line.push(button(label(t!(gl_undo)), Kind::Quiet, Some(m(Msg::Undo))));
    }
    rows = rows.push(
        container(
            crate::shared::kit::dir::row(line)
                .spacing(space::GAP)
                .align_y(iced::Center),
        )
        .padding([space::ROW_Y, 0.0]),
    );
    // the table: On screen | In <language>
    let current = TARGETS.iter().find(|l| l.0.code == page.shown).copied();
    let header = crate::dir_row![
        container(muted(Role::ROW_DESC, t!(gl_col_screen))).width(Fill),
        container(dropdown(&TARGETS, current, "", move |l: Named| m(
            Msg::Lang(l)
        )))
        .width(Fill),
        Space::new().width(table::CROSS)
    ]
    .spacing(space::GAP)
    .align_y(iced::Center);
    let q = page.query.trim().to_lowercase();
    let code = code_of(&page.shown).unwrap_or("");
    let matching = g.terms.iter().enumerate().filter(|(_, t)| {
        q.is_empty()
            || t.text.to_lowercase().contains(&q)
            || t.translations
                .get(code)
                .is_some_and(|x| x.to_lowercase().contains(&q))
    });
    let mut cells: Vec<Element<'_, Message>> = Vec::new();
    let mut hidden = 0usize;
    for (i, t) in matching {
        if cells.len() >= SHOWN {
            hidden += 1;
            continue;
        }
        let text = input("", &t.text, false, false)
            .on_input(move |s| m(Msg::Text(i, s)))
            .width(Fill);
        let tr = input(
            t!(gl_kept),
            t.translations.get(code).map_or("", String::as_str),
            false,
            false,
        )
        .on_input(move |s| m(Msg::Trans(i, s)))
        .width(Fill);
        let hot = page.hover == Some(i);
        let x = container(table::cross(hot, m(Msg::Remove(i)))).width(table::CROSS);
        let row = table::row(
            crate::dir_row![text, tr, x]
                .spacing(space::GAP)
                .align_y(iced::Center),
            hot,
        );
        cells.push(
            mouse_area(row)
                .on_enter(m(Msg::Hover(Some(i))))
                .on_exit(m(Msg::Hover(None)))
                .into(),
        );
    }
    if g.terms.is_empty() {
        cells.push(table::row(muted(Role::ROW_DESC, t!(gl_empty)), false));
    }
    if hidden > 0 {
        let more = muted(Role::ROW_DESC, fmt(S::gl_more_rows, &[&hidden]));
        cells.push(table::row(more, false));
    }
    rows.push(container(table::table(header.into(), cells)).padding([space::ROW_Y, 0.0]))
        .push(muted(Role::ROW_DESC, t!(gl_hint)))
        .into()
}

/// The "More" menu's width.
const MENU_W: f32 = 220.0;

/// The language table's code for `shown` (a config's "pt-BR" is "pt").
fn code_of(shown: &str) -> Option<&'static str> {
    let base = shown.split(['-', '_']).next().unwrap_or_default();
    let find = |c: &str| (TARGETS.iter().map(|l| l.0.code)).find(|x| x.eq_ignore_ascii_case(c));
    find(shown).or_else(|| find(base))
}
