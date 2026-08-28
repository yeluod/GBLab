use thiserror::Error;

use crate::{configuration::ConfigurationError, domain::DeviceError};

/// `GBLab` 模拟核心的统一错误。
#[derive(Debug, Error)]
pub enum CoreError {
    /// JSON 配置读取或写入失败。
    #[error("配置操作失败: {0}")]
    Configuration(#[from] ConfigurationError),
    /// 设备配置或通道派生失败。
    #[error("设备操作失败: {0}")]
    Device(#[from] DeviceError),
    /// 媒体源探测或播放操作失败。
    #[error("媒体操作失败: {0}")]
    Media(#[from] crate::media::MediaError),
}

/// `GBLab` 模拟核心的统一结果类型。
pub type Result<T> = std::result::Result<T, CoreError>;
