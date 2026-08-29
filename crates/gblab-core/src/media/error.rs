use thiserror::Error;

/// 媒体源和 MP4 解封装错误。
#[derive(Debug, Error)]
pub enum MediaError {
    /// 文件路径不是有效的 C 字符串路径。
    #[error("媒体文件路径包含不支持的 NUL 字符")]
    InvalidPath,
    /// 文件不存在。
    #[error("媒体文件不存在: {0}")]
    FileNotFound(String),
    /// rsmpeg/FFmpeg 无法打开或读取文件。
    #[error("无法打开媒体文件 {path}: {message}")]
    OpenFailed {
        /// 无法打开的文件路径。
        path: String,
        /// `FFmpeg` 返回的底层错误。
        message: String,
    },
    /// 文件中没有视频流。
    #[error("MP4 文件不包含视频流")]
    MissingVideoStream,
    /// 文件中存在当前阶段不支持的视频编码。
    #[error("不支持的视频编码: {0}")]
    UnsupportedVideoCodec(String),
    /// 当前阶段不支持的媒体源。
    #[error("当前媒体源暂不支持: {0}")]
    UnsupportedSource(String),
    /// 播放会话尚未打开。
    #[error("尚未打开媒体源")]
    NoSourceOpen,
    /// 播放会话操作失败。
    #[error("媒体播放操作失败: {0}")]
    Playback(String),
    /// 摄像头设备输入不可用或配置无效。
    #[error("摄像头输入不可用: {0}")]
    Camera(String),
    /// Native speaker output or PCM preview conversion failed.
    #[error("本地音频预览失败: {0}")]
    AudioPreview(String),
    /// The dedicated media owner or one of its bounded channels is unavailable.
    #[error("媒体运行时不可用: {0}")]
    RuntimeUnavailable(String),
    /// A command exceeded the bounded worker response deadline.
    #[error("媒体命令执行超时")]
    CommandTimedOut,
}
