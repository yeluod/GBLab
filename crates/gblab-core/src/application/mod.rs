//! 桌面应用可调用的用例与业务编排入口。

use std::{collections::HashSet, time::SystemTime};

use serde::Serialize;

use crate::{
    Result,
    configuration::{ConfigurationStore, SipServiceConfiguration},
    domain::{
        BatchDeviceDraft, DeviceError, DeviceId, DeviceSnapshot, DeviceUpdateDraft,
        SimulatedChannel, derive_channels_for_device, validate_unique_channel_ids,
    },
};

/// 单次设备分页最多返回的设备数量，避免 IPC 请求意外分配过大的集合。
const MAX_DEVICE_PAGE_SIZE: usize = 1_000;

/// `GBLab` 核心服务。
pub struct CoreService {
    configuration: ConfigurationStore,
}

/// 设备配置分页结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevicePage {
    /// 当前页设备。
    pub devices: Vec<crate::SimulatedDevice>,
    /// 过滤后的总数。
    pub total: usize,
    /// 起始偏移。
    pub offset: usize,
    /// 页大小。
    pub limit: usize,
    /// 是否已完成批量添加。
    pub has_completed_batch_add: bool,
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

    /// 返回持久化设备列表；通道由独立用例按单台设备加载。
    #[must_use]
    pub fn device_snapshot(&self) -> DeviceSnapshot {
        let collection = self.configuration.device_collection();
        let has_completed_batch_add =
            collection.has_completed_batch_add && !collection.devices.is_empty();
        DeviceSnapshot::devices_only(collection.devices, has_completed_batch_add)
    }

    /// 在配置存储上直接执行设备过滤、排序和分页。
    #[must_use]
    pub fn device_page(
        &self,
        offset: usize,
        limit: usize,
        filter: Option<&str>,
        sort: Option<&str>,
    ) -> DevicePage {
        let collection = self.configuration.device_collection();
        let filter = filter.unwrap_or_default().trim().to_ascii_lowercase();
        let mut devices: Vec<_> = collection
            .devices
            .iter()
            .filter(|device| {
                filter.is_empty()
                    || device.id.to_string().contains(&filter)
                    || device.name.to_ascii_lowercase().contains(&filter)
                    || device.manufacturer.to_ascii_lowercase().contains(&filter)
                    || device.model.to_ascii_lowercase().contains(&filter)
            })
            .cloned()
            .collect();
        match sort.unwrap_or("id-asc") {
            "id-desc" => devices.sort_by_key(|device| std::cmp::Reverse(device.id.to_string())),
            "name-asc" => devices.sort_by(|left, right| left.name.cmp(&right.name)),
            "name-desc" => devices.sort_by(|left, right| right.name.cmp(&left.name)),
            _ => devices.sort_by_key(|device| device.id.to_string()),
        }
        let total = devices.len();
        let limit = limit.clamp(1, MAX_DEVICE_PAGE_SIZE);
        let page = devices.into_iter().skip(offset).take(limit).collect();
        DevicePage {
            devices: page,
            total,
            offset,
            limit,
            has_completed_batch_add: collection.has_completed_batch_add
                && !collection.devices.is_empty(),
        }
    }

    /// 按需返回单台设备的派生通道，避免设备列表加载全部通道。
    ///
    /// # Errors
    ///
    /// 设备不存在或通道无法按国标规则生成时返回错误。
    pub fn device_channels(&self, device_id: &str) -> Result<Vec<SimulatedChannel>> {
        let device_id = DeviceId::new(device_id.to_owned()).map_err(DeviceError::from)?;
        let collection = self.configuration.device_collection();
        let device = collection
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .ok_or_else(|| DeviceError::DeviceNotFound(device_id.to_string()))?;
        Ok(derive_channels_for_device(device)?)
    }

    /// 执行唯一一次批量设备添加并写入 JSON。
    ///
    /// # Errors
    ///
    /// 批量添加已完成、输入无效、编号重复或配置写入失败时返回错误。
    pub fn add_devices_in_batch(&mut self, draft: BatchDeviceDraft) -> Result<DeviceSnapshot> {
        let mut collection = self.configuration.device_collection();
        if collection.has_completed_batch_add && !collection.devices.is_empty() {
            return Err(DeviceError::BatchAlreadyCompleted.into());
        }
        let generated = draft.generate(SystemTime::now())?;
        let existing_ids: HashSet<_> = collection.devices.iter().map(|item| &item.id).collect();
        if let Some(duplicate) = generated
            .iter()
            .find(|device| existing_ids.contains(&device.id))
        {
            return Err(DeviceError::DuplicateDeviceId(duplicate.id.to_string()).into());
        }
        collection.devices.extend(generated);
        collection.has_completed_batch_add = true;
        validate_unique_channel_ids(&collection.devices)?;
        let snapshot = DeviceSnapshot::devices_only(
            collection.devices.clone(),
            collection.has_completed_batch_add,
        );
        self.configuration.save_device_collection(collection)?;
        Ok(snapshot)
    }

    /// 清空全部设备配置并重新开放一次批量添加。
    ///
    /// SIP 服务配置保持不变；通道、注册状态和订阅不属于持久化设备配置，
    /// 会由调用方根据返回的空设备快照同步清理。
    ///
    /// # Errors
    ///
    /// 配置文件写入失败时返回错误。
    pub fn clear_devices(&mut self) -> Result<DeviceSnapshot> {
        let mut collection = self.configuration.device_collection();
        collection.devices.clear();
        collection.has_completed_batch_add = false;
        let snapshot = DeviceSnapshot::devices_only(collection.devices.clone(), false);
        self.configuration.save_device_collection(collection)?;
        Ok(snapshot)
    }

    /// 编辑设备并重新派生通道后写入 JSON。
    ///
    /// # Errors
    ///
    /// 设备不存在、输入无效、通道编号冲突或配置写入失败时返回错误。
    pub fn update_device(
        &mut self,
        device_id: &str,
        draft: DeviceUpdateDraft,
    ) -> Result<DeviceSnapshot> {
        let device_id = DeviceId::new(device_id.to_owned()).map_err(DeviceError::from)?;
        let draft = draft.normalize_and_validate()?;
        let mut collection = self.configuration.device_collection();
        let device = collection
            .devices
            .iter_mut()
            .find(|device| device.id == device_id)
            .ok_or_else(|| DeviceError::DeviceNotFound(device_id.to_string()))?;
        device.name = draft.name;
        device.kind = draft.kind;
        device.manufacturer = draft.manufacturer;
        device.model = draft.model;
        device.firmware_version = draft.firmware_version;
        device.channel_count = draft.channel_count;
        validate_unique_channel_ids(&collection.devices)?;
        let snapshot = DeviceSnapshot::devices_only(
            collection.devices.clone(),
            collection.has_completed_batch_add,
        );
        self.configuration.save_device_collection(collection)?;
        Ok(snapshot)
    }

    /// 删除设备配置；派生通道随之自然消失。
    ///
    /// # Errors
    ///
    /// 设备不存在或配置写入失败时返回错误。
    pub fn delete_device(&mut self, device_id: &str) -> Result<DeviceSnapshot> {
        let device_id = DeviceId::new(device_id.to_owned()).map_err(DeviceError::from)?;
        let mut collection = self.configuration.device_collection();
        let original_len = collection.devices.len();
        collection.devices.retain(|device| device.id != device_id);
        if collection.devices.len() == original_len {
            return Err(DeviceError::DeviceNotFound(device_id.to_string()).into());
        }
        validate_unique_channel_ids(&collection.devices)?;
        let snapshot = DeviceSnapshot::devices_only(
            collection.devices.clone(),
            collection.has_completed_batch_add,
        );
        self.configuration.save_device_collection(collection)?;
        Ok(snapshot)
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
    use crate::{
        configuration::{SignalCharset, SipServiceConfiguration, SipTransport},
        domain::{BatchDeviceDraft, DeviceKind, DeviceUpdateDraft},
    };

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
            local_bind_address: "0.0.0.0".to_owned(),
            advertised_address: "10.0.0.10".to_owned(),
            local_port: 5_060,
            register_expires: 3_600,
            keepalive_interval: 60,
            signal_charset: SignalCharset::Gb2312,
        };

        service.save_sip_service_configuration(configuration)?;

        assert_eq!(service.sip_service_configuration().uri, "sip:10.0.0.9:5060");
        Ok(())
    }

    fn batch_draft() -> BatchDeviceDraft {
        BatchDeviceDraft {
            count: 2,
            start_device_id: "34020000001320000100".to_owned(),
            name_template: "设备-{序号}".to_owned(),
            kind: DeviceKind::Camera,
            manufacturer: "GBLab".to_owned(),
            model: "SIM-100".to_owned(),
            firmware_version: "V1.0.0".to_owned(),
            channel_count: 2,
        }
    }

    #[test]
    fn device_mutations_should_persist_devices_but_not_channels_or_registration_status()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut service = CoreService::open(configuration_path.as_path())?;
        let added = service.add_devices_in_batch(batch_draft())?;
        assert_eq!(added.devices.len(), 2);
        assert!(added.channels.is_empty());

        let updated = service.update_device(
            "34020000001320000100",
            DeviceUpdateDraft {
                name: "更新设备".to_owned(),
                kind: DeviceKind::Nvr,
                manufacturer: "GBLab".to_owned(),
                model: "NVR-200".to_owned(),
                firmware_version: "V2.0.0".to_owned(),
                channel_count: 3,
            },
        )?;
        assert!(updated.channels.is_empty());

        let reloaded = CoreService::open(configuration_path.as_path())?;
        let snapshot = reloaded.device_snapshot();
        assert_eq!(snapshot.devices[0].name, "更新设备");
        assert_eq!(reloaded.device_channels("34020000001320000100")?.len(), 3);
        assert_eq!(reloaded.device_channels("34020000001320000101")?.len(), 2);
        let json = std::fs::read_to_string(configuration_path)?;
        assert!(!json.contains("channels"));
        assert!(!json.contains("registrationStatus"));
        Ok(())
    }

    #[test]
    fn batch_add_should_remain_single_use_after_restart() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut service = CoreService::open(configuration_path.as_path())?;
        service.add_devices_in_batch(batch_draft())?;
        drop(service);

        let mut reloaded = CoreService::open(configuration_path.as_path())?;

        assert!(reloaded.add_devices_in_batch(batch_draft()).is_err());
        Ok(())
    }

    #[test]
    fn device_page_should_bound_requested_page_size() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let configuration_path = directory.path().join("gblab.config.json");
        let mut service = CoreService::open(configuration_path.as_path())?;
        service.add_devices_in_batch(batch_draft())?;

        let page = service.device_page(0, usize::MAX, None, None);

        assert_eq!(page.limit, 1_000);
        assert_eq!(page.devices.len(), 2);
        Ok(())
    }
}
