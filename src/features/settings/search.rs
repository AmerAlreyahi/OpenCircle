//! Settings search (F13, Ctrl+K; Ctrl+F too): over the page titles and every row's title and
//! description, in the app's language. The index is read from the pages themselves
//! (`setting_row::harvest`: no second list of words to keep), built once when the
//! search opens, for the pages as they show now and for the rows only a choice
//! reveals (another provider, a Labs switch). A hit lists "row title . page"; Enter
//! or a click opens the page and marks the row, or the choice that reveals it.
use super::pages;
use super::state::{Section, State};
use crate::shared::app_language::tr;
use crate::shared::kit::setting_row;

/// The search field's id (it takes the focus on Ctrl+K / Ctrl+F).
pub const FIELD: &str = "settings-search";
/// The most hits listed.
pub const MAX_HITS: usize = 8;
/// The page's scroll area (`operation::snap_to`).
pub const PAGE: &str = "settings-page-scroll";
/// The marked row stays marked this long.
pub const MARK: std::time::Duration = std::time::Duration::from_millis(1500);

/// One thing the search can find.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The page a hit opens.
    pub section: Section,
    /// The page's title (the breadcrumb).
    pub page: &'static str,
    /// The row's title; the page's own title for a page entry.
    pub title: String,
    pub desc: String,
    /// A page, not a row.
    pub is_page: bool,
    /// The row a hit marks: the row itself, or the choice that reveals it (`None`: a page).
    pub mark: Option<String>,
    /// Where the row is down its page, 0 (first row) to 1 (last): the scroll to it.
    pub at: f32,
}

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    index: Vec<Entry>,
    /// The hit Enter opens.
    pub at: usize,
}

/// What Arabic search ignores: the vowel marks and the tatweel; the alef forms,
/// ta marbuta and alef maqsura match their plain letters; case is ignored.
pub fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}'))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

impl Search {
    /// The search over what `state`'s pages say now.
    pub fn open(state: &State) -> Search {
        Search {
            index: index(state),
            ..Search::default()
        }
    }

    /// Every hit for the query: every word of it in the row's title, description or
    /// page; title matches first (the page order within each).
    pub fn all_hits(&self) -> Vec<&Entry> {
        let words: Vec<String> = self.query.split_whitespace().map(fold).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let has = |text: &str| {
            let text = fold(text);
            words.iter().all(|w| text.contains(w))
        };
        let mut found: Vec<(u8, &Entry)> = self
            .index
            .iter()
            .filter_map(|e| {
                let rank = if has(&e.title) {
                    0
                } else if has(&e.desc) {
                    1
                } else if has(&format!("{} {} {}", e.page, e.title, e.desc)) {
                    2
                } else {
                    return None;
                };
                Some((rank, e))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, e)| e).collect()
    }

    /// The hits listed (the first `MAX_HITS`).
    pub fn hits(&self) -> Vec<&Entry> {
        self.all_hits().into_iter().take(MAX_HITS).collect()
    }

    /// Up (-1) or down (+1) the list.
    pub fn step(&mut self, by: i32) {
        let last = self.hits().len().saturating_sub(1);
        self.at = (self.at as i32 + by).clamp(0, last as i32) as usize;
    }

    /// The query changed: the first hit is the one Enter opens.
    pub fn typed(&mut self, query: String) {
        self.query = query;
        self.at = 0;
    }

    /// The `i`th hit (`None`: no such hit).
    pub fn hit(&self, i: usize) -> Option<&Entry> {
        self.hits().get(i).copied()
    }
}

/// A keyring that holds nothing: the throwaway states the index reads never touch
/// the user's keys.
struct NoKeys;

impl<K> crate::shared::secrets::KeyStore<K> for NoKeys {
    fn is_set(&self, _: K) -> Result<bool, crate::shared::secrets::SecretsError> {
        Ok(false)
    }
    fn read(
        &self,
        _: K,
    ) -> Result<Option<crate::shared::secrets::Secret>, crate::shared::secrets::SecretsError> {
        Ok(None)
    }
    fn set(
        &self,
        _: K,
        _: &crate::shared::secrets::Secret,
    ) -> Result<(), crate::shared::secrets::SecretsError> {
        Ok(())
    }
    fn delete(&self, _: K) -> Result<(), crate::shared::secrets::SecretsError> {
        Ok(())
    }
}

/// The page entries and the rows of the pages `state` shows, each row marked itself.
pub(super) fn rows(state: &State) -> Vec<Entry> {
    let mut all = Vec::new();
    for page in pages::visible(&state.draft.labs) {
        let name = tr(page.title);
        all.push(Entry {
            section: page.id,
            page: name,
            title: name.to_string(),
            desc: String::new(),
            is_page: true,
            mark: None,
            at: 0.0,
        });
        let ((), found) = setting_row::harvest(|| drop((page.view)(state)));
        // (a row built without a title has none: it is not listed)
        let found: Vec<_> = found.into_iter().filter(|(t, _)| !t.is_empty()).collect();
        let last = found.len().saturating_sub(1).max(1) as f32;
        all.extend(
            found
                .into_iter()
                .enumerate()
                .map(|(i, (title, desc))| Entry {
                    section: page.id,
                    page: name,
                    mark: Some(title.clone()),
                    title,
                    desc,
                    is_page: false,
                    at: i as f32 / last,
                }),
        );
    }
    all
}

/// Every page and every row: as the pages show now, then what only a choice reveals.
fn index(state: &State) -> Vec<Entry> {
    use crate::shared::config::Provider;
    // Building a State applies the language its config derives, which can differ from what the
    // window speaks: every build puts the window's own language back (W112: search turned the
    // Settings window English), and even the first rows come from a State made now, so the quick
    // actions' names are in that language too.
    let spoken = crate::shared::app_language::snapshot();
    let keep = || crate::shared::app_language::restore(spoken);
    let fresh = |cfg: crate::shared::config::Config| {
        let mut s = State::new(cfg, Vec::new(), None, Box::new(NoKeys));
        keep();
        s.refresh_quick_text();
        s
    };
    let mut all = rows(&fresh(state.draft.clone()));
    let provider_row = {
        let ((), found) = setting_row::harvest(|| drop(pages::rows::provider_row(state)));
        found.into_iter().next().map(|(t, _)| t)
    };
    let variant = |change: &dyn Fn(&mut crate::shared::config::Config)| {
        let mut cfg = state.draft.clone();
        change(&mut cfg);
        fresh(cfg)
    };
    let variants = [
        // Labs features on: their pages and rows (listed under Labs, marking their switch)
        (variant(&|c| c.labs = super::labs::Labs::all_on()), None),
        // another provider: its key and model rows (marking the provider choice)
        (
            variant(&|c| c.ai.provider = Provider::Anthropic),
            provider_row.clone(),
        ),
        (
            variant(&|c| c.ai.provider = Provider::OpenaiCompatible),
            provider_row,
        ),
    ];
    // the state the user is in is back (the throwaway states set the theme and language)
    for (other, parent) in &variants {
        for mut e in rows(other) {
            if all
                .iter()
                .any(|x| x.section == e.section && x.title == e.title && x.is_page == e.is_page)
            {
                continue;
            }
            let shown = pages::visible(&state.draft.labs).any(|p| p.id == e.section);
            if !shown {
                // a Labs feature's own page is not in the sidebar: its switch is under Labs
                let switch = e
                    .is_page
                    .then(|| e.title.clone())
                    .or_else(|| Some(e.page.to_string()));
                (e.section, e.page, e.mark) =
                    (Section::Labs, tr(pages::of(Section::Labs).title), switch);
            } else if !e.is_page {
                e.mark = parent.clone().or(e.mark);
            }
            all.push(e);
        }
    }
    crate::shared::theme::apply(state.draft.theme);
    keep();
    all
}

#[cfg(test)]
impl Search {
    pub(super) fn index_titles(&self) -> Vec<String> {
        self.index.iter().map(|e| e.title.clone()).collect()
    }
}
