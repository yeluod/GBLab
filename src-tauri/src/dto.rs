use gblab_core::{CoreError, CoreInfo, SipServiceConfiguration, SipTransport};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    app_name: &'static str,
    app_version: &'static str,
    core_version: &'static str,
    configuration_ready: bool,
}

impl AppInfoDto {
    pub const fn from_core(core: CoreInfo) -> Self {
        Self {
            app_name: "GBLab",
            app_version: env!("CARGO_PKG_VERSION"),
            core_version: core.version,
            configuration_ready: core.configuration_ready,
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SipTransportDto {
    Udp,
    Tcp,
}

impl From<SipTransport> for SipTransportDto {
    fn from(value: SipTransport) -> Self {
        match value {
            SipTransport::Udp => Self::Udp,
            SipTransport::Tcp => Self::Tcp,
        }
    }
}

impl From<SipTransportDto> for SipTransport {
    fn from(value: SipTransportDto) -> Self {
        match value {
            SipTransportDto::Udp => Self::Udp,
            SipTransportDto::Tcp => Self::Tcp,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SipServiceConfigurationDto {
    uri: String,
    transport: SipTransportDto,
    platform_id: String,
    domain: String,
    password: String,
    register_expires: u32,
    keepalive_interval: u32,
}

impl SipServiceConfigurationDto {
    pub fn from_core(configuration: SipServiceConfiguration) -> Self {
        Self {
            uri: configuration.uri,
            transport: configuration.transport.into(),
            platform_id: configuration.platform_id,
            domain: configuration.domain,
            password: configuration.password,
            register_expires: configuration.register_expires,
            keepalive_interval: configuration.keepalive_interval,
        }
    }

    pub fn into_core(self) -> SipServiceConfiguration {
        SipServiceConfiguration {
            uri: self.uri,
            transport: self.transport.into(),
            platform_id: self.platform_id,
            domain: self.domain,
            password: self.password,
            register_expires: self.register_expires,
            keepalive_interval: self.keepalive_interval,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorDto {
    code: &'static str,
    message: String,
}

impl CommandErrorDto {
    pub fn from_core(error: &CoreError) -> Self {
        Self {
            code: "configuration_error",
            message: error.to_string(),
        }
    }

    pub fn state_unavailable() -> Self {
        Self {
            code: "state_unavailable",
            message: "核心状态暂时不可用，请重启应用后重试。".to_owned(),
        }
    }

    pub fn task_failed() -> Self {
        Self {
            code: "background_task_failed",
            message: "配置保存任务异常结束，请重试。".to_owned(),
        }
    }
}
