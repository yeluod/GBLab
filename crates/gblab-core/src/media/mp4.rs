use std::{collections::VecDeque, ffi::CString};

use bytes::Bytes;
use rsmpeg::{
    avcodec::{
        AVBSFContext, AVBSFContextUninit, AVBitStreamFilter, AVCodecParameters,
        AVCodecParametersRef, AVPacket,
    },
    avformat::AVFormatContextInput,
    error::RsmpegError,
    ffi,
};

use super::{
    AudioCodec, AudioStreamInfo, EncodedMediaCodec, EncodedMediaPacket, MediaError, MediaResult,
    MediaTimeBase, MediaTrackKind, Mp4ProbeResult, VideoCodec, VideoStreamInfo,
    decoder::VideoDecoder,
    types::{MediaSource, MediaSourceSession, SourceReadOutput},
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
        let video_stream_index = context
            .streams()
            .iter()
            .position(|stream| stream.codecpar().codec_type().is_video())
            .ok_or(MediaError::MissingVideoStream)?;
        let video_bsf = {
            let video_stream = context
                .streams()
                .get(video_stream_index)
                .ok_or(MediaError::MissingVideoStream)?;
            create_annex_b_filter(&video_stream.codecpar(), video_stream.time_base)?
        };
        let video_codec_configuration = gblab_ffmpeg_device::copy_codec_extradata(
            &context
                .streams()
                .get(video_stream_index)
                .ok_or(MediaError::MissingVideoStream)?
                .codecpar(),
        )
        .map(Bytes::from);
        let audio_stream_index = context
            .streams()
            .iter()
            .position(|stream| stream.codecpar().codec_type().is_audio());
        let audio_codec_configuration = audio_stream_index
            .and_then(|index| context.streams().get(index))
            .and_then(|stream| gblab_ffmpeg_device::copy_codec_extradata(&stream.codecpar()))
            .map(Bytes::from);
        Ok(MediaSourceSession::Mp4(Box::new(Mp4Session {
            context,
            probe,
            looping,
            playing: false,
            decoder,
            video_stream_index,
            audio_stream_index,
            video_bsf,
            video_codec_configuration,
            audio_codec_configuration,
            video_config_sent: false,
            audio_config_sent: false,
            pending_packets: VecDeque::new(),
        })))
    }
}

/// 已打开的 MP4 解封装会话。
#[expect(
    clippy::struct_excessive_bools,
    reason = "loop/play/config-sent are independent FFmpeg session flags"
)]
pub struct Mp4Session {
    context: AVFormatContextInput,
    probe: Mp4ProbeResult,
    looping: bool,
    playing: bool,
    decoder: VideoDecoder,
    video_stream_index: usize,
    audio_stream_index: Option<usize>,
    video_bsf: AVBSFContext,
    video_codec_configuration: Option<Bytes>,
    audio_codec_configuration: Option<Bytes>,
    video_config_sent: bool,
    audio_config_sent: bool,
    pending_packets: VecDeque<EncodedMediaPacket>,
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
        let stream = self
            .context
            .streams()
            .get(self.video_stream_index)
            .ok_or_else(|| MediaError::Playback("视频流已不可用".to_owned()))?;
        if stream.time_base.num <= 0 || stream.time_base.den <= 0 {
            return Err(MediaError::Playback("视频流时间基无效".to_owned()));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "已校验时间值且 FFmpeg seek API 使用 i64 时间戳"
        )]
        let timestamp = (position_seconds * f64::from(stream.time_base.den)
            / f64::from(stream.time_base.num))
        .round() as i64;
        let stream_index = i32::try_from(self.video_stream_index)
            .map_err(|_| MediaError::Playback("视频流索引超出支持范围".to_owned()))?;
        self.context
            .seek(
                stream_index,
                timestamp,
                ffi::AVSEEK_FLAG_BACKWARD.cast_signed(),
            )
            .map_err(|error| MediaError::Playback(error.to_string()))?;
        self.decoder.flush();
        self.video_bsf.flush();
        self.pending_packets.clear();
        self.video_config_sent = false;
        self.audio_config_sent = false;
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "Single demux and BSF boundary keeps packet ordering explicit"
    )]
    pub(crate) fn read_source_output(&mut self) -> MediaResult<SourceReadOutput> {
        if let Some(packet) = self.pending_packets.pop_front() {
            return Ok(SourceReadOutput {
                packet: Some(packet),
                preview_frames: Vec::new(),
                looped: false,
                end_of_stream: false,
            });
        }
        let mut packet = match self.context.read_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) if self.looping => {
                self.reset()?;
                return Ok(SourceReadOutput::looped());
            }
            Ok(None) => {
                let preview_frames = self.decoder.finish_preview()?;
                let video_time_base = self
                    .context
                    .streams()
                    .get(self.video_stream_index)
                    .and_then(|stream| {
                        MediaTimeBase::new(stream.time_base.num, stream.time_base.den)
                    })
                    .ok_or_else(|| MediaError::Playback("视频流时间基无效".to_owned()))?;
                self.video_bsf.send_packet(None).map_err(|error| {
                    MediaError::Playback(format!("结束 Annex-B 过滤器失败：{error}"))
                })?;
                let mut drain_packet = AVPacket::new();
                loop {
                    match self.video_bsf.receive_packet(&mut drain_packet) {
                        Ok(()) => {
                            let mut output = encoded_packet(
                                &drain_packet,
                                MediaTrackKind::Video,
                                EncodedMediaCodec::Video(self.probe.video.codec),
                                video_time_base,
                            );
                            if !self.video_config_sent {
                                output
                                    .codec_configuration
                                    .clone_from(&self.video_codec_configuration);
                                self.video_config_sent = true;
                            }
                            self.pending_packets.push_back(output);
                        }
                        Err(
                            RsmpegError::BitstreamDrainError | RsmpegError::BitstreamFlushedError,
                        ) => break,
                        Err(error) => {
                            return Err(MediaError::Playback(format!(
                                "排空 Annex-B 过滤器失败：{error}"
                            )));
                        }
                    }
                }
                let packet = self.pending_packets.pop_front();
                return Ok(SourceReadOutput {
                    end_of_stream: packet.is_none(),
                    packet,
                    preview_frames,
                    looped: false,
                });
            }
            Err(error) => return Err(MediaError::Playback(error.to_string())),
        };
        let stream_index = usize::try_from(packet.stream_index)
            .map_err(|_| MediaError::Playback("FFmpeg 返回了无效的 stream index".to_owned()))?;
        let stream = self
            .context
            .streams()
            .get(stream_index)
            .ok_or_else(|| MediaError::Playback("packet 引用了不存在的媒体流".to_owned()))?;
        let time_base = MediaTimeBase::new(stream.time_base.num, stream.time_base.den)
            .ok_or_else(|| MediaError::Playback("媒体流时间基无效".to_owned()))?;

        if stream_index == self.video_stream_index {
            let mut preview_frames = Vec::new();
            if let Some(frame) = self.decoder.decode_packet(&packet)? {
                preview_frames.push(frame);
            }
            while let Some(frame) = self.decoder.take_pending_frame() {
                preview_frames.push(frame);
            }
            self.video_bsf
                .send_packet(Some(&mut packet))
                .map_err(|error| MediaError::Playback(format!("Annex-B 过滤失败：{error}")))?;
            loop {
                match self.video_bsf.receive_packet(&mut packet) {
                    Ok(()) => {
                        let mut output = encoded_packet(
                            &packet,
                            MediaTrackKind::Video,
                            EncodedMediaCodec::Video(self.probe.video.codec),
                            time_base,
                        );
                        if !self.video_config_sent {
                            output
                                .codec_configuration
                                .clone_from(&self.video_codec_configuration);
                            self.video_config_sent = true;
                        }
                        self.pending_packets.push_back(output);
                    }
                    Err(RsmpegError::BitstreamDrainError | RsmpegError::BitstreamFlushedError) => {
                        break;
                    }
                    Err(error) => {
                        return Err(MediaError::Playback(format!(
                            "读取 Annex-B packet 失败：{error}"
                        )));
                    }
                }
            }
            return Ok(SourceReadOutput {
                packet: self.pending_packets.pop_front(),
                preview_frames,
                looped: false,
                end_of_stream: false,
            });
        }

        if self.audio_stream_index == Some(stream_index)
            && self.probe.audio.as_ref().map(|audio| audio.codec) == Some(AudioCodec::Aac)
        {
            let mut encoded = encoded_packet(
                &packet,
                MediaTrackKind::Audio,
                EncodedMediaCodec::Audio(AudioCodec::Aac),
                time_base,
            );
            if !self.audio_config_sent {
                encoded
                    .codec_configuration
                    .clone_from(&self.audio_codec_configuration);
                self.audio_config_sent = true;
            }
            return Ok(SourceReadOutput {
                packet: Some(encoded),
                preview_frames: Vec::new(),
                looped: false,
                end_of_stream: false,
            });
        }

        Ok(SourceReadOutput {
            packet: None,
            preview_frames: Vec::new(),
            looped: false,
            end_of_stream: false,
        })
    }

    pub(crate) fn seek_frame(
        &mut self,
        position_seconds: f64,
    ) -> MediaResult<Option<super::MediaVideoFrame>> {
        self.seek(position_seconds)?;
        let frame_tolerance = if self.probe.video.frames_per_second.is_normal() {
            0.5 / self.probe.video.frames_per_second
        } else {
            0.02
        };
        let mut last_frame = None;

        for _ in 0..10_000 {
            let Some(frame) = self.decode_next_frame_without_loop()? else {
                return Ok(last_frame);
            };
            let reached_target = frame.position_seconds + frame_tolerance >= position_seconds;
            last_frame = Some(frame);
            if reached_target {
                return Ok(last_frame);
            }
        }

        Err(MediaError::Playback(
            "跳转后未能在安全帧数范围内定位目标画面".to_owned(),
        ))
    }

    pub(crate) fn step_frame(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        self.decode_next_frame()
    }

    fn decode_next_frame(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        self.decode_next_frame_with_looping(self.looping)
    }

    fn decode_next_frame_without_loop(&mut self) -> MediaResult<Option<super::MediaVideoFrame>> {
        self.decode_next_frame_with_looping(false)
    }

    fn decode_next_frame_with_looping(
        &mut self,
        restart_on_eof: bool,
    ) -> MediaResult<Option<super::MediaVideoFrame>> {
        loop {
            if let Some(frame) = self.decoder.take_pending_frame() {
                return Ok(Some(frame));
            }
            let packet = match self.context.read_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) if restart_on_eof => {
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
            if stream_index != self.video_stream_index {
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
        AudioCodec::Other
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

fn create_annex_b_filter(
    parameters: &AVCodecParametersRef<'_>,
    time_base: ffi::AVRational,
) -> MediaResult<AVBSFContext> {
    let name = match parameters.codec_id {
        ffi::AV_CODEC_ID_H264 => c"h264_mp4toannexb",
        ffi::AV_CODEC_ID_HEVC => c"hevc_mp4toannexb",
        _ => {
            return Err(MediaError::UnsupportedVideoCodec(
                "Annex-B 过滤器仅支持 H.264/H.265".to_owned(),
            ));
        }
    };
    let filter = AVBitStreamFilter::find_by_name(name).ok_or_else(|| {
        MediaError::Playback(format!("FFmpeg 缺少 {} 过滤器", name.to_string_lossy()))
    })?;
    let mut context = AVBSFContextUninit::new(&filter);
    let mut owned_parameters = AVCodecParameters::new();
    owned_parameters.copy(parameters);
    context.set_par_in(&owned_parameters);
    context.set_time_base_in(time_base);
    context
        .init()
        .map_err(|error| MediaError::Playback(format!("初始化 Annex-B 过滤器失败：{error}")))
}

fn encoded_packet(
    packet: &AVPacket,
    track: MediaTrackKind,
    codec: EncodedMediaCodec,
    time_base: MediaTimeBase,
) -> EncodedMediaPacket {
    EncodedMediaPacket {
        track,
        codec,
        data: Bytes::from(gblab_ffmpeg_device::copy_packet_data(packet)),
        pts: valid_timestamp(packet.pts),
        dts: valid_timestamp(packet.dts),
        duration: packet.duration,
        time_base,
        is_keyframe: packet.flags & ffi::AV_PKT_FLAG_KEY.cast_signed() != 0,
        codec_configuration: None,
    }
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
