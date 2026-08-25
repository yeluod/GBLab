use std::sync::{Arc, RwLock};

use gblab_core::CoreService;

pub struct AppState {
    pub core: Arc<RwLock<CoreService>>,
}

impl AppState {
    pub fn new(core: CoreService) -> Self {
        Self {
            core: Arc::new(RwLock::new(core)),
        }
    }
}
