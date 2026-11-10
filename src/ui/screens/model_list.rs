//! The Model list (`kit::dropdown::combo`), shared by Settings > Ask AI and
//! the AI chat's drawer: the provider's models (`ai::models`: built in, plus
//! what "Load models" brought), the search typed in it (a typed id that is
//! in no list is offered as is), and the one network call, "Load models from
//! <provider>", which runs only on its click, on a worker thread.
use std::fmt;
use std::sync::Arc;

use iced::Element;
use iced::widget::column;

use crate::ai::models::{self, Model};
use crate::ai::providers::Preset;
use crate::config::AiConfig;
use crate::secrets::{AiKey, KeyStore};
use crate::ui::kit::dropdown::{Search, combo};
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::tokens::{Role, space};

/// The search field's widget id, and the list's own (`kit::dropdown::open`).
pub const SEARCH: &str = "model-search";

/// The list as a window keeps it (made again when the provider changes or a
/// list arrives).
#[derive(Debug, Default)]
pub struct ModelList {
    pub models: Vec<Model>,
    pub query: String,
    /// "Load models from Google"; "Pricing is set by Google" when a row has
    /// a price tag.
    pub load: String,
    pub pricing: Option<String>,
    pub loading: bool,
    /// What the last "Load models" said: how many came, or why none did.
    pub said: Option<Result<String, String>>,
}

/// A row: its id, its price tag, and whether it is the typed id itself.
pub type Choice<'a> = (&'a str, Option<&'a str>, bool);

/// What the list sends: the menu opened, the search changed, Enter in it, a
/// row picked, "Load models".
pub struct On<M> {
    pub opened: M,
    pub query: fn(String) -> M,
    pub first: M,
    pub pick: fn(String) -> M,
    pub load: M,
}

impl ModelList {
    /// `ai`'s provider's list (none: the browser), nothing typed, nothing said.
    pub fn of(ai: &AiConfig) -> ModelList {
        let mut list = ModelList::default();
        list.refresh(ai);
        list
    }

    /// The provider's models again (a list loaded since joins them).
    pub fn refresh(&mut self, ai: &AiConfig) {
        let Some(preset) = Preset::of(ai) else {
            *self = ModelList::default();
            return;
        };
        self.models = models::known(&preset, ai);
        self.load = format!("Load models from {}", preset.name);
        self.pricing =
            (self.models.iter().any(|m| m.tag.is_some())).then(|| models::pricing_note(&preset));
    }

    /// The rows for the search typed: the models that contain it (any case),
    /// then the typed id itself unless a model has exactly that id.
    pub fn choices(&self) -> Vec<Choice<'_>> {
        let q = self.query.trim();
        let low = q.to_lowercase();
        let mut out: Vec<Choice<'_>> = (self.models.iter())
            .filter(|m| m.id.to_lowercase().contains(&low))
            .map(|m| (m.id.as_str(), m.tag.as_deref(), false))
            .collect();
        if !q.is_empty() && self.models.iter().all(|m| m.id != q) {
            out.push((q, None, true));
        }
        out
    }

    /// `id` picked: the model it sets (trimmed); the search empties.
    pub fn pick(&mut self, id: &str) -> String {
        self.query.clear();
        id.trim().to_string()
    }

    /// Enter in the search: its first row, picked.
    pub fn pick_first(&mut self) -> Option<String> {
        let first = self.choices().first().map(|c| c.0.to_string())?;
        Some(self.pick(&first))
    }

    /// "Load models" for `ai` with `owner`'s key from `store`: the job,
    /// unless one is on its way.
    pub fn load(
        &mut self,
        ai: &AiConfig,
        store: Arc<dyn KeyStore>,
        owner: Option<AiKey>,
    ) -> Option<ModelsJob> {
        if self.loading {
            return None;
        }
        self.loading = true;
        self.said = None;
        Some(ModelsJob {
            ai: ai.clone(),
            store,
            owner,
        })
    }

    /// Its answer for `asked`, while `now` is the provider shown (one for
    /// another provider, picked meanwhile, is dropped).
    pub fn loaded(&mut self, now: &AiConfig, asked: &AiConfig, n: Result<usize, String>) {
        if asked.provider != now.provider || asked.base_url != now.base_url {
            return;
        }
        let name = Preset::of(asked).map_or("the provider", |p| p.name);
        self.loading = false;
        self.said = Some(n.map(|n| format!("{n} models from {name}")));
        self.refresh(now);
    }

    /// The list for the model `current`, the provider's suggested model as
    /// the hint (`example`), and what "Load models" said under it.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        current: &str,
        example: &str,
        on: On<M>,
    ) -> Element<'a, M> {
        let rows = (self.choices().into_iter())
            .map(|(id, tag, typed)| {
                let label = match typed {
                    true => format!("Use \u{201c}{id}\u{201d}"),
                    false => id.to_string(),
                };
                let picked = !typed && id == current;
                (label, tag, picked, (on.pick)(id.to_string()))
            })
            .collect();
        let foot = match self.loading {
            true => ("Loading models\u{2026}", None),
            false => (self.load.as_str(), Some(on.load)),
        };
        let search = Search {
            id: SEARCH,
            hint: "Search or type a model id",
            query: &self.query,
            on_query: Box::new(on.query),
            on_submit: on.first,
        };
        let value = (!current.is_empty()).then_some(current);
        let list = combo(
            value,
            example,
            search,
            self.pricing.as_deref(),
            rows,
            Some(foot),
            on.opened,
        );
        let mut part = column![list].spacing(space::GAP);
        match &self.said {
            Some(Ok(n)) => part = part.push(sub(Role::ROW_DESC, n.as_str())),
            Some(Err(e)) => part = part.push(error(Role::ROW_DESC, e.as_str())),
            None => {}
        }
        part.into()
    }
}

/// "Load models" for a worker thread: the provider's list, asked with its
/// own key (read there, never on the UI thread). Not `Debug`-printable
/// beyond its provider.
#[derive(Clone)]
pub struct ModelsJob {
    ai: AiConfig,
    store: Arc<dyn KeyStore>,
    owner: Option<AiKey>,
}

impl fmt::Debug for ModelsJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ModelsJob({:?})", self.owner)
    }
}

impl PartialEq for ModelsJob {
    fn eq(&self, other: &Self) -> bool {
        self.ai == other.ai && self.owner == other.owner
    }
}

impl ModelsJob {
    /// The provider it asked, and how many models came (or why none did).
    pub fn run(self) -> (AiConfig, Result<usize, String>) {
        let key = match self.owner.map(|o| self.store.read(o)) {
            Some(Err(e)) => return (self.ai, Err(e.to_string())),
            Some(Ok(key)) => key,
            None => None,
        };
        let n = models::load(&self.ai, key.as_ref()).map_err(|e| e.to_string());
        (self.ai, n)
    }
}
