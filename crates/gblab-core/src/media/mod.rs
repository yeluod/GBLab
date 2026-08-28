//! 媒体源抽象与 MP4 解封装能力。
//!
//! 本模块负责本地媒体容器、摄像头输入和播放时钟，不负责编码、MPEG-PS、RTP
//! 或 SIP 会话。后续媒体传输可以复用 [`MediaPacket`] 和 [`MediaPipeline`] 边界。

#![expect(clippy::missing_errors_doc, reason = "媒体错误由 MediaError 统一表达")]

mod camera;
mod error;
mod mp4;
mod types;

pub use camera::{CameraMediaSource, CameraSession};
pub use error::MediaError;
pub use mp4::{Mp4MediaSource, Mp4Session};
pub use types::{
    AudioCodec, AudioStreamInfo, CameraCaptureSettings, CaptureDeviceInfo, CaptureDeviceLists,
    MediaPacket, MediaPipeline, MediaResult, MediaRuntimeStatus, MediaSource, MediaSourceKind,
    MediaSourceSession, MediaSourceStatus, Mp4ProbeResult, VideoCodec, VideoStreamInfo,
};

/// 媒体引擎，负责当前全局媒体源的生命周期。
pub struct MediaEngine {
    session: Option<MediaSourceSession>,
    status: MediaRuntimeStatus,
}

impl Default for MediaEngine {
    fn default() -> Self {
        Self {
            session: None,
            status: MediaRuntimeStatus::unconfigured(),
        }
    }
}

impl MediaEngine {
    /// 创建一个未配置的媒体引擎。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 探测 MP4 文件，但不改变当前播放会话。
    pub fn probe_mp4(path: &std::path::Path) -> MediaResult<Mp4ProbeResult> {
        Mp4MediaSource::new(path.to_owned()).probe()
    }

    /// 打开全局 MP4 源并准备播放。
    pub fn open_mp4(
        &mut self,
        path: &std::path::Path,
        looping: bool,
    ) -> MediaResult<MediaRuntimeStatus> {
        let source = Mp4MediaSource::new(path.to_owned());
        let session = source.open(looping)?;
        let probe = session.probe().clone();
        self.session = Some(session);
        self.status = MediaRuntimeStatus::ready(
            MediaSourceKind::Mp4,
            probe.video.clone(),
            probe.audio,
            probe.duration_seconds,
        );
        Ok(self.status.clone())
    }

    /// 打开全局摄像头源并准备采集。
    pub fn open_camera(
        &mut self,
        settings: &CameraCaptureSettings,
    ) -> MediaResult<MediaRuntimeStatus> {
        let source = CameraMediaSource::new(settings.clone());
        let session = source.open(false)?;
        let probe = session.probe().clone();
        self.session = Some(session);
        self.status = MediaRuntimeStatus::ready(
            MediaSourceKind::Camera,
            probe.video.clone(),
            probe.audio,
            None,
        );
        Ok(self.status.clone())
    }

    /// 探测摄像头当前协商出的输入能力。
    pub fn probe_camera(settings: &CameraCaptureSettings) -> MediaResult<Mp4ProbeResult> {
        CameraMediaSource::new(settings.clone()).probe()
    }

    /// 枚举当前平台的摄像头和麦克风输入设备。
    pub fn list_capture_devices() -> MediaResult<CaptureDeviceLists> {
        camera::list_capture_devices()
    }

    /// 当前源开始播放。
    pub fn play(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.play();
        self.status.source_status = MediaSourceStatus::Playing;
        Ok(self.status.clone())
    }

    /// 暂停当前源。
    pub fn pause(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.pause();
        self.status.source_status = MediaSourceStatus::Paused;
        Ok(self.status.clone())
    }

    /// 停止当前源并回到起点。
    pub fn stop(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.stop()?;
        self.status.source_status = MediaSourceStatus::Stopped;
        self.status.position_seconds = 0.0;
        Ok(self.status.clone())
    }

    /// 重置当前源到起点并保持就绪状态。
    pub fn reset(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.reset()?;
        self.status.source_status = MediaSourceStatus::Ready;
        self.status.position_seconds = 0.0;
        Ok(self.status.clone())
    }

    /// 获取当前播放状态。
    #[must_use]
    pub fn status(&self) -> MediaRuntimeStatus {
        self.status.clone()
    }

    /// 读取一个编码 packet 的时间线元数据。
    pub fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let packet = session.next_packet()?;
        if let Some(packet) = &packet {
            self.status.position_seconds = packet.position_seconds;
        }
        Ok(packet)
    }
}

impl MediaPipeline for MediaEngine {
    fn play(&mut self) -> MediaResult<MediaRuntimeStatus> {
        Self::play(self)
    }

    fn pause(&mut self) -> MediaResult<MediaRuntimeStatus> {
        Self::pause(self)
    }

    fn stop(&mut self) -> MediaResult<MediaRuntimeStatus> {
        Self::stop(self)
    }

    fn reset(&mut self) -> MediaResult<MediaRuntimeStatus> {
        Self::reset(self)
    }

    fn status(&self) -> MediaRuntimeStatus {
        Self::status(self)
    }

    fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>> {
        Self::next_packet(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaEngine, MediaError, MediaSourceStatus};

    #[test]
    fn new_engine_should_start_unconfigured() {
        let engine = MediaEngine::new();

        assert_eq!(
            engine.status().source_status,
            MediaSourceStatus::Unconfigured
        );
        assert!(engine.status().source_kind.is_none());
    }

    #[test]
    fn playback_commands_should_require_an_open_source() {
        let mut engine = MediaEngine::new();

        assert!(matches!(engine.play(), Err(MediaError::NoSourceOpen)));
        assert!(matches!(engine.pause(), Err(MediaError::NoSourceOpen)));
        assert!(matches!(engine.stop(), Err(MediaError::NoSourceOpen)));
        assert!(matches!(engine.reset(), Err(MediaError::NoSourceOpen)));
        assert!(matches!(
            engine.next_packet(),
            Err(MediaError::NoSourceOpen)
        ));
    }

    #[test]
    fn probe_should_report_missing_file_without_calling_ffmpeg() {
        let result = MediaEngine::probe_mp4(std::path::Path::new("/path/that/does/not/exist.mp4"));

        assert!(matches!(result, Err(MediaError::FileNotFound(_))));
    }
}
