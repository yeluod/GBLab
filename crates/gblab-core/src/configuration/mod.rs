//! JSON 配置文件的读取、校验与写入。

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::SimulatedDevice;

const CURRENT_SCHEMA_VERSION: u8 = 2;
const MAX_SIP_URI_LENGTH: usize = 256;
const MAX_PASSWORD_LENGTH: usize = 128;
const MAX_DOMAIN_LENGTH: usize = 64;
const MAX_REGISTER_EXPIRES: u32 = 86_400;
const MAX_KEEPALIVE_INTERVAL: u32 = 3_600;

/// 保存在 JSON 文件中的应用配置。
///
/// 仅保存用户配置；设备注册状态、SIP 消息和交互日志等运行时数据不会落盘。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfiguration {
    /// 配置文件格式版本。
    pub schema_version: u8,
    /// 全部模拟设备共享的唯一 SIP 服务配置。
    pub sip_service: SipServiceConfiguration,
    /// 设备配置；注册状态和通道不会写入此集合。
    pub device_collection: DeviceCollectionConfiguration,
}

impl Default for AppConfiguration {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            sip_service: SipServiceConfiguration::default(),
            device_collection: DeviceCollectionConfiguration::default(),
        }
    }
}

/// 写入 JSON 的设备集合配置。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeviceCollectionConfiguration {
    /// 是否已执行唯一一次批量添加。
    pub has_completed_batch_add: bool,
    /// 持久化设备，不包含注册状态与派生通道。
    pub devices: Vec<SimulatedDevice>,
}

/// SIP 信令传输协议。
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SipTransport {
    /// 用户数据报协议。
    Udp,
    /// 传输控制协议。
    Tcp,
}

/// 全部模拟设备共享的 GB28181 XML 信令字符集。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SignalCharset {
    /// GB2312。为兼容多数国标平台，作为默认字符集。
    #[default]
    #[serde(rename = "GB2312")]
    Gb2312,
    /// GBK。
    #[serde(rename = "GBK")]
    Gbk,
    /// UTF-8。
    #[serde(rename = "UTF-8")]
    Utf8,
}

impl SignalCharset {
    /// 返回用于 XML 声明和 Content-Type 的标准标签。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gb2312 => "GB2312",
            Self::Gbk => "GBK",
            Self::Utf8 => "UTF-8",
        }
    }
}

/// 全部模拟设备共享的 SIP 服务配置。
///
/// 密码作为模拟器配置明文读取、传输并写入 JSON。
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SipServiceConfiguration {
    /// SIP 平台地址，格式为 `sip:host:port`。
    pub uri: String,
    /// SIP 信令传输协议。
    pub transport: SipTransport,
    /// 20 位数字平台 ID。
    pub platform_id: String,
    /// SIP 认证域。
    pub domain: String,
    /// 全部设备共用的 SIP Digest 认证密码。
    pub password: String,
    /// SIP socket 本地绑定地址。
    pub local_bind_address: String,
    /// 写入 Via 与 Contact 的对外地址；为空时根据到平台的路由自动探测。
    pub advertised_address: String,
    /// 全部设备共享的本地 SIP 监听端口。
    pub local_port: u16,
    /// 注册有效期，单位为秒。
    pub register_expires: u32,
    /// 心跳间隔，单位为秒。
    pub keepalive_interval: u32,
    /// 全部设备共享的 GB28181 XML 信令字符集。
    pub signal_charset: SignalCharset,
}

impl Default for SipServiceConfiguration {
    fn default() -> Self {
        Self {
            uri: "sip:192.168.1.100:5060".to_owned(),
            transport: SipTransport::Udp,
            platform_id: "34020000002000000001".to_owned(),
            domain: "3402000000".to_owned(),
            password: String::new(),
            local_bind_address: "0.0.0.0".to_owned(),
            advertised_address: String::new(),
            local_port: 5_060,
            register_expires: 3_600,
            keepalive_interval: 60,
            signal_charset: SignalCharset::Gb2312,
        }
    }
}

impl SipServiceConfiguration {
    /// 规范化文本字段并验证 SIP 服务配置。
    ///
    /// # Errors
    ///
    /// 任一字段不满足 SIP 服务配置约束时返回字段级验证错误。
    pub fn normalize_and_validate(mut self) -> Result<Self, ConfigurationError> {
        self.uri = self.uri.trim().to_owned();
        self.platform_id = self.platform_id.trim().to_owned();
        self.domain = self.domain.trim().to_owned();
        self.local_bind_address = self.local_bind_address.trim().to_owned();
        self.advertised_address = self.advertised_address.trim().to_owned();

        if self.transport != SipTransport::Udp {
            return Err(ConfigurationError::invalid_field(
                "transport",
                "当前真实 SIP 传输仅支持 UDP",
            ));
        }
        validate_sip_uri(&self.uri)?;
        validate_twenty_digit_id("platformId", &self.platform_id)?;
        validate_domain(&self.domain)?;
        validate_password(&self.password)?;
        validate_ip_address("localBindAddress", &self.local_bind_address, false)?;
        validate_ip_address("advertisedAddress", &self.advertised_address, true)?;
        if self.local_port == 0 {
            return Err(ConfigurationError::invalid_field(
                "localPort",
                "本地 SIP 端口必须介于 1 到 65535",
            ));
        }
        validate_range(
            "registerExpires",
            self.register_expires,
            1,
            MAX_REGISTER_EXPIRES,
        )?;
        validate_range(
            "keepaliveInterval",
            self.keepalive_interval,
            1,
            MAX_KEEPALIVE_INTERVAL,
        )?;

        Ok(self)
    }
}

/// 只承载配置文件路径与已加载配置的 JSON 存储。
pub struct ConfigurationStore {
    path: PathBuf,
    configuration: AppConfiguration,
}

impl ConfigurationStore {
    /// 读取已有 JSON 配置；文件不存在时创建默认配置。
    ///
    /// 旧版本配置缺少密码字段时会加载为空密码，等待用户在界面中补充。
    ///
    /// # Errors
    ///
    /// 无法创建目录、读写文件或解析 JSON 时返回错误。
    pub fn open(path: &Path) -> Result<Self, ConfigurationError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let configuration = if path.exists() {
            let contents = fs::read_to_string(path)?;
            serde_json::from_str(&contents)?
        } else {
            let configuration = AppConfiguration::default();
            write_configuration(path, &configuration)?;
            configuration
        };

        Ok(Self {
            path: path.to_owned(),
            configuration,
        })
    }

    /// 返回当前 JSON 配置是否已就绪。
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.path.is_file() && self.configuration.schema_version > 0
    }

    /// 返回当前 SIP 服务配置快照。
    #[must_use]
    pub fn sip_service(&self) -> SipServiceConfiguration {
        self.configuration.sip_service.clone()
    }

    /// 返回当前设备集合配置快照。
    #[must_use]
    pub fn device_collection(&self) -> DeviceCollectionConfiguration {
        self.configuration.device_collection.clone()
    }

    /// 校验并保存唯一 SIP 服务配置。
    ///
    /// 只有文件写入成功后才会替换内存配置，确保内存与磁盘一致。
    ///
    /// # Errors
    ///
    /// 配置校验、序列化或文件写入失败时返回错误。
    pub fn save_sip_service(
        &mut self,
        sip_service: SipServiceConfiguration,
    ) -> Result<SipServiceConfiguration, ConfigurationError> {
        let sip_service = sip_service.normalize_and_validate()?;
        let next_configuration = AppConfiguration {
            schema_version: CURRENT_SCHEMA_VERSION,
            sip_service: sip_service.clone(),
            device_collection: self.configuration.device_collection.clone(),
        };

        write_configuration(&self.path, &next_configuration)?;
        self.configuration = next_configuration;
        Ok(sip_service)
    }

    /// 保存设备配置与一次性批量添加标记。
    ///
    /// # Errors
    ///
    /// JSON 序列化或文件写入失败时返回错误。
    pub fn save_device_collection(
        &mut self,
        device_collection: DeviceCollectionConfiguration,
    ) -> Result<DeviceCollectionConfiguration, ConfigurationError> {
        let next_configuration = AppConfiguration {
            schema_version: CURRENT_SCHEMA_VERSION,
            sip_service: self.configuration.sip_service.clone(),
            device_collection: device_collection.clone(),
        };
        write_configuration(&self.path, &next_configuration)?;
        self.configuration = next_configuration;
        Ok(device_collection)
    }
}

fn validate_sip_uri(uri: &str) -> Result<(), ConfigurationError> {
    if uri.is_empty() || uri.len() > MAX_SIP_URI_LENGTH || uri.chars().any(char::is_whitespace) {
        return Err(ConfigurationError::invalid_field(
            "uri",
            "SIP 地址不能为空、不能包含空白且长度不能超过 256 个字符",
        ));
    }

    let Some(authority) = uri.strip_prefix("sip:") else {
        return Err(ConfigurationError::invalid_field(
            "uri",
            "SIP 地址必须以 sip: 开头",
        ));
    };
    let Some((host, port)) = authority.rsplit_once(':') else {
        return Err(ConfigurationError::invalid_field(
            "uri",
            "SIP 地址必须包含主机和端口",
        ));
    };
    let Ok(port) = port.parse::<u16>() else {
        return Err(ConfigurationError::invalid_field(
            "uri",
            "SIP 地址端口必须介于 1 到 65535",
        ));
    };
    if host.is_empty() || port == 0 {
        return Err(ConfigurationError::invalid_field(
            "uri",
            "SIP 地址主机或端口无效",
        ));
    }

    Ok(())
}

fn validate_twenty_digit_id(field: &'static str, value: &str) -> Result<(), ConfigurationError> {
    if value.len() == 20 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(());
    }

    Err(ConfigurationError::invalid_field(field, "必须为 20 位数字"))
}

fn validate_domain(domain: &str) -> Result<(), ConfigurationError> {
    if domain.is_empty()
        || domain.len() > MAX_DOMAIN_LENGTH
        || domain.chars().any(char::is_whitespace)
    {
        return Err(ConfigurationError::invalid_field(
            "domain",
            "域不能为空、不能包含空白且长度不能超过 64 个字符",
        ));
    }

    Ok(())
}

fn validate_password(password: &str) -> Result<(), ConfigurationError> {
    if password.is_empty() || password.len() > MAX_PASSWORD_LENGTH {
        return Err(ConfigurationError::invalid_field(
            "password",
            "密码不能为空且长度不能超过 128 个字符",
        ));
    }
    if password.chars().any(char::is_control) {
        return Err(ConfigurationError::invalid_field(
            "password",
            "密码不能包含控制字符",
        ));
    }

    Ok(())
}

fn validate_ip_address(
    field: &'static str,
    address: &str,
    allow_empty: bool,
) -> Result<(), ConfigurationError> {
    if allow_empty && address.is_empty() {
        return Ok(());
    }
    address
        .parse::<IpAddr>()
        .map(|_| ())
        .map_err(|_| ConfigurationError::invalid_field(field, "必须是有效的 IPv4 或 IPv6 地址"))
}

fn validate_range(
    field: &'static str,
    value: u32,
    min: u32,
    max: u32,
) -> Result<(), ConfigurationError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }

    Err(ConfigurationError::invalid_field(field, "数值超出允许范围"))
}

fn write_configuration(
    path: &Path,
    configuration: &AppConfiguration,
) -> Result<(), ConfigurationError> {
    let mut contents = serde_json::to_vec_pretty(configuration)?;
    contents.push(b'\n');

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path)?;
    file.write_all(&contents)?;
    file.sync_all()?;

    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;

    Ok(())
}

/// JSON 配置文件的访问与验证错误。
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// 文件系统访问失败。
    #[error("配置文件访问失败: {0}")]
    Io(#[from] io::Error),
    /// JSON 格式不合法。
    #[error("配置 JSON 格式错误: {0}")]
    Json(#[from] serde_json::Error),
    /// SIP 服务字段不符合约束。
    #[error("配置字段 {field} 无效: {reason}")]
    InvalidField {
        /// IPC 使用的字段名。
        field: &'static str,
        /// 不包含输入值的错误原因。
        reason: &'static str,
    },
}

impl ConfigurationError {
    const fn invalid_field(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidField { field, reason }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        ConfigurationError, ConfigurationStore, DeviceCollectionConfiguration, SignalCharset,
        SipServiceConfiguration, SipTransport,
    };

    fn valid_sip_service() -> SipServiceConfiguration {
        SipServiceConfiguration {
            uri: "sip:10.0.0.8:5060".to_owned(),
            transport: SipTransport::Udp,
            platform_id: "34020000002000000001".to_owned(),
            domain: "3402000000".to_owned(),
            password: "test-only-password".to_owned(),
            local_bind_address: "0.0.0.0".to_owned(),
            advertised_address: "10.0.0.10".to_owned(),
            local_port: 5_060,
            register_expires: 3_600,
            keepalive_interval: 60,
            signal_charset: SignalCharset::Gb2312,
        }
    }

    #[test]
    fn open_should_create_default_json_when_file_is_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");

        let store = ConfigurationStore::open(&configuration_path)?;

        assert!(store.is_ready());
        Ok(())
    }

    #[test]
    fn save_sip_service_should_persist_password_and_reload_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut store = ConfigurationStore::open(&configuration_path)?;
        store.save_sip_service(valid_sip_service())?;

        let reloaded = ConfigurationStore::open(&configuration_path)?;

        assert_eq!(reloaded.sip_service().password, "test-only-password");
        Ok(())
    }

    #[test]
    fn save_sip_service_should_reject_empty_password_without_changing_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut store = ConfigurationStore::open(&configuration_path)?;
        let original_contents = fs::read_to_string(&configuration_path)?;
        let mut invalid = valid_sip_service();
        invalid.password.clear();

        let error = store.save_sip_service(invalid).err();

        assert!(matches!(
            error,
            Some(ConfigurationError::InvalidField {
                field: "password",
                ..
            })
        ));
        assert_eq!(fs::read_to_string(&configuration_path)?, original_contents);
        Ok(())
    }

    #[test]
    fn save_sip_service_should_reject_tcp_until_transport_is_implemented()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut store = ConfigurationStore::open(&configuration_path)?;
        let mut configuration = valid_sip_service();
        configuration.transport = SipTransport::Tcp;

        let error = store.save_sip_service(configuration).err();

        assert!(matches!(
            error,
            Some(ConfigurationError::InvalidField {
                field: "transport",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn open_should_migrate_legacy_configuration_with_empty_password()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        fs::write(&configuration_path, r#"{"schemaVersion":1}"#)?;

        let store = ConfigurationStore::open(&configuration_path)?;

        assert!(store.sip_service().password.is_empty());
        assert_eq!(store.sip_service().signal_charset, SignalCharset::Gb2312);
        assert!(store.device_collection().devices.is_empty());
        Ok(())
    }

    #[test]
    fn save_sip_service_should_persist_signal_charset() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut store = ConfigurationStore::open(&configuration_path)?;
        let mut configuration = valid_sip_service();
        configuration.signal_charset = SignalCharset::Utf8;

        store.save_sip_service(configuration)?;
        let reloaded = ConfigurationStore::open(&configuration_path)?;

        assert_eq!(reloaded.sip_service().signal_charset, SignalCharset::Utf8);
        Ok(())
    }

    #[test]
    fn save_sip_service_should_preserve_device_collection() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut store = ConfigurationStore::open(&configuration_path)?;
        store.save_device_collection(DeviceCollectionConfiguration {
            has_completed_batch_add: true,
            devices: Vec::new(),
        })?;

        store.save_sip_service(valid_sip_service())?;

        assert!(store.device_collection().has_completed_batch_add);
        Ok(())
    }
}
