//! 桌面应用可调用的用例与业务编排入口。

use serde::Serialize;

use crate::{
    Result,
    configuration::{ConfigurationStore, SipServiceConfiguration},
};

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

    /// 返回当前唯一 SIP 服务配置快照。
    #[must_use]
    pub fn sip_service_configuration(&self) -> SipServiceConfiguration {
        self.configuration.sip_service()
    }

    /// 校验并保存唯一 SIP 服务配置。
    ///
    /// # Errors
    ///
    /// 字段校验或 JSON 文件写入失败时返回错误。
    pub fn save_sip_service_configuration(
        &mut self,
        configuration: SipServiceConfiguration,
    ) -> Result<SipServiceConfiguration> {
        Ok(self.configuration.save_sip_service(configuration)?)
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
    use crate::configuration::{SipServiceConfiguration, SipTransport};

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

    #[test]
    fn save_sip_service_configuration_should_update_core_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut service = CoreService::open(configuration_path.as_path())?;
        let configuration = SipServiceConfiguration {
            uri: "sip:10.0.0.9:5060".to_owned(),
            transport: SipTransport::Udp,
            platform_id: "34020000002000000001".to_owned(),
            domain: "3402000000".to_owned(),
            password: "test-only-password".to_owned(),
            register_expires: 3_600,
            keepalive_interval: 60,
        };

        service.save_sip_service_configuration(configuration)?;

        assert_eq!(service.sip_service_configuration().uri, "sip:10.0.0.9:5060");
        Ok(())
    }
}
