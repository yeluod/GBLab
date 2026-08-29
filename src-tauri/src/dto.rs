use gblab_core::{
    BatchDeviceDraft, CoreError, CoreInfo, DeviceKind, DeviceSnapshot, DeviceUpdateDraft,
    MediaPacket, MediaRuntimeStatus, MediaVideoFrame, Mp4ProbeResult, SignalCharset,
    SimulatedChannel, SimulatedDevice, SipServiceConfiguration, SipTransport,
    VideoCaptureCapabilities, VideoEncoderCapabilities,
    runtime::{BatchOperationAccepted, RegistrationRuntimeError},
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

#[derive(Clone, Copy, Deserialize, Serialize)]
pub enum SignalCharsetDto {
    #[serde(rename = "GB2312")]
    Gb2312,
    #[serde(rename = "GBK")]
    Gbk,
    #[serde(rename = "UTF-8")]
    Utf8,
}

impl From<SignalCharset> for SignalCharsetDto {
    fn from(value: SignalCharset) -> Self {
        match value {
            SignalCharset::Gb2312 => Self::Gb2312,
            SignalCharset::Gbk => Self::Gbk,
            SignalCharset::Utf8 => Self::Utf8,
        }
    }
}

impl From<SignalCharsetDto> for SignalCharset {
    fn from(value: SignalCharsetDto) -> Self {
        match value {
            SignalCharsetDto::Gb2312 => Self::Gb2312,
            SignalCharsetDto::Gbk => Self::Gbk,
            SignalCharsetDto::Utf8 => Self::Utf8,
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
    signal_charset: SignalCharsetDto,
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
            signal_charset: configuration.signal_charset.into(),
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
            signal_charset: self.signal_charset.into(),
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
    pub fn from_page(page: gblab_core::DevicePage) -> Self {
        Self {
            devices: page.devices.into_iter().map(Into::into).collect(),
            total: page.total,
            offset: page.offset,
            limit: page.limit,
            has_completed_batch_add: page.has_completed_batch_add,
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
            message: "请先完成全量停止注册，再修改全局配置或设备数据。".to_owned(),
        }
    }

    pub fn registration(error: &RegistrationRuntimeError) -> Self {
        let code = match error {
            RegistrationRuntimeError::MissingActiveSubscription(_) => "subscription_unavailable",
            RegistrationRuntimeError::BusinessUnavailable => "business_unavailable",
            RegistrationRuntimeError::BusinessFailed(_) => "business_failed",
            _ => "registration_error",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }

    pub const fn invalid_configuration(message: String) -> Self {
        Self {
            code: "invalid_configuration",
            message,
        }
    }

    pub fn media(error: &gblab_core::MediaError) -> Self {
        Self {
            code: "media_error",
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRuntimeStatusDto {
    source_status: gblab_core::media::MediaSourceStatus,
    source_kind: Option<gblab_core::media::MediaSourceKind>,
    video: Option<gblab_core::media::VideoStreamInfo>,
    audio: Option<gblab_core::media::AudioStreamInfo>,
    duration_seconds: Option<f64>,
    position_seconds: f64,
    playback_rate: f64,
    decoded_frames: u64,
    muted: bool,
    volume: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDeviceInfoDto {
    id: String,
    name: String,
    status: &'static str,
}

impl From<gblab_core::CaptureDeviceInfo> for CaptureDeviceInfoDto {
    fn from(value: gblab_core::CaptureDeviceInfo) -> Self {
        Self {
            id: value.id,
            name: value.name,
            status: "available",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDeviceListsDto {
    video: Vec<CaptureDeviceInfoDto>,
    audio: Vec<CaptureDeviceInfoDto>,
}

impl From<gblab_core::CaptureDeviceLists> for CaptureDeviceListsDto {
    fn from(value: gblab_core::CaptureDeviceLists) -> Self {
        Self {
            video: value.video.into_iter().map(Into::into).collect(),
            audio: value.audio.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCaptureCapabilitiesDto {
    device_id: String,
    modes: Vec<gblab_core::VideoCaptureMode>,
}

impl From<VideoCaptureCapabilities> for VideoCaptureCapabilitiesDto {
    fn from(value: VideoCaptureCapabilities) -> Self {
        Self {
            device_id: value.device_id,
            modes: value.modes,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncoderCapabilitiesDto {
    supported_codecs: Vec<gblab_core::media::VideoCodec>,
}

impl From<VideoEncoderCapabilities> for VideoEncoderCapabilitiesDto {
    fn from(value: VideoEncoderCapabilities) -> Self {
        Self {
            supported_codecs: value.supported_codecs,
        }
    }
}

impl From<MediaRuntimeStatus> for MediaRuntimeStatusDto {
    fn from(value: MediaRuntimeStatus) -> Self {
        Self {
            source_status: value.source_status,
            source_kind: value.source_kind,
            video: value.video,
            audio: value.audio,
            duration_seconds: value.duration_seconds,
            position_seconds: value.position_seconds,
            playback_rate: value.playback_rate,
            decoded_frames: value.decoded_frames,
            muted: value.muted,
            volume: value.volume,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mp4ProbeResultDto {
    file_path: String,
    video: gblab_core::media::VideoStreamInfo,
    audio: Option<gblab_core::media::AudioStreamInfo>,
    duration_seconds: Option<f64>,
    bitrate: Option<u64>,
}

impl From<Mp4ProbeResult> for Mp4ProbeResultDto {
    fn from(value: Mp4ProbeResult) -> Self {
        Self {
            file_path: value.file_path,
            video: value.video,
            audio: value.audio,
            duration_seconds: value.duration_seconds,
            bitrate: value.bitrate,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPacketDto {
    stream_index: usize,
    pts: Option<i64>,
    dts: Option<i64>,
    duration: i64,
    size: usize,
    is_keyframe: bool,
    position_seconds: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaVideoFrameDto {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    position_seconds: f64,
}

impl From<MediaVideoFrame> for MediaVideoFrameDto {
    fn from(value: MediaVideoFrame) -> Self {
        Self {
            width: value.width,
            height: value.height,
            rgba: value.rgba,
            position_seconds: value.position_seconds,
        }
    }
}

impl From<MediaPacket> for MediaPacketDto {
    fn from(value: MediaPacket) -> Self {
        Self {
            stream_index: value.stream_index,
            pts: value.pts,
            dts: value.dts,
            duration: value.duration,
            size: value.size,
            is_keyframe: value.is_keyframe,
            position_seconds: value.position_seconds,
        }
    }
}
