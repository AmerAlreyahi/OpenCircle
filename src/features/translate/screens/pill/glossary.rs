//! The pill's glossary button and its list (GLOSSARY_SPEC.md "Translate
//! pill"): one more icon button after Snip (accent while a glossary is
//! active, its name in the tooltip), and a list hanging from it: None, the
//! glossaries, New glossary, Edit glossaries…. The pill's width stays
//! fixed: the button is always there. The user's pick is saved in
//! `translate.glossary` (remembered; where the OS tells the focused
//! window's title, a glossary's `windows` list picks it by itself first).
use iced::widget::tooltip::Position;
use iced::{Element, Vector};

use super::Msg;
use crate::features::translate::glossary::store;
use crate::features::translate::screens::language_picker as picker;
use crate::shared::app_language::{S, fmt};
use crate::shared::kit::dir;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button::{State, icon_button};
use crate::shared::kit::option_list::{MENU_WIDTH, item, menu, separator};
use crate::shared::kit::tooltip::{Why, hint};
use crate::t;

#[derive(Debug, Clone)]
pub enum GlossaryMsg {
    /// The button: open the list (or close it).
    Toggle,
    /// A glossary (its file name); "" = none.
    Pick(String),
    /// New glossary / Edit glossaries…: the editor is Settings > Translate.
    Edit,
}

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
}

impl Menu {
    /// The files now, with the saved pick; the window title (where the OS
    /// gives one) picks by itself.
    pub fn load(chosen: &str, title: Option<&str>) -> Menu {
        let all = store::load_all(&store::dir());
        let active = store::for_window(&all, title).unwrap_or(chosen);
        // (a pick whose file is gone is none)
        let active = all.iter().find(|(s, _)| s == active).map_or("", |(s, _)| s);
        let mut menu = Menu {
            open: false,
            active: active.to_string(),
            items: (all.iter())
                .map(|(s, g)| (s.clone(), g.name.clone(), g.terms.len()))
                .collect(),
            tip: String::new(),
        };
        menu.words();
        menu
    }

    fn words(&mut self) {
        self.tip = match self.name() {
            Some(n) => fmt(S::gl_tip_on, &[&n]),
            None => t!(gl_tip).to_string(),
        };
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
                    self.active = slug.clone();
                    self.words();
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
        }
    }

    fn name(&self) -> Option<&str> {
        (self.items.iter())
            .find(|(s, ..)| *s == self.active)
            .map(|(_, n, _)| n.as_str())
    }

    /// The pill's button: accent while a glossary is active.
    pub fn button(&self) -> Element<'_, Msg> {
        let on = self.name().is_some();
        let b = icon_button(
            Icon::Glossary.glyph(),
            State {
                active: on,
                ..State::default()
            },
            Some(Msg::Glossary(GlossaryMsg::Toggle)),
        );
        hint(b, &self.tip, Why::Icon, Position::Bottom)
    }

    /// Where the list hangs, from the pill's top-left: under the buttons,
    /// its end edge on the pill's (mirrored right to left).
    pub fn at(&self) -> Option<Vector> {
        self.open.then(|| {
            let x = match dir::rtl() {
                true => 0.0,
                false => picker::PILL.width - MENU_WIDTH,
            };
            Vector::new(x, picker::PILL.height + picker::LIST_GAP)
        })
    }

    pub fn list(&self) -> Option<Element<'_, Msg>> {
        self.open.then(|| {
            let pick = |s: &str| Msg::Glossary(GlossaryMsg::Pick(s.to_string()));
            let mut rows = vec![item(
                t!(gl_none),
                self.active.is_empty(),
                None,
                false,
                pick(""),
            )];
            if !self.items.is_empty() {
                rows.push(separator());
            }
            for (slug, name, n) in &self.items {
                rows.push(item(
                    name,
                    *slug == self.active,
                    Some(n.to_string()),
                    false,
                    pick(slug),
                ));
            }
            rows.push(separator());
            let edit = Msg::Glossary(GlossaryMsg::Edit);
            rows.push(item(t!(gl_new), false, None, false, edit.clone()));
            rows.push(item(t!(gl_edit), false, None, false, edit));
            menu(rows)
        })
    }
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

/// The first capitalised word of `original` that doesn't start it (a name:
/// "the Vyrnhold gate"), else the whole text.
pub fn guess(original: &str) -> String {
    let words: Vec<&str> = original.split_whitespace().collect();
    let name = (words.iter().skip(1))
        .find(|w| {
            w.starts_with(char::is_uppercase) && w.chars().filter(|c| c.is_alphabetic()).count() > 1
        })
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()));
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
        let reloaded = Menu::load(slug, None);
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
        };
        m.words();
        m
    }
}
