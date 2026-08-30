//! Camera raw-video to H.264/H.265 encoding.

use std::collections::VecDeque;

use bytes::Bytes;
use rsmpeg::{
    avcodec::{AVCodec, AVCodecContext},
    avutil::{AVFrame, AVRational},
    error::RsmpegError,
    ffi,
    swscale::SwsContext,
};

use super::{
    CameraCaptureSettings, EncodedMediaCodec, EncodedMediaPacket, EncodedOutputInfo, FrameRate,
    MediaError, MediaResult, MediaTimeBase, MediaTrackKind,
};

/// `FFmpeg` camera video encoder owned by the source worker.
pub(super) struct CameraVideoEncoder {
    context: AVCodecContext,
    scaler: Option<SwsContext>,
    width: i32,
    height: i32,
    next_pts: i64,
    time_base: MediaTimeBase,
    codec: super::VideoCodec,
    pending: VecDeque<EncodedMediaPacket>,
    codec_configuration: Option<Bytes>,
    config_sent: bool,
}

impl CameraVideoEncoder {
    pub(super) fn new(settings: &CameraCaptureSettings) -> MediaResult<Self> {
        let candidates = super::camera::video_encoder_candidates(settings);
        if candidates.is_empty() {
            return Err(MediaError::Camera(format!(
                "没有可用于 {:?}/{:?} 的视频编码器",
                settings.video_codec, settings.encoder_backend
            )));
        }
        let mut last_error = None;
        for capability in candidates {
            match Self::try_new(settings, capability) {
                Ok(encoder) => return Ok(encoder),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error
            .unwrap_or_else(|| MediaError::Camera("所有候选视频编码器均无法打开".to_owned())))
    }

    fn try_new(
        settings: &CameraCaptureSettings,
        capability: super::VideoEncoderCapability,
    ) -> MediaResult<Self> {
        let encoder_name = std::ffi::CString::new(capability.encoder_name)
            .map_err(|_| MediaError::Camera("编码器名称无效".to_owned()))?;
        let encoder = AVCodec::find_encoder_by_name(encoder_name.as_c_str()).ok_or_else(|| {
            MediaError::Camera(format!(
                "FFmpeg 编码器 {} 不可用",
                encoder_name.to_string_lossy()
            ))
        })?;
        let width = i32::try_from(settings.width)
            .map_err(|_| MediaError::Camera("视频宽度超出编码器范围".to_owned()))?;
        let height = i32::try_from(settings.height)
            .map_err(|_| MediaError::Camera("视频高度超出编码器范围".to_owned()))?;
        let frame_rate = FrameRate::from_f64(settings.frames_per_second)
            .ok_or_else(|| MediaError::Camera("视频帧率无效".to_owned()))?;
        let fps_num = i32::try_from(frame_rate.numerator)
            .map_err(|_| MediaError::Camera("视频帧率分子超出编码器范围".to_owned()))?;
        let fps_den = i32::try_from(frame_rate.denominator)
            .map_err(|_| MediaError::Camera("视频帧率分母超出编码器范围".to_owned()))?;
        let mut context = AVCodecContext::new(&encoder);
        context.set_width(width);
        context.set_height(height);
        context.set_pix_fmt(ffi::AV_PIX_FMT_YUV420P);
        context.set_time_base(AVRational {
            num: fps_den,
            den: fps_num,
        });
        context.set_framerate(AVRational {
            num: fps_num,
            den: fps_den,
        });
        context.set_bit_rate(
            i64::try_from(settings.video_bitrate)
                .map_err(|_| MediaError::Camera("视频码率超出编码器支持范围".to_owned()))?,
        );
        let gop_size = (frame_rate.as_f64() * 2.0).round().max(1.0);
        if !gop_size.is_finite() || gop_size > f64::from(i32::MAX) {
            return Err(MediaError::Camera("视频 GOP 大小超出编码器范围".to_owned()));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "GOP size was range-checked against i32"
        )]
        let gop_size = gop_size as i32;
        context.set_gop_size(gop_size);
        context.set_max_b_frames(0);
        context
            .open(None)
            .map_err(|error| MediaError::Camera(format!("打开视频编码器失败：{error}")))?;
        let time_base = MediaTimeBase::new(fps_den, fps_num)
            .ok_or_else(|| MediaError::Camera("编码器时间基无效".to_owned()))?;
        let mut parameters = rsmpeg::avcodec::AVCodecParameters::new();
        parameters.from_context(&context);
        let codec_configuration =
            gblab_ffmpeg_device::copy_owned_codec_extradata(&parameters).map(Bytes::from);
        Ok(Self {
            context,
            scaler: None,
            width,
            height,
            next_pts: 0,
            time_base,
            codec: settings.video_codec,
            pending: VecDeque::new(),
            codec_configuration,
            config_sent: false,
        })
    }

    pub(super) fn take_pending(&mut self) -> Option<EncodedMediaPacket> {
        self.pending.pop_front()
    }

    pub(super) fn encode(
        &mut self,
        input: &AVFrame,
        source_time_base: MediaTimeBase,
    ) -> MediaResult<()> {
        let scaler = self
            .scaler
            .take()
            .or_else(|| {
                SwsContext::get_context(
                    input.width,
                    input.height,
                    ffi::AVPixelFormat::from(input.format),
                    self.width,
                    self.height,
                    ffi::AV_PIX_FMT_YUV420P,
                    2,
                    None,
                    None,
                    None,
                )
            })
            .ok_or_else(|| MediaError::Camera("创建摄像头像素转换器失败".to_owned()))?;
        let mut scaler = scaler;
        let mut frame = AVFrame::new();
        frame.set_format(ffi::AV_PIX_FMT_YUV420P);
        frame.set_width(self.width);
        frame.set_height(self.height);
        let frame_pts = if input.pts == ffi::AV_NOPTS_VALUE {
            self.next_pts
        } else {
            source_time_base.rescale(input.pts, self.time_base)
        };
        frame.set_pts(frame_pts);
        frame.set_time_base(AVRational {
            num: self.time_base.numerator,
            den: self.time_base.denominator,
        });
        frame
            .alloc_buffer()
            .map_err(|error| MediaError::Camera(format!("分配编码帧失败：{error}")))?;
        frame
            .make_writable()
            .map_err(|error| MediaError::Camera(format!("准备编码帧失败：{error}")))?;
        scaler
            .scale_frame(input, 0, input.height, &mut frame)
            .map_err(|error| MediaError::Camera(format!("转换编码帧失败：{error}")))?;
        self.scaler = Some(scaler);
        self.context
            .send_frame(Some(&frame))
            .map_err(|error| MediaError::Camera(format!("提交编码帧失败：{error}")))?;
        self.next_pts = frame_pts.saturating_add(1);
        self.drain_packets()
    }

    pub(super) fn finish(&mut self) -> MediaResult<()> {
        match self.context.send_frame(None) {
            Ok(()) | Err(RsmpegError::EncoderFlushedError) => self.drain_packets(),
            Err(error) => Err(MediaError::Camera(format!("结束视频编码器失败：{error}"))),
        }
    }

    fn drain_packets(&mut self) -> MediaResult<()> {
        loop {
            match self.context.receive_packet() {
                Ok(packet) => {
                    let codec_configuration = if self.config_sent {
                        None
                    } else {
                        self.config_sent = true;
                        self.codec_configuration.clone()
                    };
                    self.pending.push_back(EncodedMediaPacket {
                        track: MediaTrackKind::Video,
                        codec: EncodedMediaCodec::Video(self.codec),
                        data: Bytes::from(gblab_ffmpeg_device::copy_packet_data(&packet)),
                        pts: (packet.pts != ffi::AV_NOPTS_VALUE).then_some(packet.pts),
                        dts: (packet.dts != ffi::AV_NOPTS_VALUE).then_some(packet.dts),
                        duration: packet.duration.max(1),
                        time_base: self.time_base,
                        is_keyframe: packet.flags & ffi::AV_PKT_FLAG_KEY.cast_signed() != 0,
                        codec_configuration,
                        output_info: Some(EncodedOutputInfo {
                            width: Some(u32::try_from(self.width).unwrap_or(0)),
                            height: Some(u32::try_from(self.height).unwrap_or(0)),
                            frame_rate: FrameRate::new(
                                u32::try_from(self.context.framerate.num).unwrap_or(0),
                                u32::try_from(self.context.framerate.den).unwrap_or(0),
                            ),
                            sample_rate: None,
                            channels: None,
                            bitrate: u64::try_from(self.context.bit_rate).ok(),
                        }),
                    });
                }
                Err(RsmpegError::EncoderDrainError | RsmpegError::EncoderFlushedError) => break,
                Err(error) => {
                    return Err(MediaError::Camera(format!("读取编码 packet 失败：{error}")));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::CString, path::PathBuf};

    use rsmpeg::avformat::AVFormatContextInput;

    use super::CameraVideoEncoder;
    use crate::{
        configuration::EncoderBackend,
        media::{
            AudioCodec, CameraCaptureSettings, EncodedMediaCodec, VideoCodec,
            camera::video_encoder_capabilities, decoder::VideoDecoder,
        },
    };

    fn asset() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
            .join("h264-noaudio.mp4")
    }

    fn settings(codec: VideoCodec) -> CameraCaptureSettings {
        CameraCaptureSettings {
            video_device_id: "test".to_owned(),
            video_codec: codec,
            video_bitrate: 1_000_000,
            encoder_backend: EncoderBackend::Auto,
            audio_enabled: false,
            audio_device_id: String::new(),
            audio_codec: AudioCodec::Aac,
            audio_sample_rate: 48_000,
            audio_channels: 2,
            audio_bitrate: 128_000,
            width: 128,
            height: 72,
            frames_per_second: 10.0,
        }
    }

    #[test]
    fn available_camera_encoders_should_emit_annex_b_h264_or_h265()
    -> Result<(), Box<dyn std::error::Error>> {
        let capabilities = video_encoder_capabilities();
        for codec in [VideoCodec::H264, VideoCodec::H265] {
            if !capabilities
                .encoders
                .iter()
                .any(|capability| capability.codec == codec)
            {
                continue;
            }

            let path = CString::new(asset().to_string_lossy().as_bytes())?;
            let mut context = AVFormatContextInput::open(path.as_c_str())?;
            let video_stream = context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_video())
                .ok_or("fixture lacks video stream")?;
            let video_stream_index = video_stream.index;
            let mut decoder = VideoDecoder::new(&video_stream.codecpar(), video_stream)?;
            let mut encoder = CameraVideoEncoder::new(&settings(codec))?;
            let mut encoded_packet = None;

            while let Some(packet) = context.read_packet()? {
                if packet.stream_index != video_stream_index {
                    continue;
                }
                for frame in decoder.decode_raw_frames(&packet)? {
                    encoder.encode(&frame, crate::media::MediaTimeBase::MPEG_CLOCK)?;
                }
                if let Some(packet) = encoder.take_pending() {
                    encoded_packet = Some(packet);
                    break;
                }
            }
            encoder.finish()?;
            while let Some(packet) = encoder.take_pending() {
                encoded_packet.get_or_insert(packet);
            }

            let packet = encoded_packet.ok_or("video encoder produced no packet")?;
            assert_eq!(packet.codec, EncodedMediaCodec::Video(codec));
            assert!(!packet.data.is_empty());
            assert!(
                packet.data.starts_with(&[0, 0, 0, 1]) || packet.data.starts_with(&[0, 0, 1]),
                "camera encoder did not produce Annex-B for {codec:?}"
            );
        }
        Ok(())
    }
}
