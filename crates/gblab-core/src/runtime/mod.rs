//! Tokio task 监督、调度、限流与关闭机制。

mod business;
mod handle;
mod operations;
mod platform;
mod registration;
mod scheduler;
mod state;
mod time;
mod types;

pub use handle::RegistrationHandle;
pub use platform::{
    PlatformCommandType, PlatformRequest, PlatformRequestMethod, SubscriptionManager,
    SubscriptionRuntimeStatus, SubscriptionSnapshot,
};
pub use types::{
    BatchOperationAccepted, DeviceControlAction, DeviceRegistrationSnapshot,
    DeviceRegistrationStatus, InteractionDirection, InteractionLog, PtzAction, RegistrationEvent,
    RegistrationOperationStatus, RegistrationRuntimeError, RegistrationSnapshot,
};

/// 面向整个模拟器运行时的公开句柄。
pub type SimulatorHandle = RegistrationHandle;
/// 面向整个模拟器运行时的事件流。
pub type SimulatorEvent = RegistrationEvent;
/// 面向整个模拟器运行时的错误类型。
pub type SimulatorRuntimeError = RegistrationRuntimeError;

/// 核心运行时的有界资源配置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeLimits {
    /// 同时执行的设备启动任务上限。
    pub device_start_concurrency: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            device_start_concurrency: 128,
        }
    }
}
