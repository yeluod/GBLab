//! 与桌面框架和外部适配无关的领域类型与规则。

mod devices;
mod ids;

pub use devices::{
    BatchDeviceDraft, DeviceError, DeviceKind, DeviceSnapshot, DeviceUpdateDraft,
    MAX_BATCH_DEVICE_COUNT, MAX_CHANNEL_COUNT, SimulatedChannel, SimulatedDevice,
    derive_channels_for_device, validate_unique_channel_ids,
};
pub use ids::{DeviceId, DeviceIdError};
