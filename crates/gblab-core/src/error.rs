use thiserror::Error;

use crate::configuration::ConfigurationError;

/// `GBLab` 模拟核心的统一错误。
#[derive(Debug, Error)]
pub enum CoreError {
    /// JSON 配置读取或写入失败。
    #[error("配置操作失败: {0}")]
    Configuration(#[from] ConfigurationError),
}

/// `GBLab` 模拟核心的统一结果类型。
pub type Result<T> = std::result::Result<T, CoreError>;
