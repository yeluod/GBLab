//! Single-owner global media runtime, source adapters and encoded-stream fan-out.
//!
//! 本模块负责本地媒体容器、摄像头采集、音视频编码、统一播放时钟和编码流分发，
//! 不负责 MPEG-PS、RTP 或 SIP 会话。后续媒体传输直接消费 [`EncodedMediaPacket`]。

#![expect(clippy::missing_errors_doc, reason = "媒体错误由 MediaError 统一表达")]

mod audio_encoder;
mod audio_preview;
mod camera;
mod clock;
mod decoder;
mod error;
mod hub;
mod mp4;
mod packet;
mod runtime;
mod types;
mod video_encoder;

use camera::CameraMediaSource;
pub use clock::MediaClock;
pub use error::MediaError;
pub use hub::{
    BackpressurePolicy, BroadcastReport, MediaConsumerKind, MediaStreamHub, MediaSubscription,
};
use mp4::Mp4MediaSource;
pub use packet::{
    AudioCodec, CodecConfigurationFormat, EncodedMediaCodec, EncodedMediaPacket, EncodedOutputInfo,
    EncodedStreamDescriptor, FrameRate, MediaTimeBase, MediaTrackKind, RawAudioFrame,
    RawVideoFrame, VideoCodec,
};
pub use runtime::{GlobalMediaHandle, GlobalMediaRuntime};
pub use types::{
    AudioSinkInfo, AudioSinkStatus, AudioStreamInfo, CameraCaptureSettings, CaptureDeviceInfo,
    CaptureDeviceLists, FrameRateCapability, MediaResult, MediaRuntimeMetrics, MediaRuntimeStatus,
    MediaSourceKind, MediaSourceStatus, MediaVideoFrame, Mp4ProbeResult, VideoCaptureCapabilities,
    VideoCaptureMode, VideoEncoderCapabilities, VideoEncoderCapability, VideoStreamInfo,
};

/// Probes an MP4 file without opening the global runtime source.
pub fn probe_mp4(path: &std::path::Path) -> MediaResult<Mp4ProbeResult> {
    use types::MediaSource;
    Mp4MediaSource::new(path.to_owned()).probe()
}

/// Enumerates native camera and microphone inputs.
pub fn list_capture_devices() -> MediaResult<CaptureDeviceLists> {
    camera::list_capture_devices()
}

/// Returns native capture modes for one stable camera identifier.
pub fn video_capture_capabilities(device_id: &str) -> MediaResult<VideoCaptureCapabilities> {
    camera::video_capture_capabilities(device_id)
}

/// Returns concrete encoders present in the linked `FFmpeg` libraries.
#[must_use]
pub fn video_encoder_capabilities() -> VideoEncoderCapabilities {
    camera::video_encoder_capabilities()
}

#[cfg(test)]
mod tests {
    use super::{MediaError, probe_mp4};

    #[test]
    fn probe_should_report_missing_file_without_calling_ffmpeg() {
        let result = probe_mp4(std::path::Path::new("/path/that/does/not/exist.mp4"));

        assert!(matches!(result, Err(MediaError::FileNotFound(_))));
    }
}
