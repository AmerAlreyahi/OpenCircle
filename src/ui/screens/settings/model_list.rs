//! Settings > Ask AI's Model list (`kit::dropdown::combo`): the provider's
//! models (`ai::models`: built in, plus what "Load models" brought), the
//! search typed in it (a typed id that is in no list is offered as is), and
//! the one network call, "Load models from <provider>", which runs only on
//! its click, on a worker thread.
use std::fmt;
use std::sync::Arc;

use super::state::{Outcome, State};
use crate::ai::models::{self, Model};
use crate::ai::providers::Preset;
use crate::config::AiConfig;
use crate::secrets::{AiKey, KeyStore};

/// The search field's widget id.
pub const SEARCH: &str = "model-search";

/// The list as the window keeps it (made again when the provider changes or
/// a list arrives).
#[derive(Debug, Default)]
pub struct ModelList {
    pub(super) models: Vec<Model>,
    pub(super) query: String,
    /// "Load models from Google"; "Pricing is set by Google" when a row has
    /// a price tag.
    pub(super) load: String,
    pub(super) pricing: Option<String>,
    pub(super) loading: bool,
    /// What the last "Load models" said: how many came, or why none did.
    pub(super) said: Option<Result<String, String>>,
}

/// A row: its id, its price tag, and whether it is the typed id itself.
pub(super) type Choice<'a> = (&'a str, Option<&'a str>, bool);

impl ModelList {
    /// The rows for the search typed: the models that contain it (any case),
    /// then the typed id itself unless a model has exactly that id.
    pub(super) fn choices(&self) -> Vec<Choice<'_>> {
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

impl State {
    /// The list for the provider the draft sends to (none: the browser).
    pub(super) fn refresh_models(&mut self) {
        let ai = &self.draft.ai;
        let list = &mut self.model_list;
        let Some(preset) = Preset::of(ai) else {
            *list = ModelList::default();
            return;
        };
        list.models = models::known(&preset, ai);
        list.load = format!("Load models from {}", preset.name);
        list.pricing =
            (list.models.iter().any(|m| m.tag.is_some())).then(|| models::pricing_note(&preset));
    }

    /// The provider changed: its own list, nothing typed, nothing said.
    pub(super) fn new_model_list(&mut self) {
        self.model_list = ModelList::default();
        self.refresh_models();
    }

    pub(super) fn pick_model(&mut self, id: &str) {
        self.draft.ai.model = id.trim().to_string();
        self.model_list.query.clear();
    }

    /// Enter in the search: its first row.
    pub(super) fn pick_first_model(&mut self) {
        let first = self.model_list.choices().first().map(|c| c.0.to_string());
        if let Some(id) = first {
            self.pick_model(&id);
        }
    }

    /// "Load models": the job, unless one is on its way.
    pub(super) fn load_models(&mut self) -> Outcome {
        if self.model_list.loading {
            return Outcome::None;
        }
        self.model_list.loading = true;
        self.model_list.said = None;
        Outcome::LoadModels(ModelsJob {
            ai: self.draft.ai.clone(),
            store: self.store.clone(),
            owner: self.key_owner,
        })
    }

    /// Its answer (one for another provider, picked meanwhile, is dropped).
    pub(super) fn models_loaded(&mut self, ai: &AiConfig, n: Result<usize, String>) {
        let now = &self.draft.ai;
        if ai.provider != now.provider || ai.base_url != now.base_url {
            return;
        }
        let name = Preset::of(ai).map_or("the provider", |p| p.name);
        self.model_list.loading = false;
        self.model_list.said = Some(n.map(|n| format!("{n} models from {name}")));
        self.refresh_models();
    }
}
