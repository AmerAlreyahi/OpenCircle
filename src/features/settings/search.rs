//! Settings search (F13, Ctrl+F): over the page titles and every row's title and
//! description, in the app's language. The index is read from the pages themselves
//! (`setting_row::harvest`: no second list of words to keep), built once when the
//! search opens. A hit lists "row title . page"; Enter or a click opens the page and
//! marks the row.
use super::pages;
use super::state::{Section, State};
use crate::shared::app_language::tr;
use crate::shared::kit::setting_row;

/// The search field's id (it takes the focus on Ctrl+F).
pub const FIELD: &str = "settings-search";
/// The most hits listed.
pub const MAX_HITS: usize = 8;

/// One thing the search can find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub section: Section,
    /// The page's title (the breadcrumb).
    pub page: &'static str,
    /// The row's title; the page's own title for a page entry.
    pub title: String,
    pub desc: String,
    /// A page, not a row.
    pub is_page: bool,
}

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    index: Vec<Entry>,
    /// The hit Enter opens.
    pub at: usize,
}

impl Search {
    /// The search over what `state`'s pages say now.
    pub fn open(state: &State) -> Search {
        Search {
            index: index(state),
            ..Search::default()
        }
    }

    /// The hits for the query: every word of it in the row's title, description or
    /// page; title matches first (the page order within each).
    pub fn hits(&self) -> Vec<&Entry> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        let has = |text: &str| {
            let text = text.to_lowercase();
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
        found.into_iter().map(|(_, e)| e).take(MAX_HITS).collect()
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

/// Every page and every row the pages build, for the pages shown now.
fn index(state: &State) -> Vec<Entry> {
    let mut all = Vec::new();
    for page in pages::visible(&state.draft.labs) {
        let name = tr(page.title);
        all.push(Entry {
            section: page.id,
            page: name,
            title: name.to_string(),
            desc: String::new(),
            is_page: true,
        });
        let ((), rows) = setting_row::harvest(|| drop((page.view)(state)));
        all.extend(rows.into_iter().map(|(title, desc)| Entry {
            section: page.id,
            page: name,
            title,
            desc,
            is_page: false,
        }));
    }
    all
}
