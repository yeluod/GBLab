//! Camera microphone decode, resample and encode pipeline.

use std::collections::VecDeque;

use bytes::Bytes;
use rsmpeg::{
    avcodec::{AVCodec, AVCodecContext, AVCodecParametersRef, AVPacket},
    avutil::{AVChannelLayout, AVFrame, AVRational},
    error::RsmpegError,
    ffi,
    swresample::SwrContext,
};

use super::{
    AudioCodec, CameraCaptureSettings, EncodedMediaCodec, EncodedMediaPacket, MediaError,
    MediaResult, MediaTimeBase, MediaTrackKind,
};

/// Owns the microphone decoder, resampler and target audio encoder.
pub(super) struct CameraAudioEncoder {
    decoder: AVCodecContext,
    encoder: AVCodecContext,
    resampler: Option<SwrContext>,
    output_layout: AVChannelLayout,
    output_sample_format: ffi::AVSampleFormat,
    sample_rate: i32,
    frame_size: i32,
    next_pts: i64,
    codec: AudioCodec,
    time_base: MediaTimeBase,
    pending: VecDeque<EncodedMediaPacket>,
}

impl CameraAudioEncoder {
    pub(super) fn new(
        settings: &CameraCaptureSettings,
        parameters: &AVCodecParametersRef<'_>,
    ) -> MediaResult<Self> {
        let decoder_codec = AVCodec::find_decoder(parameters.codec_id)
            .ok_or_else(|| MediaError::Camera("未找到麦克风输入解码器".to_owned()))?;
        let mut decoder = AVCodecContext::new(&decoder_codec);
        let mut owned_parameters = rsmpeg::avcodec::AVCodecParameters::new();
        owned_parameters.copy(parameters);
        decoder
            .apply_codecpar(&owned_parameters)
            .map_err(|error| MediaError::Camera(format!("初始化音频解码器失败：{error}")))?;
        decoder
            .open(None)
            .map_err(|error| MediaError::Camera(format!("打开音频解码器失败：{error}")))?;

        let codec_id = match settings.audio_codec {
            AudioCodec::Aac => ffi::AV_CODEC_ID_AAC,
            AudioCodec::G711a => ffi::AV_CODEC_ID_PCM_ALAW,
            AudioCodec::G711u => ffi::AV_CODEC_ID_PCM_MULAW,
            AudioCodec::Other => {
                return Err(MediaError::Camera("摄像头目标音频编码无效".to_owned()));
            }
        };
        let encoder_codec = AVCodec::find_encoder(codec_id).ok_or_else(|| {
            MediaError::Camera(format!(
                "当前 FFmpeg 不支持 {:?} 音频编码",
                settings.audio_codec
            ))
        })?;
        let sample_rate = i32::try_from(settings.audio_sample_rate)
            .map_err(|_| MediaError::Camera("音频采样率超出支持范围".to_owned()))?;
        let channels = i32::try_from(settings.audio_channels)
            .map_err(|_| MediaError::Camera("音频声道数超出支持范围".to_owned()))?;
        if sample_rate <= 0 || channels <= 0 {
            return Err(MediaError::Camera(
                "音频采样率和声道数必须大于零".to_owned(),
            ));
        }
        if encoder_codec
            .supported_samplerates()
            .is_some_and(|rates| !rates.is_empty() && !rates.contains(&sample_rate))
        {
            return Err(MediaError::Camera(format!(
                "{:?} 编码器不支持 {} Hz",
                settings.audio_codec, settings.audio_sample_rate
            )));
        }
        let output_sample_format = encoder_codec
            .sample_fmts()
            .and_then(|formats| formats.first().copied())
            .ok_or_else(|| MediaError::Camera("音频编码器未声明采样格式".to_owned()))?;
        let output_layout = AVChannelLayout::from_nb_channels(channels);
        let mut encoder = AVCodecContext::new(&encoder_codec);
        encoder.set_sample_rate(sample_rate);
        encoder.set_sample_fmt(output_sample_format);
        encoder.set_ch_layout(output_layout.clone().into_inner());
        encoder.set_time_base(AVRational {
            num: 1,
            den: sample_rate,
        });
        encoder.set_bit_rate(
            i64::try_from(settings.audio_bitrate)
                .map_err(|_| MediaError::Camera("音频码率超出编码器范围".to_owned()))?,
        );
        encoder
            .open(None)
            .map_err(|error| MediaError::Camera(format!("打开音频编码器失败：{error}")))?;
        let frame_size = encoder.frame_size;
        let time_base = MediaTimeBase::new(1, sample_rate)
            .ok_or_else(|| MediaError::Camera("音频编码器时间基无效".to_owned()))?;
        Ok(Self {
            decoder,
            encoder,
            resampler: None,
            output_layout,
            output_sample_format,
            sample_rate,
            frame_size,
            next_pts: 0,
            codec: settings.audio_codec,
            time_base,
            pending: VecDeque::new(),
        })
    }

    pub(super) fn encode_packet(&mut self, packet: &AVPacket) -> MediaResult<()> {
        self.decoder
            .send_packet(Some(packet))
            .map_err(|error| MediaError::Camera(format!("提交麦克风 packet 失败：{error}")))?;
        loop {
            match self.decoder.receive_frame() {
                Ok(frame) => self.encode_frame(&frame)?,
                Err(RsmpegError::DecoderDrainError) => break,
                Err(error) => {
                    return Err(MediaError::Camera(format!(
                        "解码麦克风 packet 失败：{error}"
                    )));
                }
            }
        }
        Ok(())
    }

    pub(super) fn take_pending(&mut self) -> Option<EncodedMediaPacket> {
        self.pending.pop_front()
    }

    pub(super) fn finish(&mut self) -> MediaResult<()> {
        match self.decoder.send_packet(None) {
            Ok(()) | Err(RsmpegError::DecoderFlushedError) => {}
            Err(error) => {
                return Err(MediaError::Camera(format!("结束麦克风解码器失败：{error}")));
            }
        }
        loop {
            match self.decoder.receive_frame() {
                Ok(frame) => self.encode_frame(&frame)?,
                Err(RsmpegError::DecoderDrainError | RsmpegError::DecoderFlushedError) => break,
                Err(error) => {
                    return Err(MediaError::Camera(format!("排空麦克风解码器失败：{error}")));
                }
            }
        }
        self.flush_resampler()?;
        match self.encoder.send_frame(None) {
            Ok(()) | Err(RsmpegError::EncoderFlushedError) => self.drain_packets(),
            Err(error) => Err(MediaError::Camera(format!("结束音频编码器失败：{error}"))),
        }
    }

    fn encode_frame(&mut self, input: &AVFrame) -> MediaResult<()> {
        let resampler = if let Some(resampler) = self.resampler.take() {
            resampler
        } else {
            let mut resampler = SwrContext::new(
                &self.output_layout,
                self.output_sample_format,
                self.sample_rate,
                &input.ch_layout(),
                input.format,
                input.sample_rate,
            )
            .map_err(|error| MediaError::Camera(format!("创建音频重采样器失败：{error}")))?;
            resampler
                .init()
                .map_err(|error| MediaError::Camera(format!("初始化音频重采样器失败：{error}")))?;
            resampler
        };
        let available_samples = resampler.get_out_samples(input.nb_samples).max(1);
        let output_samples = if self.frame_size > 0 {
            self.frame_size
        } else {
            available_samples
        };
        let mut output = self.allocate_output_frame(output_samples)?;
        resampler
            .convert_frame(Some(input), &mut output)
            .map_err(|error| MediaError::Camera(format!("音频重采样失败：{error}")))?;
        self.resampler = Some(resampler);
        if output.nb_samples <= 0 {
            return Ok(());
        }
        self.submit_output_frame(&output)
    }

    fn flush_resampler(&mut self) -> MediaResult<()> {
        let Some(resampler) = self.resampler.take() else {
            return Ok(());
        };
        loop {
            let delayed_samples = resampler.get_out_samples(0);
            if delayed_samples <= 0 {
                break;
            }
            let output_samples = if self.frame_size > 0 {
                self.frame_size.min(delayed_samples).max(1)
            } else {
                delayed_samples
            };
            let mut output = self.allocate_output_frame(output_samples)?;
            resampler
                .convert_frame(None, &mut output)
                .map_err(|error| MediaError::Camera(format!("排空音频重采样器失败：{error}")))?;
            if output.nb_samples <= 0 {
                break;
            }
            self.submit_output_frame(&output)?;
        }
        self.resampler = Some(resampler);
        Ok(())
    }

    fn allocate_output_frame(&self, samples: i32) -> MediaResult<AVFrame> {
        let mut output = AVFrame::new();
        output.set_format(self.output_sample_format);
        output.set_sample_rate(self.sample_rate);
        output.set_ch_layout(self.output_layout.clone().into_inner());
        output.set_nb_samples(samples);
        output.set_time_base(AVRational {
            num: 1,
            den: self.sample_rate,
        });
        output.set_pts(self.next_pts);
        output
            .alloc_buffer()
            .map_err(|error| MediaError::Camera(format!("分配音频编码帧失败：{error}")))?;
        Ok(output)
    }

    fn submit_output_frame(&mut self, output: &AVFrame) -> MediaResult<()> {
        self.encoder
            .send_frame(Some(output))
            .map_err(|error| MediaError::Camera(format!("提交音频编码帧失败：{error}")))?;
        self.next_pts = self.next_pts.saturating_add(i64::from(output.nb_samples));
        self.drain_packets()
    }

    fn drain_packets(&mut self) -> MediaResult<()> {
        loop {
            match self.encoder.receive_packet() {
                Ok(packet) => self.pending.push_back(EncodedMediaPacket {
                    track: MediaTrackKind::Audio,
                    codec: EncodedMediaCodec::Audio(self.codec),
                    data: Bytes::from(gblab_ffmpeg_device::copy_packet_data(&packet)),
                    pts: (packet.pts != ffi::AV_NOPTS_VALUE).then_some(packet.pts),
                    dts: (packet.dts != ffi::AV_NOPTS_VALUE).then_some(packet.dts),
                    duration: packet.duration.max(1),
                    time_base: self.time_base,
                    is_keyframe: true,
                    codec_configuration: None,
                }),
                Err(RsmpegError::EncoderDrainError | RsmpegError::EncoderFlushedError) => {
                    return Ok(());
                }
                Err(error) => {
                    return Err(MediaError::Camera(format!(
                        "读取音频编码 packet 失败：{error}"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::CString, path::PathBuf};

    use rsmpeg::avformat::AVFormatContextInput;

    use super::CameraAudioEncoder;
    use crate::{
        configuration::EncoderBackend,
        media::{AudioCodec, CameraCaptureSettings, VideoCodec},
    };

    fn asset() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
            .join("h264-aac.mp4")
    }

    fn settings(codec: AudioCodec) -> CameraCaptureSettings {
        let (sample_rate, channels, bitrate) = match codec {
            AudioCodec::Aac | AudioCodec::Other => (48_000, 2, 128_000),
            AudioCodec::G711a | AudioCodec::G711u => (8_000, 1, 64_000),
        };
        CameraCaptureSettings {
            video_device_id: "test".to_owned(),
            video_codec: VideoCodec::H264,
            video_bitrate: 1_000_000,
            encoder_backend: EncoderBackend::Auto,
            audio_enabled: true,
            audio_device_id: "test".to_owned(),
            audio_codec: codec,
            audio_sample_rate: sample_rate,
            audio_channels: channels,
            audio_bitrate: bitrate,
            width: 128,
            height: 72,
            frames_per_second: 10.0,
        }
    }

    #[test]
    fn microphone_pipeline_should_encode_aac_g711a_and_g711u()
    -> Result<(), Box<dyn std::error::Error>> {
        for codec in [AudioCodec::Aac, AudioCodec::G711a, AudioCodec::G711u] {
            let path = CString::new(asset().to_string_lossy().as_bytes())?;
            let mut context = AVFormatContextInput::open(path.as_c_str())?;
            let audio_stream = context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_audio())
                .ok_or("fixture lacks audio stream")?;
            let audio_stream_index = audio_stream.index;
            let mut encoder = CameraAudioEncoder::new(&settings(codec), &audio_stream.codecpar())?;
            let mut encoded_packet = None;
            while let Some(packet) = context.read_packet()? {
                if packet.stream_index != audio_stream_index {
                    continue;
                }
                encoder.encode_packet(&packet)?;
                if let Some(packet) = encoder.take_pending() {
                    encoded_packet = Some(packet);
                    break;
                }
            }
            encoder.finish()?;
            while let Some(packet) = encoder.take_pending() {
                encoded_packet.get_or_insert(packet);
            }
            let packet = encoded_packet.ok_or("audio encoder produced no packet")?;
            assert_eq!(packet.codec, crate::media::EncodedMediaCodec::Audio(codec));
            assert!(!packet.data.is_empty());
        }
        Ok(())
    }
}
