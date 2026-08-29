use std::ffi::CString;

use rsmpeg::{avcodec::AVCodecParametersRef, avformat::AVFormatContextInput, ffi};

use super::{
    AudioCodec, AudioStreamInfo, MediaError, MediaPacket, MediaResult, MediaSource,
    MediaSourceSession, Mp4ProbeResult, VideoCodec, VideoStreamInfo, decoder::VideoDecoder,
};

/// MP4 文件媒体源。
pub struct Mp4MediaSource {
    path: std::path::PathBuf,
}

impl Mp4MediaSource {
    /// 创建一个 MP4 源描述。
    #[must_use]
    pub const fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }

    fn open_context(&self) -> MediaResult<AVFormatContextInput> {
        if !self.path.is_file() {
            return Err(MediaError::FileNotFound(self.path.display().to_string()));
        }
        let path = self.path.to_str().ok_or(MediaError::InvalidPath)?;
        let path = CString::new(path).map_err(|_| MediaError::InvalidPath)?;
        AVFormatContextInput::open(path.as_c_str()).map_err(|error| MediaError::OpenFailed {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })
    }

    fn probe_context(&self, context: &AVFormatContextInput) -> MediaResult<Mp4ProbeResult> {
        let video_stream = context
            .streams()
            .iter()
            .find(|stream| stream.codecpar().codec_type().is_video())
            .ok_or(MediaError::MissingVideoStream)?;
        let video_parameters = video_stream.codecpar();
        let video = video_info(&video_parameters, video_stream)?;
        let audio = context
            .streams()
            .iter()
            .find(|stream| stream.codecpar().codec_type().is_audio())
            .map(|stream| {
                let parameters = stream.codecpar();
                audio_info(&parameters)
            })
            .transpose()?;
        let duration_seconds = timestamp_seconds(context.duration, 1_000_000);
        let bitrate = positive_u64(context.bit_rate);
        Ok(Mp4ProbeResult {
            file_path: self.path.display().to_string(),
            video,
            audio,
            duration_seconds,
            bitrate,
        })
    }
}

impl MediaSource for Mp4MediaSource {
    fn probe(&self) -> MediaResult<Mp4ProbeResult> {
        let context = self.open_context()?;
        self.probe_context(&context)
    }

    fn open(&self, looping: bool) -> MediaResult<MediaSourceSession> {
        let context = self.open_context()?;
        let probe = self.probe_context(&context)?;
        let decoder = {
            let video_stream = context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_video())
                .ok_or(MediaError::MissingVideoStream)?;
            let video_parameters = video_stream.codecpar();
            VideoDecoder::new(&video_parameters, video_stream)?
        };
        Ok(MediaSourceSession::Mp4(Mp4Session {
            context,
            probe,
            looping,
            playing: false,
            decoder,
        }))
    }
}

/// 已打开的 MP4 解封装会话。
pub struct Mp4Session {
    context: AVFormatContextInput,
    probe: Mp4ProbeResult,
    looping: bool,
    playing: bool,
    decoder: VideoDecoder,
}

impl Mp4Session {
    pub(crate) const fn probe(&self) -> &Mp4ProbeResult {
        &self.probe
    }
}

impl Mp4Session {
    pub(crate) const fn play(&mut self) {
        self.playing = true;
    }

    pub(crate) const fn pause(&mut self) {
        self.playing = false;
    }

    pub(crate) fn stop(&mut self) -> MediaResult<()> {
        self.playing = false;
        self.reset()
    }

    pub(crate) fn reset(&mut self) -> MediaResult<()> {
        self.seek(0.0)
    }

    pub(crate) fn seek(&mut self, position_seconds: f64) -> MediaResult<()> {
        if !position_seconds.is_finite() || position_seconds < 0.0 {
            return Err(MediaError::Playback(
                "跳转位置必须是非负有限数值".to_owned(),
            ));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "已校验时间值且 FFmpeg seek API 使用 i64 微秒"
        )]
        let timestamp = (position_seconds * 1_000_000.0).round() as i64;
        self.context
            .seek(-1, timestamp, ffi::AVSEEK_FLAG_BACKWARD.cast_signed())
            .map_err(|error| MediaError::Playback(error.to_string()))?;
        self.decoder.flush();
        Ok(())
    }

    pub(crate) fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>> {
        if !self.playing {
            return Ok(None);
        }
        let packet = match self.context.read_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) if self.looping => {
                self.reset()?;
                match self.context.read_packet() {
                    Ok(Some(packet)) => packet,
                    Ok(None) => return Ok(None),
                    Err(error) => return Err(MediaError::Playback(error.to_string())),
                }
            }
            Ok(None) => return Ok(None),
            Err(error) => return Err(MediaError::Playback(error.to_string())),
        };
        let stream_index = usize::try_from(packet.stream_index)
            .map_err(|_| MediaError::Playback("FFmpeg 返回了无效的 stream index".to_owned()))?;
        let stream = self
            .context
            .streams()
            .get(stream_index)
            .ok_or_else(|| MediaError::Playback("packet 引用了不存在的媒体流".to_owned()))?;
        let position_seconds =
            scaled_timestamp(packet.pts, stream.time_base.num, stream.time_base.den).unwrap_or(0.0);
        Ok(Some(MediaPacket {
            stream_index,
            pts: valid_timestamp(packet.pts),
            dts: valid_timestamp(packet.dts),
            duration: packet.duration,
            size: usize::try_from(packet.size).unwrap_or(0),
            is_keyframe: packet.flags & ffi::AV_PKT_FLAG_KEY.cast_signed() != 0,
            position_seconds,
        }))
    }

    pub(crate) fn next_frame(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        if !self.playing {
            return Ok(None);
        }
        self.decode_next_frame()
    }

    pub(crate) fn step_frame(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        self.decode_next_frame()
    }

    fn decode_next_frame(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        loop {
            let packet = match self.context.read_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) if self.looping => {
                    self.reset()?;
                    continue;
                }
                Ok(None) => return Ok(None),
                Err(error) => return Err(MediaError::Playback(error.to_string())),
            };
            if packet.stream_index < 0 {
                continue;
            }
            let stream_index = usize::try_from(packet.stream_index).unwrap_or(usize::MAX);
            let video_index = self
                .context
                .streams()
                .iter()
                .position(|stream| stream.codecpar().codec_type().is_video());
            if Some(stream_index) != video_index {
                continue;
            }
            if let Some(frame) = self.decoder.decode_packet(&packet)? {
                return Ok(Some(frame));
            }
        }
    }
}

fn video_info(
    parameters: &AVCodecParametersRef<'_>,
    stream: &rsmpeg::avformat::AVStreamRef<'_>,
) -> MediaResult<VideoStreamInfo> {
    let codec = match parameters.codec_id {
        ffi::AV_CODEC_ID_H264 => VideoCodec::H264,
        ffi::AV_CODEC_ID_HEVC => VideoCodec::H265,
        other => {
            return Err(MediaError::UnsupportedVideoCodec(format!(
                "FFmpeg codec id {other}"
            )));
        }
    };
    let fps = stream.guess_framerate().map_or(0.0, |rate| {
        if rate.den == 0 {
            0.0
        } else {
            f64::from(rate.num) / f64::from(rate.den)
        }
    });
    Ok(VideoStreamInfo {
        codec,
        width: u32::try_from(parameters.width)
            .map_err(|_| MediaError::Playback("视频宽度超出支持范围".to_owned()))?,
        height: u32::try_from(parameters.height)
            .map_err(|_| MediaError::Playback("视频高度超出支持范围".to_owned()))?,
        frames_per_second: fps,
        bitrate: positive_u64(parameters.bit_rate),
        duration_seconds: scaled_timestamp(
            stream.duration,
            stream.time_base.num,
            stream.time_base.den,
        ),
    })
}

fn audio_info(parameters: &AVCodecParametersRef<'_>) -> MediaResult<AudioStreamInfo> {
    let codec = if parameters.codec_id == ffi::AV_CODEC_ID_AAC {
        AudioCodec::Aac
    } else {
        return Err(MediaError::UnsupportedAudioCodec(format!(
            "FFmpeg codec id {}",
            parameters.codec_id
        )));
    };
    Ok(AudioStreamInfo {
        codec,
        sample_rate: u32::try_from(parameters.sample_rate)
            .map_err(|_| MediaError::Playback("音频采样率超出支持范围".to_owned()))?,
        channels: u32::try_from(parameters.ch_layout.nb_channels)
            .map_err(|_| MediaError::Playback("音频声道数超出支持范围".to_owned()))?,
        bitrate: positive_u64(parameters.bit_rate),
    })
}

fn positive_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|value| *value > 0)
}

fn valid_timestamp(value: i64) -> Option<i64> {
    (value != ffi::AV_NOPTS_VALUE).then_some(value)
}

#[expect(clippy::cast_precision_loss, reason = "播放位置 API 使用 f64 秒精度")]
fn timestamp_seconds(value: i64, time_base_denominator: i64) -> Option<f64> {
    (value >= 0 && time_base_denominator > 0).then_some(value as f64 / time_base_denominator as f64)
}

#[expect(clippy::cast_precision_loss, reason = "播放位置 API 使用 f64 秒精度")]
fn scaled_timestamp(value: i64, numerator: i32, denominator: i32) -> Option<f64> {
    (value >= 0 && numerator > 0 && denominator > 0)
        .then_some(value as f64 * f64::from(numerator) / f64::from(denominator))
}
