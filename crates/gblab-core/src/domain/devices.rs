use std::{collections::HashSet, time::SystemTime};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::DeviceId;

/// 单次允许批量创建的最大设备数量。
pub const MAX_BATCH_DEVICE_COUNT: u16 = 1_000;
/// 单台设备允许配置的最大通道数量。
pub const MAX_CHANNEL_COUNT: u16 = 128;

const MAX_TEXT_LENGTH: usize = 128;

/// 模拟设备类型。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// 固定摄像机。
    Camera,
    /// 球形摄像机。
    PtzCamera,
    /// 网络视频录像机。
    Nvr,
    /// 门禁设备。
    AccessControl,
}

/// 写入 JSON 配置文件的模拟设备。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedDevice {
    /// 20 位 GB28181 国标编号。
    pub id: DeviceId,
    /// 设备名称。
    pub name: String,
    /// 设备类型。
    pub kind: DeviceKind,
    /// 制造商。
    pub manufacturer: String,
    /// 设备型号。
    pub model: String,
    /// 固件版本。
    pub firmware_version: String,
    /// 运行时派生的通道数量。
    pub channel_count: u16,
    /// 创建时间，Unix 毫秒。
    pub created_at: u64,
}

/// 批量新增设备的输入。
#[derive(Clone, Debug)]
pub struct BatchDeviceDraft {
    /// 批量数量。
    pub count: u16,
    /// 起始设备国标编号。
    pub start_device_id: String,
    /// 支持 `{序号}` 占位符的名称模板。
    pub name_template: String,
    /// 设备类型。
    pub kind: DeviceKind,
    /// 制造商。
    pub manufacturer: String,
    /// 设备型号。
    pub model: String,
    /// 固件版本。
    pub firmware_version: String,
    /// 每台设备的通道数量。
    pub channel_count: u16,
}

/// 编辑单台设备的输入；国标编号与创建时间不可修改。
#[derive(Clone, Debug)]
pub struct DeviceUpdateDraft {
    /// 设备名称。
    pub name: String,
    /// 设备类型。
    pub kind: DeviceKind,
    /// 制造商。
    pub manufacturer: String,
    /// 设备型号。
    pub model: String,
    /// 固件版本。
    pub firmware_version: String,
    /// 通道数量。
    pub channel_count: u16,
}

/// 由设备配置按规则实时派生的通道。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatedChannel {
    /// 20 位通道国标编号。
    pub id: DeviceId,
    /// 所属设备国标编号。
    pub device_id: DeviceId,
    /// 通道名称。
    pub name: String,
    /// 从 1 开始的通道序号。
    pub index: u16,
}

/// 返回给桌面端的设备与派生通道快照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceSnapshot {
    /// 已持久化的设备。
    pub devices: Vec<SimulatedDevice>,
    /// 当前是否已经完成唯一一次批量添加。
    pub has_completed_batch_add: bool,
    /// 根据设备配置即时生成的通道。
    pub channels: Vec<SimulatedChannel>,
}

impl DeviceSnapshot {
    /// 构建设备列表快照，不提前生成通道。
    #[must_use]
    pub const fn devices_only(
        devices: Vec<SimulatedDevice>,
        has_completed_batch_add: bool,
    ) -> Self {
        Self {
            devices,
            has_completed_batch_add,
            channels: Vec::new(),
        }
    }

    /// 从设备配置构建快照，通道只在内存中派生。
    ///
    /// # Errors
    ///
    /// 设备配置无法生成合法且全局唯一的通道编号时返回错误。
    pub fn derive(
        devices: Vec<SimulatedDevice>,
        has_completed_batch_add: bool,
    ) -> Result<Self, DeviceError> {
        let mut channel_ids = HashSet::new();
        let mut channels = Vec::new();
        for device in &devices {
            let derived = derive_channels_for_device(device)?;
            for channel in &derived {
                if !channel_ids.insert(channel.id.clone()) {
                    return Err(DeviceError::DuplicateChannelId(channel.id.to_string()));
                }
            }
            channels.extend(derived);
        }
        Ok(Self {
            devices,
            has_completed_batch_add,
            channels,
        })
    }
}

impl BatchDeviceDraft {
    /// 规范化并校验批量输入。
    ///
    /// # Errors
    ///
    /// 数量、文本、编号或通道数量不符合约束时返回错误。
    pub fn normalize_and_validate(mut self) -> Result<Self, DeviceError> {
        self.start_device_id = self.start_device_id.trim().to_owned();
        self.name_template = normalize_required_text("nameTemplate", &self.name_template)?;
        self.manufacturer = normalize_required_text("manufacturer", &self.manufacturer)?;
        self.model = normalize_required_text("model", &self.model)?;
        self.firmware_version = normalize_required_text("firmwareVersion", &self.firmware_version)?;
        validate_count(self.count)?;
        validate_channel_count(self.channel_count)?;
        DeviceId::new(self.start_device_id.clone())?;
        Ok(self)
    }

    /// 生成批量设备配置。
    ///
    /// # Errors
    ///
    /// 生成结果超过 20 位编号范围或不符合国标编码结构时返回错误。
    pub fn generate(self, created_at: SystemTime) -> Result<Vec<SimulatedDevice>, DeviceError> {
        let draft = self.normalize_and_validate()?;
        let start =
            draft
                .start_device_id
                .parse::<u128>()
                .map_err(|_| DeviceError::InvalidField {
                    field: "startDeviceId",
                    reason: "起始设备编号必须为 20 位数字",
                })?;
        let created_at = created_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| DeviceError::InvalidClock)?
            .as_millis()
            .try_into()
            .map_err(|_| DeviceError::InvalidClock)?;
        (0..draft.count)
            .map(|offset| {
                let raw_id = start
                    .checked_add(u128::from(offset))
                    .ok_or(DeviceError::DeviceIdOverflow)?;
                let id = DeviceId::new(format!("{raw_id:020}"))?;
                let sequence = usize::from(offset) + 1;
                Ok(SimulatedDevice {
                    id,
                    name: draft
                        .name_template
                        .replace("{序号}", &format!("{sequence:03}")),
                    kind: draft.kind,
                    manufacturer: draft.manufacturer.clone(),
                    model: draft.model.clone(),
                    firmware_version: draft.firmware_version.clone(),
                    channel_count: draft.channel_count,
                    created_at,
                })
            })
            .collect()
    }
}

impl DeviceUpdateDraft {
    /// 规范化并校验编辑输入。
    ///
    /// # Errors
    ///
    /// 任一文本或通道数量不符合约束时返回错误。
    pub fn normalize_and_validate(mut self) -> Result<Self, DeviceError> {
        self.name = normalize_required_text("name", &self.name)?;
        self.manufacturer = normalize_required_text("manufacturer", &self.manufacturer)?;
        self.model = normalize_required_text("model", &self.model)?;
        self.firmware_version = normalize_required_text("firmwareVersion", &self.firmware_version)?;
        validate_channel_count(self.channel_count)?;
        Ok(self)
    }
}

/// 根据单台设备配置即时生成通道，不进行持久化。
///
/// # Errors
///
/// 通道数量或生成的通道国标编号无效时返回错误。
pub fn derive_channels_for_device(
    device: &SimulatedDevice,
) -> Result<Vec<SimulatedChannel>, DeviceError> {
    validate_channel_count(device.channel_count)?;
    let raw = device.id.as_str();
    let channel_prefix = &raw[..14];
    let device_sequence = &raw[17..20];
    (1..=device.channel_count)
        .map(|index| {
            let id = DeviceId::new(format!("{channel_prefix}{device_sequence}{index:03}"))?;
            Ok(SimulatedChannel {
                id,
                device_id: device.id.clone(),
                name: format!("{} · 通道 {index:02}", device.name),
                index,
            })
        })
        .collect()
}

fn normalize_required_text(field: &'static str, value: &str) -> Result<String, DeviceError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.len() > MAX_TEXT_LENGTH {
        return Err(DeviceError::InvalidField {
            field,
            reason: "不能为空且长度不能超过 128 个字符",
        });
    }
    Ok(value)
}

fn validate_count(count: u16) -> Result<(), DeviceError> {
    if (1..=MAX_BATCH_DEVICE_COUNT).contains(&count) {
        Ok(())
    } else {
        Err(DeviceError::InvalidField {
            field: "count",
            reason: "设备数量必须介于 1 到 1000",
        })
    }
}

fn validate_channel_count(channel_count: u16) -> Result<(), DeviceError> {
    if (1..=MAX_CHANNEL_COUNT).contains(&channel_count) {
        Ok(())
    } else {
        Err(DeviceError::InvalidField {
            field: "channelCount",
            reason: "通道数量必须介于 1 到 128",
        })
    }
}

/// 设备配置与通道派生错误。
#[derive(Debug, Error)]
pub enum DeviceError {
    /// 设备国标编号无效。
    #[error(transparent)]
    InvalidDeviceId(#[from] super::DeviceIdError),
    /// 输入字段不符合约束。
    #[error("设备字段 {field} 无效: {reason}")]
    InvalidField {
        /// IPC 字段名。
        field: &'static str,
        /// 不包含输入内容的错误原因。
        reason: &'static str,
    },
    /// 设备编号数值递增后超过 20 位。
    #[error("批量生成的设备编号超出 20 位范围")]
    DeviceIdOverflow,
    /// 系统时间不能转换为 Unix 时间。
    #[error("系统时间无效")]
    InvalidClock,
    /// 唯一一次批量添加已经完成。
    #[error("设备仅允许批量添加一次")]
    BatchAlreadyCompleted,
    /// 设备编号已经存在。
    #[error("设备国标编号重复: {0}")]
    DuplicateDeviceId(String),
    /// 目标设备不存在。
    #[error("设备不存在或已被删除: {0}")]
    DeviceNotFound(String),
    /// 派生出了重复通道编号。
    #[error("通道国标编号重复: {0}")]
    DuplicateChannelId(String),
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{BatchDeviceDraft, DeviceKind, DeviceSnapshot};

    fn draft() -> BatchDeviceDraft {
        BatchDeviceDraft {
            count: 2,
            start_device_id: "34020000001320000100".to_owned(),
            name_template: "模拟设备-{序号}".to_owned(),
            kind: DeviceKind::Camera,
            manufacturer: "GBLab".to_owned(),
            model: "SIM-CAM-100".to_owned(),
            firmware_version: "V1.0.0".to_owned(),
            channel_count: 3,
        }
    }

    #[test]
    fn batch_should_generate_requested_devices_and_protocol_channel_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let devices = draft().generate(created_at)?;
        let snapshot = DeviceSnapshot::derive(devices, true)?;

        assert_eq!(snapshot.devices.len(), 2);
        assert_eq!(snapshot.devices[0].name, "模拟设备-001");
        assert_eq!(snapshot.channels.len(), 6);
        assert_eq!(snapshot.channels[0].id.as_str(), "34020000001320100001");
        assert_eq!(snapshot.channels[2].id.as_str(), "34020000001320100003");
        assert_eq!(snapshot.channels[3].id.as_str(), "34020000001320101001");
        Ok(())
    }

    #[test]
    fn snapshot_should_reject_duplicate_derived_channel_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut first = draft();
        first.count = 1;
        first.start_device_id = "34020000001320000001".to_owned();
        let mut second = draft();
        second.count = 1;
        second.start_device_id = "34020000001320001001".to_owned();
        let mut devices = first.generate(created_at)?;
        devices.extend(second.generate(created_at)?);

        assert!(DeviceSnapshot::derive(devices, true).is_err());
        Ok(())
    }
}
