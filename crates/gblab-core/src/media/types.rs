#![expect(
    clippy::derive_partial_eq_without_eq,
    reason = "媒体时间和帧率使用 f64，无法实现 Eq"
)]

use serde::{Deserialize, Serialize};

use super::{AudioCodec, MediaError, VideoCodec};
use crate::configuration::EncoderBackend;

/// 媒体操作结果。
pub type MediaResult<T> = Result<T, MediaError>;

/// 全局媒体源种类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaSourceKind {
    /// MP4 文件源。
    Mp4,
    /// 摄像头与可选麦克风采集源。
    Camera,
}

/// 视频流能力。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamInfo {
    /// 视频编码。
    pub codec: VideoCodec,
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// 平均帧率。
    pub frames_per_second: f64,
    /// 码率，单位 bit/s；未知时为 None。
    pub bitrate: Option<u64>,
    /// 时长，单位秒；未知时为 None。
    pub duration_seconds: Option<f64>,
}

/// 音频流能力。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamInfo {
    /// AAC 或其它已识别音频编码。
    pub codec: AudioCodec,
    /// 采样率。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u32,
    /// 码率，单位 bit/s；未知时为 None。
    pub bitrate: Option<u64>,
}

/// MP4 探测结果。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mp4ProbeResult {
    /// 被探测文件。
    pub file_path: String,
    /// 视频流信息。
    pub video: VideoStreamInfo,
    /// 音频流信息；没有音频是合法结果。
    pub audio: Option<AudioStreamInfo>,
    /// 容器时长，单位秒；未知时为 None。
    pub duration_seconds: Option<f64>,
    /// 容器码率，单位 bit/s；未知时为 None。
    pub bitrate: Option<u64>,
}

/// 摄像头采集输入配置。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraCaptureSettings {
    /// Stable camera device identifier.
    pub video_device_id: String,
    /// Target encoded video codec.
    pub video_codec: VideoCodec,
    /// Target video bitrate in bit/s.
    pub video_bitrate: u64,
    /// Requested encoder backend.
    pub encoder_backend: EncoderBackend,
    /// 是否同时打开音频输入。
    pub audio_enabled: bool,
    /// 麦克风设备标识。
    pub audio_device_id: String,
    /// Target encoded audio codec.
    pub audio_codec: AudioCodec,
    /// Target audio sample rate.
    pub audio_sample_rate: u32,
    /// Target audio channel count.
    pub audio_channels: u32,
    /// Target audio bitrate in bit/s.
    pub audio_bitrate: u64,
    /// 采集宽度。
    pub width: u32,
    /// 采集高度。
    pub height: u32,
    /// 采集帧率。
    pub frames_per_second: f64,
}

/// 原生采集设备的可选项。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDeviceInfo {
    /// `FFmpeg` 输入设备标识。
    pub id: String,
    /// 用于界面展示的设备名称。
    pub name: String,
}

/// 原生采集设备枚举结果。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDeviceLists {
    /// 摄像头设备。
    pub video: Vec<CaptureDeviceInfo>,
    /// 麦克风设备。
    pub audio: Vec<CaptureDeviceInfo>,
}

/// 摄像头支持的一组分辨率与帧率。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCaptureMode {
    /// 采集宽度。
    pub width: u32,
    /// 采集高度。
    pub height: u32,
    /// Frame-rate capabilities reported by the native backend.
    pub frame_rates: Vec<FrameRateCapability>,
}

/// An exact frame rate or a continuous native range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FrameRateCapability {
    /// One exact supported frame rate.
    Exact {
        /// Exact frames per second.
        value: f64,
    },
    /// Inclusive continuous range reported by the device.
    Range {
        /// Minimum frames per second.
        minimum: f64,
        /// Maximum frames per second.
        maximum: f64,
    },
}

/// 单个摄像头的原生采集能力。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCaptureCapabilities {
    /// 摄像头设备标识。
    pub device_id: String,
    /// 原生支持的采集模式。
    pub modes: Vec<VideoCaptureMode>,
}

/// 当前随应用链接的 `FFmpeg` 视频编码能力。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncoderCapabilities {
    /// Encoders actually present in the linked `FFmpeg` build.
    pub encoders: Vec<VideoEncoderCapability>,
}

/// One concrete encoder implementation available to the media runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncoderCapability {
    /// Output codec.
    pub codec: VideoCodec,
    /// User-facing backend family.
    pub backend: EncoderBackend,
    /// Exact `FFmpeg` encoder name.
    pub encoder_name: String,
    /// Whether the implementation is hardware accelerated.
    pub hardware: bool,
}

/// 播放状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaSourceStatus {
    /// 尚未配置源。
    Unconfigured,
    /// 已打开并位于起点。
    Ready,
    /// 正在播放。
    Playing,
    /// 已暂停。
    Paused,
    /// 已停止。
    Stopped,
}

/// 全局媒体运行状态。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRuntimeStatus {
    /// 当前源状态。
    pub source_status: MediaSourceStatus,
    /// 当前源类型。
    pub source_kind: Option<MediaSourceKind>,
    /// 视频流能力。
    pub video: Option<VideoStreamInfo>,
    /// 音频流能力。
    pub audio: Option<AudioStreamInfo>,
    /// 总时长，单位秒。
    pub duration_seconds: Option<f64>,
    /// 当前播放位置，单位秒。
    pub position_seconds: f64,
    /// 当前预览读取倍速。倍速只影响本地播放时钟，不改变源文件。
    pub playback_rate: f64,
    /// 已向界面输出的解码帧数。
    pub decoded_frames: u64,
    /// 音频是否静音。音频输出管线接入后继续复用此状态。
    pub muted: bool,
    /// 音量，范围 0.0 到 1.0。
    pub volume: f64,
    /// 最近一次 source worker 错误；正常打开新源后清除。
    pub last_error: Option<String>,
}

impl MediaRuntimeStatus {
    pub(crate) const fn unconfigured() -> Self {
        Self {
            source_status: MediaSourceStatus::Unconfigured,
            source_kind: None,
            video: None,
            audio: None,
            duration_seconds: None,
            position_seconds: 0.0,
            playback_rate: 1.0,
            decoded_frames: 0,
            muted: false,
            volume: 1.0,
            last_error: None,
        }
    }

    pub(crate) const fn ready(
        source_kind: MediaSourceKind,
        video: VideoStreamInfo,
        audio: Option<AudioStreamInfo>,
        duration_seconds: Option<f64>,
    ) -> Self {
        Self {
            source_status: MediaSourceStatus::Ready,
            source_kind: Some(source_kind),
            video: Some(video),
            audio,
            duration_seconds,
            position_seconds: 0.0,
            playback_rate: 1.0,
            decoded_frames: 0,
            muted: false,
            volume: 1.0,
            last_error: None,
        }
    }
}

/// 一帧可供界面预览的 RGBA 视频帧。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaVideoFrame {
    /// 帧宽度。
    pub width: u32,
    /// 帧高度。
    pub height: u32,
    /// 按行紧密排列的 RGBA 字节。
    pub rgba: Vec<u8>,
    /// 帧在源时间线中的位置。
    pub position_seconds: f64,
}

/// 媒体源探测和打开能力。
pub trait MediaSource {
    /// 探测源能力。
    fn probe(&self) -> MediaResult<Mp4ProbeResult>;
    /// 打开源会话。
    fn open(&self, looping: bool) -> MediaResult<MediaSourceSession>;
}

/// 已打开的媒体源会话。
pub enum MediaSourceSession {
    /// MP4 文件会话。
    Mp4(Box<super::mp4::Mp4Session>),
    /// 摄像头输入会话。
    Camera(Box<super::camera::CameraSession>),
}

/// One source read distributed by the single media worker.
pub struct SourceReadOutput {
    pub(crate) packet: Option<super::EncodedMediaPacket>,
    pub(crate) preview_frames: Vec<MediaVideoFrame>,
    pub(crate) looped: bool,
    pub(crate) end_of_stream: bool,
}

impl SourceReadOutput {
    pub(crate) const fn end_of_stream() -> Self {
        Self {
            packet: None,
            preview_frames: Vec::new(),
            looped: false,
            end_of_stream: true,
        }
    }

    pub(crate) const fn looped() -> Self {
        Self {
            packet: None,
            preview_frames: Vec::new(),
            looped: true,
            end_of_stream: false,
        }
    }
}

impl MediaSourceSession {
    pub(crate) const fn is_live_capture(&self) -> bool {
        matches!(self, Self::Camera(_))
    }

    pub(crate) const fn probe(&self) -> &Mp4ProbeResult {
        match self {
            Self::Mp4(session) => session.probe(),
            Self::Camera(session) => session.probe(),
        }
    }

    pub(crate) const fn play(&mut self) {
        match self {
            Self::Mp4(session) => {
                session.play();
            }
            Self::Camera(session) => {
                session.play();
            }
        }
    }

    pub(crate) const fn pause(&mut self) {
        match self {
            Self::Mp4(session) => {
                session.pause();
            }
            Self::Camera(session) => {
                session.pause();
            }
        }
    }

    pub(crate) fn stop(&mut self) -> MediaResult<()> {
        match self {
            Self::Mp4(session) => session.stop(),
            Self::Camera(session) => session.stop(),
        }
    }

    pub(crate) fn reset(&mut self) -> MediaResult<()> {
        match self {
            Self::Mp4(session) => session.reset(),
            Self::Camera(session) => session.reset(),
        }
    }

    pub(crate) fn seek_frame(
        &mut self,
        position_seconds: f64,
    ) -> MediaResult<Option<MediaVideoFrame>> {
        match self {
            Self::Mp4(session) => session.seek_frame(position_seconds),
            Self::Camera(_) => Err(MediaError::UnsupportedSource(
                "实时摄像头不支持时间线跳转".to_owned(),
            )),
        }
    }

    pub(crate) fn step_frame(&mut self) -> MediaResult<Option<MediaVideoFrame>> {
        match self {
            Self::Mp4(session) => session.step_frame(),
            Self::Camera(_) => Err(MediaError::UnsupportedSource(
                "实时摄像头不支持单帧步进".to_owned(),
            )),
        }
    }

    pub(crate) fn read_source_output(&mut self) -> MediaResult<SourceReadOutput> {
        match self {
            Self::Mp4(session) => session.read_source_output(),
            Self::Camera(session) => session.read_source_output(),
        }
    }

    pub(crate) fn finish_encoded_packets(&mut self) -> MediaResult<Vec<super::EncodedMediaPacket>> {
        match self {
            Self::Mp4(_) => Ok(Vec::new()),
            Self::Camera(session) => session.finish_encoded_packets(),
        }
    }
}
