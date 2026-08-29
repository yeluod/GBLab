//! `GBLab` 的高性能 GB28181 多设备模拟核心。
//!
//! 本 crate 独立于 Tauri，负责领域模型、设备运行编排、Tokio 生命周期、
//! SIP/GB28181 适配和 JSON 配置存储。

#![deny(missing_docs)]

pub mod application;
pub mod configuration;
pub mod domain;
pub mod media;
pub mod runtime;
pub mod sip;

mod error;

pub use application::{CoreInfo, CoreService, DevicePage};
pub use configuration::{
    AudioCaptureConfiguration, AudioCodec, CameraSourceConfiguration,
    DeviceCollectionConfiguration, EncoderBackend, MediaConfiguration, MediaPreferences,
    MediaRecordingConfiguration, MediaSourceConfiguration, MediaSourceType, Mp4SourceConfiguration,
    SignalCharset, SipServiceConfiguration, SipTransport, VideoCaptureConfiguration, VideoCodec,
};
pub use domain::{
    BatchDeviceDraft, DeviceKind, DeviceSnapshot, DeviceUpdateDraft, SimulatedChannel,
    SimulatedDevice,
};
pub use error::{CoreError, Result};
pub use media::{
    CameraCaptureSettings, CaptureDeviceInfo, CaptureDeviceLists, GlobalMediaHandle,
    GlobalMediaRuntime, MediaError, MediaResult, MediaRuntimeStatus, MediaVideoFrame,
    Mp4ProbeResult, VideoCaptureCapabilities, VideoCaptureMode, VideoEncoderCapabilities,
    list_capture_devices, probe_mp4, video_capture_capabilities, video_encoder_capabilities,
};
