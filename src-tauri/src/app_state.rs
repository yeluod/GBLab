use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use gblab_core::{CoreService, GlobalMediaRuntime, MediaError, runtime::RegistrationHandle};

pub struct AppState {
    pub core: Arc<RwLock<CoreService>>,
    pub registration: RegistrationHandle,
    pub media: GlobalMediaRuntime,
    operation_gate: AtomicBool,
    shutdown_started: AtomicBool,
}

impl AppState {
    pub fn new(
        core: CoreService,
        registration: RegistrationHandle,
        media: GlobalMediaRuntime,
    ) -> Result<Self, MediaError> {
        Ok(Self {
            core: Arc::new(RwLock::new(core)),
            registration,
            media,
            operation_gate: AtomicBool::new(false),
            shutdown_started: AtomicBool::new(false),
        })
    }

    pub fn try_operation(&self) -> Option<OperationGuard<'_>> {
        self.operation_gate
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| OperationGuard {
                gate: &self.operation_gate,
            })
    }

    pub fn begin_shutdown(&self) -> bool {
        self.shutdown_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

pub struct OperationGuard<'a> {
    gate: &'a AtomicBool,
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.gate.store(false, Ordering::Release);
    }
}
