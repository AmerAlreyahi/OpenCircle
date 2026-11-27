//! The pill's glossary button and its list (GLOSSARY_SPEC.md "Translate
//! pill"): one more icon button after Snip (accent while a glossary is
//! active, its name in the tooltip), and a list hanging from it: None, the
//! glossaries, New glossary, Edit glossaries…. The pill's width stays
//! fixed: the button is always there. The user's pick is saved in
//! `translate.glossary` (remembered; where the OS tells the focused
//! window's title, a glossary's `windows` list picks it by itself first).
use iced::widget::tooltip::Position;
use iced::widget::{column, container, stack};
use iced::{Center, Element, Fill, Vector};

use super::Msg;
use crate::features::translate::glossary::{Glossary, screen, store, suggest};
use crate::features::translate::screens::language_picker as picker;
use crate::shared::app_language::{S, fmt};
use crate::shared::kit::chip;
use crate::shared::kit::dir;
use crate::shared::kit::field::input;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{State, pill_icon, quiet_close};
/// The glossary menu's width (design `.mn` 272).
const MENU_W: f32 = 272.0;
use crate::shared::kit::option_list::{
    floating_item, floating_menu_width, floating_separator, group,
};
use crate::shared::kit::text::text as label;
use crate::shared::kit::tokens::{Role, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::platform::capture::window_rect;
use crate::t;

#[derive(Debug, Clone)]
pub enum GlossaryMsg {
    /// The button: open the list (or close it).
    Toggle,
    /// A glossary (its file name); "" = none.
    Pick(String),
    /// New glossary / Edit glossaries…: the editor is Settings > Translate.
    Edit,
    /// A suggested name: keep it as it is (the pill saves it and translates
    /// again), ask for its translation, or never suggest it again.
    Keep(String),
    Ask(String),
    Typed(String),
    /// The typed translation of the name being asked about.
    SaveAs,
    Reject(String),
}

/// Suggestions the list shows at once (the rest follow as these are answered).
const SHOWN: usize = 5;

/// What the list shows.
#[derive(Debug, Clone, Default)]
pub struct Menu {
    open: bool,
    /// (slug, name, number of terms), by name.
    items: Vec<(String, String, usize)>,
    /// The active glossary's slug, "" = none.
    pub active: String,
    /// The button's tooltip (names the active glossary).
    tip: String,
    /// Names seen on several screens, to review (`suggest::pending`).
    names: Vec<String>,
    /// The focused window's title (Windows, X11): names the glossary a first
    /// accepted name creates; `None` on Wayland.
    pub title: Option<String>,
    /// The name whose translation is being typed, and what is typed.
    ask: Option<(String, String)>,
}

impl Menu {
    /// The files now, with the glossary this capture uses (`screen::resolved`:
    /// the pick, the window in front's own, else the saved pick on Wayland).
    pub fn load(chosen: &str) -> Menu {
        let all = store::load_all(&store::dir());
        // (a pick whose file is gone is none)
        let active = screen::resolved(&all, chosen).unwrap_or("");
        let mut menu = Menu {
            open: false,
            active: active.to_string(),
            items: (all.iter())
                .map(|(s, g)| (s.clone(), g.name.clone(), g.terms.len()))
                .collect(),
            tip: String::new(),
            names: Vec::new(),
            title: window_rect::captured()
                .map(|f| f.title)
                .filter(|t| !t.trim().is_empty()),
            ask: None,
        };
        menu.names = pending(&all, &menu.active, menu.title.as_deref());
        menu.words();
        menu
    }

    /// Reads the counted names again (a screen was just translated).
    pub fn refresh(&mut self) {
        self.names = pending(
            &store::load_all(&store::dir()),
            &self.active,
            self.title.as_deref(),
        );
        self.words();
    }

    fn words(&mut self) {
        let tip = match self.name() {
            Some(n) => fmt(S::gl_tip_on, &[&n]),
            None => t!(gl_tip).to_string(),
        };
        self.tip = match self.names.len() {
            0 => tip,
            n => fmt(S::gl_names_tip, &[&tip, &n]),
        };
    }

    /// `word` is answered: it leaves the list.
    pub fn answered(&mut self, word: &str) {
        self.names.retain(|n| n != word);
        self.ask = None;
        self.words();
    }

    /// The typed translation to save, with its name (`SaveAs`).
    pub fn asked(&self) -> Option<&(String, String)> {
        self.ask.as_ref()
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// `true` = the active glossary changed (translate again).
    pub fn update(&mut self, msg: GlossaryMsg) -> Option<bool> {
        match msg {
            GlossaryMsg::Toggle => {
                self.open = !self.open;
                Some(false)
            }
            GlossaryMsg::Pick(slug) => {
                self.open = false;
                let changed = slug != self.active;
                if changed {
                    screen::pick(&slug);
                    self.active = slug.clone();
                    self.ask = None;
                    self.refresh();
                    #[cfg(not(test))]
                    std::thread::spawn(move || {
                        if let Err(e) =
                            crate::shared::config::update(|c| c.translate.glossary = slug)
                        {
                            crate::log!("[opencircle] could not save the config: {e}");
                        }
                    });
                }
                Some(changed)
            }
            GlossaryMsg::Edit => {
                self.open = false;
                None
            }
            GlossaryMsg::Ask(word) => {
                self.ask = Some((word, String::new()));
                Some(false)
            }
            GlossaryMsg::Typed(text) => {
                if let Some((_, t)) = &mut self.ask {
                    *t = text;
                }
                Some(false)
            }
            GlossaryMsg::Reject(word) => {
                if let Err(e) = suggest::reject(
                    &store::dir(),
                    &suggest::game_key(&self.active, self.title.as_deref()),
                    &word,
                ) {
                    crate::log!("[opencircle] could not remember the rejected name: {e}");
                }
                self.answered(&word);
                Some(false)
            }
            // (the pill saves the term: it knows the target language)
            GlossaryMsg::Keep(_) | GlossaryMsg::SaveAs => Some(false),
        }
    }

    fn name(&self) -> Option<&str> {
        (self.items.iter())
            .find(|(s, ..)| *s == self.active)
            .map(|(_, n, _)| n.as_str())
    }

    /// The pill's button: accent while a glossary is active. Only on a game
    /// screen; elsewhere its slot stays empty (the pill never changes size).
    pub fn button(&self) -> Element<'_, Msg> {
        if !screen::is_game() {
            let side = crate::shared::kit::tokens::size::ICON_BUTTON;
            return iced::widget::Space::new().width(side).height(side).into();
        }
        let on = self.name().is_some();
        let b = pill_icon(
            Icon::Glossary.glyph(),
            State {
                active: on || self.open,
                ..State::default()
            },
            Some(Msg::Glossary(GlossaryMsg::Toggle)),
        );
        let b = hint(b, &self.tip, Why::Icon, Position::Bottom);
        if self.names.is_empty() {
            return b;
        }
        // a small dot with the count at the button's corner (never a pop-up)
        let dot = chip::count(self.names.len());
        let dot = match dir::rtl() {
            true => container(dot).align_left(Fill),
            false => container(dot).align_right(Fill),
        };
        stack![b, dot.align_top(Fill)].into()
    }

    /// Where the list hangs, from the pill's top-left: under the buttons,
    /// its end edge on the pill's (mirrored right to left).
    pub fn at(&self) -> Option<Vector> {
        self.open.then(|| {
            let x = match dir::rtl() {
                true => 0.0,
                false => picker::PILL.width - MENU_W,
            };
            Vector::new(x, picker::PILL.height + picker::LIST_GAP)
        })
    }

    pub fn list(&self) -> Option<Element<'_, Msg>> {
        self.open.then(|| {
            let pick = |s: &str| Msg::Glossary(GlossaryMsg::Pick(s.to_string()));
            let mut rows = vec![floating_item(
                t!(gl_none),
                false,
                None,
                false,
                self.active.is_empty(),
                pick(""),
            )];
            if !self.items.is_empty() {
                rows.push(floating_separator());
            }
            for (slug, name, n) in &self.items {
                let current = *slug == self.active;
                rows.push(floating_item(
                    name,
                    false,
                    Some(n.to_string()),
                    false,
                    current,
                    pick(slug),
                ));
            }
            if !self.names.is_empty() {
                rows.push(floating_separator());
                rows.push(group(t!(gl_names), 24.0));
                rows.extend(self.names.iter().take(SHOWN).map(|n| self.suggestion(n)));
            }
            rows.push(floating_separator());
            let edit = Msg::Glossary(GlossaryMsg::Edit);
            rows.push(floating_item(
                t!(gl_new),
                false,
                None,
                false,
                false,
                edit.clone(),
            ));
            rows.push(floating_item(t!(gl_edit), false, None, false, false, edit));
            floating_menu_width(MENU_W, rows)
        })
    }
}

impl Menu {
    /// One suggested name: its three answers, or (after Translate as…) the
    /// field for the translation.
    fn suggestion<'a>(&'a self, name: &'a str) -> Element<'a, Msg> {
        fn msg(m: GlossaryMsg) -> Msg {
            Msg::Glossary(m)
        }
        let answers: Element<'a, Msg> = match &self.ask {
            Some((w, typed)) if w == name => dir::row([
                input(t!(gl_translate_as), typed, false, false)
                    .id(ASK)
                    .on_input(|s| msg(GlossaryMsg::Typed(s)))
                    .on_submit(msg(GlossaryMsg::SaveAs))
                    .width(Fill)
                    .into(),
                chip::action(t!(gl_sug_save).to_string(), msg(GlossaryMsg::SaveAs)),
            ])
            .spacing(space::GAP)
            .align_y(Center)
            .into(),
            _ => dir::row([
                chip::action(t!(gl_keep).to_string(), msg(GlossaryMsg::Keep(name.into()))),
                chip::action(
                    t!(gl_translate_as).to_string(),
                    msg(GlossaryMsg::Ask(name.into())),
                ),
            ])
            .spacing(space::GAP)
            .into(),
        };
        // (the name, and a small x at its end: never suggested again)
        let reject = quiet_close(false, Some(msg(GlossaryMsg::Reject(name.into()))));
        let title = dir::row([
            container(label(Role::CONTROL, name)).width(Fill).into(),
            hint(reject, t!(gl_reject), Why::Icon, Position::Bottom),
        ])
        .align_y(Center);
        container(column![title, answers].spacing(space::GAP))
            .padding([4.0, 10.0])
            .into()
    }
}

/// The field of "Translate as…".
pub const ASK: &str = "glossary-name-translation";

/// The names to review for glossary `slug` of `all`, or (none yet) for the
/// game the window `title` names.
fn pending(all: &[(String, Glossary)], slug: &str, title: Option<&str>) -> Vec<String> {
    let g = all.iter().find(|(s, _)| s == slug).map(|(_, g)| g);
    let key = suggest::game_key(slug, title);
    suggest::pending(&store::dir(), &key, g.unwrap_or(&Glossary::default()))
}

/// A glossary of the card's "In" dropdown.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick(pub String, pub String);

impl std::fmt::Display for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// The word card's "Add to glossary" form (GLOSSARY_SPEC.md "Word card").
#[derive(Debug, Clone, PartialEq)]
pub struct AddForm {
    pub term: String,
    /// Keep as is (else Always as `always`).
    pub keep: bool,
    pub always: String,
    /// The glossary's slug; "" = a new one named `name`.
    pub into: String,
    pub name: String,
    /// The window a new glossary is for ("Use for windows named").
    pub window: Option<String>,
    /// The glossaries to put it in (empty: a new one named `name`).
    pub picks: Vec<Pick>,
}

#[derive(Debug, Clone)]
pub enum AddMsg {
    Term(String),
    Keep(bool),
    Always(String),
    Into(Pick),
    Name(String),
    Cancel,
    Save,
}

pub use crate::features::translate::glossary::suggest::names;

/// The first name of `original` ("the Vyrnhold gate"), else the whole text.
pub fn guess(original: &str) -> String {
    let words: Vec<&str> = original.split_whitespace().collect();
    let name = (names(&words).iter())
        .position(|n| *n)
        .map(|i| words[i].trim_matches(|c: char| !c.is_alphanumeric()));
    (name.unwrap_or(original.trim()).chars())
        .take(store::MAX_TERM_CHARS)
        .collect()
}

impl AddForm {
    pub fn new(original: &str, into: &str, title: Option<&str>, picks: Vec<Pick>) -> AddForm {
        AddForm {
            picks,
            term: guess(original),
            keep: true,
            always: String::new(),
            into: into.to_string(),
            name: title.unwrap_or(t!(gl_default_name)).to_string(),
            window: None,
        }
    }

    /// Writes the term into its glossary (a new one when none was picked),
    /// in `dir`. A term already there (any case) is replaced. Returns the
    /// glossary's (slug, name).
    pub fn save(&self, dir: &std::path::Path, target: &str) -> Result<(String, String), String> {
        use crate::features::translate::{Glossary, Term};
        let term = self.term.trim();
        if term.is_empty() {
            return Err(t!(gl_name).to_string());
        }
        let (slug, mut g) = match store::load(dir, &self.into) {
            Some(g) => (self.into.clone(), g),
            None => {
                let name = self.name.trim();
                let name = if name.is_empty() {
                    t!(gl_default_name)
                } else {
                    name
                };
                let g = Glossary {
                    name: name.to_string(),
                    windows: self.window.iter().cloned().collect(),
                    ..Glossary::default()
                };
                (store::free_slug(dir, name), g)
            }
        };
        let mut t = Term {
            text: term.to_string(),
            ..Term::default()
        };
        let fixed = self.always.trim();
        if !self.keep && !fixed.is_empty() {
            let code = crate::features::translate::lang::find(target)
                .ok_or("language")?
                .code;
            t.translations.insert(code.to_string(), fixed.to_string());
        }
        g.terms.retain(|o| !o.text.eq_ignore_ascii_case(&t.text));
        g.terms.push(t);
        store::save(dir, &slug, &g).map_err(|e| e.to_string())?;
        Ok((slug, g.name))
    }
}

impl Menu {
    /// The glossaries as the card's dropdown picks them.
    pub fn picks(&self) -> Vec<Pick> {
        (self.items.iter())
            .map(|(s, n, _)| Pick(s.clone(), n.clone()))
            .collect()
    }

    /// Reads the files again and makes `slug` the active glossary (after the
    /// card saved a term into it).
    pub fn activate(&mut self, slug: &str) {
        screen::pick(slug);
        let reloaded = Menu::load(slug);
        self.items = reloaded.items;
        self.active = reloaded.active;
        self.words();
        #[cfg(not(test))]
        {
            let slug = slug.to_string();
            std::thread::spawn(move || {
                if let Err(e) = crate::shared::config::update(|c| c.translate.glossary = slug) {
                    crate::log!("[opencircle] could not save the config: {e}");
                }
            });
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// A menu of `names` (each with 12 terms), `active` picked (tests).
    #[cfg(test)]
    pub fn sample(names: &[&str], active: &str, open: bool) -> Menu {
        let mut m = Menu {
            open,
            items: (names.iter())
                .map(|n| (n.to_lowercase().replace(' ', "-"), n.to_string(), 12))
                .collect(),
            active: active.into(),
            tip: String::new(),
            names: Vec::new(),
            title: None,
            ask: None,
        };
        m.words();
        m
    }

    /// `self` with suggested `names` (tests); `ask` = the one being typed.
    #[cfg(test)]
    pub fn with_names(mut self, names: &[&str], ask: Option<(&str, &str)>) -> Menu {
        self.names = names.iter().map(|n| n.to_string()).collect();
        self.ask = ask.map(|(a, b)| (a.into(), b.into()));
        self.words();
        self
    }
}
