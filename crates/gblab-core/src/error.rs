use thiserror::Error;

use crate::persistence::DatabaseError;

/// `GBLab` 模拟核心的统一错误。
#[derive(Debug, Error)]
pub enum CoreError {
    /// SQLite 初始化或访问失败。
    #[error("数据库操作失败: {0}")]
    Database(#[from] DatabaseError),
}

/// `GBLab` 模拟核心的统一结果类型。
pub type Result<T> = std::result::Result<T, CoreError>;
