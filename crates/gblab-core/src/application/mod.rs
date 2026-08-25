//! 桌面应用可调用的用例与业务编排入口。

use std::path::Path;

use serde::Serialize;

use crate::{Result, persistence::Database};

/// `GBLab` 核心服务。
pub struct CoreService {
    database: Database,
}

impl CoreService {
    /// 打开持久化文件并初始化核心服务。
    ///
    /// # Errors
    ///
    /// SQLite 连接或迁移失败时返回错误。
    pub async fn open(database_path: &Path) -> Result<Self> {
        let database = Database::open(database_path).await?;
        Ok(Self { database })
    }

    /// 返回桌面端展示所需的轻量核心信息。
    #[must_use]
    pub fn info(&self) -> CoreInfo {
        CoreInfo {
            version: env!("CARGO_PKG_VERSION"),
            database_ready: self.database.is_ready(),
        }
    }
}

/// Rust 模拟核心的基础状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CoreInfo {
    /// 核心 crate 版本。
    pub version: &'static str,
    /// SQLite 连接池是否已建立连接。
    pub database_ready: bool,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::tempdir;

    use super::CoreService;

    #[tokio::test]
    async fn open_should_initialize_database_and_report_ready()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let database_path = directory.path().join("gblab.db");

        let service = tokio::time::timeout(
            Duration::from_secs(5),
            CoreService::open(database_path.as_path()),
        )
        .await??;

        assert!(service.info().database_ready);
        Ok(())
    }
}
