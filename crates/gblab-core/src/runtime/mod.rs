//! Tokio task 监督、调度、限流与关闭机制。

mod registration;

pub use registration::{
    BatchOperationAccepted, DeviceRegistrationSnapshot, DeviceRegistrationStatus,
    InteractionDirection, InteractionLog, RegistrationEvent, RegistrationHandle,
    RegistrationOperationStatus, RegistrationRuntimeError, RegistrationSnapshot,
};

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
