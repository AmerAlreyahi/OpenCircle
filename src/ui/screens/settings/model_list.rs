//! Settings > Ask AI's Model list: the shared one (`screens::model_list`)
//! on the draft's provider.
pub use crate::ui::screens::model_list::{ModelList, ModelsJob, SEARCH};

use super::state::{Outcome, State};

impl State {
    /// The list for the provider the draft sends to (none: the browser).
    pub(super) fn refresh_models(&mut self) {
        self.model_list.refresh(&self.draft.ai);
    }

    /// The provider changed: its own list, nothing typed, nothing said.
    pub(super) fn new_model_list(&mut self) {
        self.model_list = ModelList::of(&self.draft.ai);
    }

    pub(super) fn pick_model(&mut self, id: &str) {
        self.draft.ai.model = self.model_list.pick(id);
    }

    /// Enter in the search: its first row.
    pub(super) fn pick_first_model(&mut self) {
        if let Some(id) = self.model_list.pick_first() {
            self.draft.ai.model = id;
        }
    }

    /// "Load models": the job, unless one is on its way.
    pub(super) fn load_models(&mut self) -> Outcome {
        let job = self
            .model_list
            .load(&self.draft.ai, self.store.clone(), self.key_owner);
        job.map_or(Outcome::None, Outcome::LoadModels)
    }

    /// Its answer (one for another provider, picked meanwhile, is dropped).
    pub(super) fn models_loaded(&mut self, ai: &crate::config::AiConfig, n: Result<usize, String>) {
        self.model_list.loaded(&self.draft.ai, ai, n);
    }
}
