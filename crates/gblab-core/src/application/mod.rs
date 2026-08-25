//! 桌面应用可调用的用例与业务编排入口。

use serde::Serialize;

use crate::{Result, configuration::ConfigurationStore};

/// `GBLab` 核心服务。
pub struct CoreService {
    configuration: ConfigurationStore,
}

impl CoreService {
    /// 打开 JSON 配置文件并初始化核心服务。
    ///
    /// # Errors
    ///
    /// JSON 配置读取或创建失败时返回错误。
    pub fn open(configuration_path: &std::path::Path) -> Result<Self> {
        let configuration = ConfigurationStore::open(configuration_path)?;
        Ok(Self { configuration })
    }

    /// 返回桌面端展示所需的轻量核心信息。
    #[must_use]
    pub fn info(&self) -> CoreInfo {
        CoreInfo {
            version: env!("CARGO_PKG_VERSION"),
            configuration_ready: self.configuration.is_ready(),
        }
    }
}

/// Rust 模拟核心的基础状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CoreInfo {
    /// 核心 crate 版本。
    pub version: &'static str,
    /// JSON 配置文件是否已成功读取或创建。
    pub configuration_ready: bool,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::CoreService;

    #[tokio::test]
    async fn open_should_create_json_configuration_and_report_ready()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");

        let service = CoreService::open(configuration_path.as_path())?;

        assert!(service.info().configuration_ready);
        assert!(configuration_path.is_file());
        Ok(())
    }
}
