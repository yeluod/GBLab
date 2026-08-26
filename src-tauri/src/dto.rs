use gblab_core::{
    BatchDeviceDraft, CoreError, CoreInfo, DeviceKind, DeviceSnapshot, DeviceUpdateDraft,
    SimulatedChannel, SimulatedDevice, SipServiceConfiguration, SipTransport,
    runtime::{BatchOperationAccepted, InteractionLog, RegistrationRuntimeError},
};
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
    local_bind_address: String,
    advertised_address: String,
    local_port: u16,
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
            local_bind_address: configuration.local_bind_address,
            advertised_address: configuration.advertised_address,
            local_port: configuration.local_port,
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
            local_bind_address: self.local_bind_address,
            advertised_address: self.advertised_address,
            local_port: self.local_port,
            register_expires: self.register_expires,
            keepalive_interval: self.keepalive_interval,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOperationAcceptedDto {
    operation_id: String,
    total: usize,
}

impl From<BatchOperationAccepted> for BatchOperationAcceptedDto {
    fn from(value: BatchOperationAccepted) -> Self {
        Self {
            operation_id: value.operation_id,
            total: value.total,
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub enum DeviceKindDto {
    #[serde(rename = "摄像机")]
    Camera,
    #[serde(rename = "球机")]
    PtzCamera,
    #[serde(rename = "NVR")]
    Nvr,
    #[serde(rename = "门禁设备")]
    AccessControl,
}

impl From<DeviceKind> for DeviceKindDto {
    fn from(value: DeviceKind) -> Self {
        match value {
            DeviceKind::Camera => Self::Camera,
            DeviceKind::PtzCamera => Self::PtzCamera,
            DeviceKind::Nvr => Self::Nvr,
            DeviceKind::AccessControl => Self::AccessControl,
        }
    }
}

impl From<DeviceKindDto> for DeviceKind {
    fn from(value: DeviceKindDto) -> Self {
        match value {
            DeviceKindDto::Camera => Self::Camera,
            DeviceKindDto::PtzCamera => Self::PtzCamera,
            DeviceKindDto::Nvr => Self::Nvr,
            DeviceKindDto::AccessControl => Self::AccessControl,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedDeviceDto {
    id: String,
    name: String,
    r#type: DeviceKindDto,
    manufacturer: String,
    model: String,
    firmware_version: String,
    channel_count: u16,
    registration_status: &'static str,
    created_at: u64,
}

impl From<SimulatedDevice> for SimulatedDeviceDto {
    fn from(value: SimulatedDevice) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            r#type: value.kind.into(),
            manufacturer: value.manufacturer,
            model: value.model,
            firmware_version: value.firmware_version,
            channel_count: value.channel_count,
            registration_status: "unregistered",
            created_at: value.created_at,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedChannelDto {
    id: String,
    device_id: String,
    name: String,
    index: u16,
    platform_subscriptions: Vec<String>,
}

impl From<SimulatedChannel> for SimulatedChannelDto {
    fn from(value: SimulatedChannel) -> Self {
        Self {
            id: value.id.to_string(),
            device_id: value.device_id.to_string(),
            name: value.name,
            index: value.index,
            platform_subscriptions: Vec::new(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshotDto {
    devices: Vec<SimulatedDeviceDto>,
    has_completed_batch_add: bool,
}

/// 设备查询分页结果；桌面端不需要为分页再次拉取完整设备集合。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePageDto {
    devices: Vec<SimulatedDeviceDto>,
    total: usize,
    offset: usize,
    limit: usize,
    has_completed_batch_add: bool,
}

impl DevicePageDto {
    pub fn from_snapshot(
        snapshot: DeviceSnapshot,
        offset: usize,
        limit: usize,
        filter: Option<&str>,
        sort: Option<&str>,
    ) -> Self {
        let filter = filter.unwrap_or_default().trim().to_ascii_lowercase();
        let mut devices: Vec<_> = snapshot
            .devices
            .into_iter()
            .filter(|device| {
                filter.is_empty()
                    || device.id.to_string().contains(&filter)
                    || device.name.to_ascii_lowercase().contains(&filter)
                    || device.manufacturer.to_ascii_lowercase().contains(&filter)
                    || device.model.to_ascii_lowercase().contains(&filter)
            })
            .collect();
        match sort.unwrap_or("id-asc") {
            "id-desc" => devices.sort_by_key(|device| std::cmp::Reverse(device.id.to_string())),
            "name-asc" => devices.sort_by(|left, right| left.name.cmp(&right.name)),
            "name-desc" => devices.sort_by(|left, right| right.name.cmp(&left.name)),
            _ => devices.sort_by_key(|device| device.id.to_string()),
        }
        let total = devices.len();
        let page = devices
            .into_iter()
            .skip(offset)
            .take(limit.max(1))
            .map(Into::into)
            .collect();
        Self {
            devices: page,
            total,
            offset,
            limit: limit.max(1),
            has_completed_batch_add: snapshot.has_completed_batch_add,
        }
    }
}

/// SIP 交互日志查询分页结果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLogPageDto {
    items: Vec<InteractionLog>,
    total: usize,
    offset: usize,
    limit: usize,
}

/// 交互日志查询条件。
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLogQueryDto {
    pub offset: usize,
    pub limit: usize,
    pub device_id: Option<String>,
    pub direction: Option<String>,
    pub method: Option<String>,
    pub keyword: Option<String>,
}

impl InteractionLogPageDto {
    pub fn from_logs(logs: Vec<InteractionLog>, query: &InteractionLogQueryDto) -> Self {
        let method = query
            .method
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        let keyword = query
            .keyword
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let mut filtered: Vec<_> = logs
            .into_iter()
            .filter(|log| {
                query
                    .device_id
                    .as_deref()
                    .is_none_or(|value| value.is_empty() || log.device_id == value)
                    && query.direction.as_deref().is_none_or(|value| {
                        value.is_empty()
                            || matches!(
                                (value, log.direction),
                                ("send", gblab_core::runtime::InteractionDirection::Send)
                                    | (
                                        "receive",
                                        gblab_core::runtime::InteractionDirection::Receive
                                    )
                            )
                    })
                    && (method.is_empty()
                        || log
                            .message
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().next())
                            .is_some_and(|value| value.eq_ignore_ascii_case(&method)))
                    && (keyword.is_empty() || log.message.to_ascii_lowercase().contains(&keyword))
            })
            .collect();
        filtered.sort_by_key(|log| log.sequence);
        let total = filtered.len();
        let page = filtered
            .into_iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .collect();
        Self {
            items: page,
            total,
            offset: query.offset,
            limit: query.limit.max(1),
        }
    }
}

impl DeviceSnapshotDto {
    pub fn from_core(value: DeviceSnapshot) -> Self {
        Self {
            devices: value.devices.into_iter().map(Into::into).collect(),
            has_completed_batch_add: value.has_completed_batch_add,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeviceDraftDto {
    count: u16,
    start_device_id: String,
    name_template: String,
    r#type: DeviceKindDto,
    manufacturer: String,
    model: String,
    firmware_version: String,
    channel_count: u16,
}

impl BatchDeviceDraftDto {
    pub fn into_core(self) -> BatchDeviceDraft {
        BatchDeviceDraft {
            count: self.count,
            start_device_id: self.start_device_id,
            name_template: self.name_template,
            kind: self.r#type.into(),
            manufacturer: self.manufacturer,
            model: self.model,
            firmware_version: self.firmware_version,
            channel_count: self.channel_count,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceUpdateDraftDto {
    name: String,
    r#type: DeviceKindDto,
    manufacturer: String,
    model: String,
    firmware_version: String,
    channel_count: u16,
}

impl DeviceUpdateDraftDto {
    pub fn into_core(self) -> DeviceUpdateDraft {
        DeviceUpdateDraft {
            name: self.name,
            kind: self.r#type.into(),
            manufacturer: self.manufacturer,
            model: self.model,
            firmware_version: self.firmware_version,
            channel_count: self.channel_count,
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
            code: "core_error",
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

    pub fn operation_busy() -> Self {
        Self {
            code: "operation_busy",
            message: "另一个设备或配置操作正在执行，请稍后重试。".to_owned(),
        }
    }

    pub fn registration_active() -> Self {
        Self {
            code: "registration_active",
            message: "请先完成全量停止注册，再修改 SIP 或设备配置。".to_owned(),
        }
    }

    pub fn registration(error: &RegistrationRuntimeError) -> Self {
        Self {
            code: "registration_error",
            message: error.to_string(),
        }
    }

    pub const fn invalid_configuration(message: String) -> Self {
        Self {
            code: "invalid_configuration",
            message,
        }
    }
}
