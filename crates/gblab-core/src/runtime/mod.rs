//! Tokio task 监督、调度、限流与关闭机制。

mod dialog;
mod platform;
mod registration;
mod scheduler;

pub use dialog::{Dialog, DialogId, DialogManager, DialogState};
pub use platform::{
    PlatformCommandType, PlatformRequest, PlatformRequestMethod, SubscriptionManager,
    SubscriptionRuntimeStatus, SubscriptionSnapshot,
};
pub use registration::{
    BatchOperationAccepted, DeviceControlAction, DeviceRegistrationSnapshot,
    DeviceRegistrationStatus, InteractionDirection, InteractionLog, PtzAction, RegistrationEvent,
    RegistrationHandle, RegistrationOperationStatus, RegistrationRuntimeError,
    RegistrationSnapshot,
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
    /// 同时运行的 `FFmpeg` 媒体会话上限。
    pub media_session_concurrency: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            device_start_concurrency: 128,
            media_session_concurrency: 16,
        }
    }
}
