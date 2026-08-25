use std::sync::Arc;

use gblab_core::CoreService;

pub struct AppState {
    pub core: Arc<CoreService>,
}

impl AppState {
    pub fn new(core: CoreService) -> Self {
        Self {
            core: Arc::new(core),
        }
    }
}
