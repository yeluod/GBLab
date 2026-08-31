//! Single-owner global media runtime, source adapters and encoded-stream fan-out.
//!
//! 本模块负责 MP4 容器、音视频解码、统一播放时钟和编码流分发，不负责 MPEG-PS、RTP
//! 或 SIP 会话。后续媒体传输直接消费 [`EncodedMediaPacket`]。

#![expect(clippy::missing_errors_doc, reason = "媒体错误由 MediaError 统一表达")]

mod audio_preview;
mod clock;
mod decoder;
mod error;
mod hub;
mod media_coordinator;
mod media_session;
mod mp4;
mod packet;
mod ps;
mod rtp;
mod runtime;
mod types;

pub use clock::MediaClock;
pub use error::MediaError;
pub use hub::{
    BackpressurePolicy, BroadcastReport, MediaConsumerKind, MediaStreamHub, MediaSubscription,
};
pub use media_coordinator::{MediaCoordinatorConfig, MediaSessionCoordinator};
pub use media_session::{MediaSession, MediaSessionStats};
use mp4::Mp4MediaSource;
pub use packet::{
    AudioCodec, CodecConfigurationFormat, EncodedMediaCodec, EncodedMediaPacket,
    EncodedStreamDescriptor, FrameRate, MediaTimeBase, MediaTrackKind, VideoCodec,
};
pub use ps::mux_video_packet;
pub use rtp::RtpPacketizer;
pub use runtime::{GlobalMediaHandle, GlobalMediaRuntime};
pub use types::{
    AudioSinkInfo, AudioSinkStatus, AudioStreamInfo, MediaResult, MediaRuntimeMetrics,
    MediaRuntimeStatus, MediaSourceKind, MediaSourceStatus, MediaVideoFrame, Mp4ProbeResult,
    VideoStreamInfo,
};

/// Probes an MP4 file without opening the global runtime source.
pub fn probe_mp4(path: &std::path::Path) -> MediaResult<Mp4ProbeResult> {
    use types::MediaSource;
    Mp4MediaSource::new(path.to_owned()).probe()
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
