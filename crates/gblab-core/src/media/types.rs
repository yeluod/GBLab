#![expect(
    clippy::derive_partial_eq_without_eq,
    reason = "媒体时间和帧率使用 f64，无法实现 Eq"
)]
#![expect(clippy::missing_errors_doc, reason = "媒体错误由 MediaError 统一表达")]

use serde::{Deserialize, Serialize};

use super::MediaError;

/// 媒体操作结果。
pub type MediaResult<T> = Result<T, MediaError>;

/// 全局媒体源种类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaSourceKind {
    /// MP4 文件源。
    Mp4,
    /// 摄像头源，当前阶段只保留领域枚举。
    Camera,
}

/// 视频编码。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoCodec {
    /// H.264/AVC。
    H264,
    /// H.265/HEVC。
    H265,
    /// 摄像头输入的原始视频格式，尚未编码。
    RawVideo,
}

/// 音频编码。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioCodec {
    /// AAC。
    Aac,
    /// 其它音频编码，MP4 探测会明确标记但不纳入当前传输能力。
    Other,
    /// 摄像头输入的 PCM 音频，尚未编码。
    Pcm,
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraCaptureSettings {
    /// 摄像头设备标识；macOS 通常为设备索引，Windows 为 `DirectShow` 名称。
    pub video_device_id: String,
    /// 是否同时打开音频输入。
    pub audio_enabled: bool,
    /// 麦克风设备标识。
    pub audio_device_id: String,
    /// 采集宽度。
    pub width: u32,
    /// 采集高度。
    pub height: u32,
    /// 采集帧率。
    pub frames_per_second: u32,
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCaptureMode {
    /// 采集宽度。
    pub width: u32,
    /// 采集高度。
    pub height: u32,
    /// 该分辨率支持的实用整数帧率。
    pub supported_frames_per_second: Vec<u32>,
}

/// 单个摄像头的原生采集能力。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
    /// 当前真正存在的编码器所对应的视频编码。
    pub supported_codecs: Vec<VideoCodec>,
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
        }
    }
}

/// 编码 packet 的时间线元数据。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPacket {
    /// 流索引。
    pub stream_index: usize,
    /// PTS，单位为源流 time base。
    pub pts: Option<i64>,
    /// DTS，单位为源流 time base。
    pub dts: Option<i64>,
    /// packet 时长，单位为源流 time base。
    pub duration: i64,
    /// packet 大小，单位字节。
    pub size: usize,
    /// 是否为关键帧。
    pub is_keyframe: bool,
    /// 相对于媒体起点的秒数。
    pub position_seconds: f64,
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

/// 可被播放引擎驱动的媒体管线。
pub trait MediaPipeline {
    /// 开始播放。
    fn play(&mut self) -> MediaResult<MediaRuntimeStatus>;
    /// 暂停播放。
    fn pause(&mut self) -> MediaResult<MediaRuntimeStatus>;
    /// 停止并回到起点。
    fn stop(&mut self) -> MediaResult<MediaRuntimeStatus>;
    /// 重置到起点。
    fn reset(&mut self) -> MediaResult<MediaRuntimeStatus>;
    /// 跳转到 MP4 时间线中的指定位置。
    fn seek(&mut self, position_seconds: f64) -> MediaResult<MediaRuntimeStatus>;
    /// 设置本地预览倍速。
    fn set_playback_rate(&mut self, rate: f64) -> MediaResult<MediaRuntimeStatus>;
    /// 在暂停状态读取一帧。
    fn step_frame(&mut self) -> MediaResult<Option<MediaVideoFrame>>;
    /// 获取运行状态。
    fn status(&self) -> MediaRuntimeStatus;
    /// 读取下一个 packet 元数据。
    fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>>;
    /// 读取下一帧解码后的 RGBA 图像。
    fn next_frame(&mut self) -> MediaResult<Option<MediaVideoFrame>>;
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
    Mp4(super::Mp4Session),
    /// 摄像头输入会话。
    Camera(super::CameraSession),
}

impl MediaSourceSession {
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

    pub(crate) fn seek(&mut self, position_seconds: f64) -> MediaResult<()> {
        match self {
            Self::Mp4(session) => session.seek(position_seconds),
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

    pub(crate) fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>> {
        match self {
            Self::Mp4(session) => session.next_packet(),
            Self::Camera(session) => session.next_packet(),
        }
    }

    pub(crate) fn next_frame(&mut self) -> MediaResult<Option<MediaVideoFrame>> {
        match self {
            Self::Mp4(session) => session.next_frame(),
            Self::Camera(session) => session.next_frame(),
        }
    }
}
