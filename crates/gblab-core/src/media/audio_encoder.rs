//! Camera microphone decode, resample and encode pipeline.

use std::collections::VecDeque;

use bytes::Bytes;
use rsmpeg::{
    avcodec::{AVCodec, AVCodecContext, AVCodecParameters},
    avutil::{AVAudioFifo, AVChannelLayout, AVFrame, AVRational},
    error::RsmpegError,
    ffi,
    swresample::SwrContext,
};

use super::{
    AudioCodec, CameraCaptureSettings, EncodedMediaCodec, EncodedMediaPacket, EncodedOutputInfo,
    MediaError, MediaResult, MediaTimeBase, MediaTrackKind,
};

/// Resamples the single decoded microphone PCM branch into the target encoder.
pub(super) struct CameraAudioEncoder {
    encoder: AVCodecContext,
    resampler: Option<SwrContext>,
    output_layout: AVChannelLayout,
    output_sample_format: ffi::AVSampleFormat,
    sample_rate: i32,
    frame_size: i32,
    fifo: AVAudioFifo,
    next_pts: i64,
    codec: AudioCodec,
    time_base: MediaTimeBase,
    pending: VecDeque<EncodedMediaPacket>,
    codec_configuration: Option<Bytes>,
    config_sent: bool,
}

impl CameraAudioEncoder {
    pub(super) fn new(settings: &CameraCaptureSettings) -> MediaResult<Self> {
        if matches!(settings.audio_codec, AudioCodec::G711a | AudioCodec::G711u)
            && (settings.audio_sample_rate != 8_000
                || settings.audio_channels != 1
                || settings.audio_bitrate != 64_000)
        {
            return Err(MediaError::Camera(
                "G.711 音频必须使用 8000 Hz、单声道、64000 bit/s".to_owned(),
            ));
        }
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
        let fifo = AVAudioFifo::new(output_sample_format, channels, frame_size.max(1));
        let time_base = MediaTimeBase::new(1, sample_rate)
            .ok_or_else(|| MediaError::Camera("音频编码器时间基无效".to_owned()))?;
        let mut codec_parameters = AVCodecParameters::new();
        codec_parameters.from_context(&encoder);
        let codec_configuration =
            gblab_ffmpeg_device::copy_owned_codec_extradata(&codec_parameters).map(Bytes::from);
        Ok(Self {
            encoder,
            resampler: None,
            output_layout,
            output_sample_format,
            sample_rate,
            frame_size,
            fifo,
            next_pts: 0,
            codec: settings.audio_codec,
            time_base,
            pending: VecDeque::new(),
            codec_configuration,
            config_sent: false,
        })
    }

    pub(super) fn encode_pcm(
        &mut self,
        pcm: &super::audio_preview::AudioPcmFrame,
    ) -> MediaResult<()> {
        let sample_rate = i32::try_from(pcm.sample_rate)
            .map_err(|_| MediaError::Camera("麦克风 PCM 采样率超出范围".to_owned()))?;
        let channels = i32::from(pcm.channels);
        let sample_count = pcm.samples.len() / usize::from(pcm.channels.max(1));
        let mut frame = AVFrame::new();
        frame.set_format(ffi::AV_SAMPLE_FMT_FLT);
        frame.set_sample_rate(sample_rate);
        frame.set_ch_layout(AVChannelLayout::from_nb_channels(channels).into_inner());
        frame.set_nb_samples(
            i32::try_from(sample_count)
                .map_err(|_| MediaError::Camera("麦克风 PCM 样本数超出范围".to_owned()))?,
        );
        frame.set_pts(pcm.pts.unwrap_or(ffi::AV_NOPTS_VALUE));
        frame
            .alloc_buffer()
            .map_err(|error| MediaError::Camera(format!("分配麦克风 PCM 帧失败：{error}")))?;
        if !gblab_ffmpeg_device::write_interleaved_f32(&mut frame, &pcm.samples) {
            return Err(MediaError::Camera("写入麦克风 PCM 帧失败".to_owned()));
        }
        self.encode_frame(&frame, pcm.time_base)
    }

    pub(super) fn take_pending(&mut self) -> Option<EncodedMediaPacket> {
        self.pending.pop_front()
    }

    pub(super) fn finish(&mut self) -> MediaResult<()> {
        self.flush_resampler()?;
        self.submit_fifo_frames(true)?;
        match self.encoder.send_frame(None) {
            Ok(()) | Err(RsmpegError::EncoderFlushedError) => self.drain_packets(),
            Err(error) => Err(MediaError::Camera(format!("结束音频编码器失败：{error}"))),
        }
    }

    fn encode_frame(
        &mut self,
        input: &AVFrame,
        source_time_base: MediaTimeBase,
    ) -> MediaResult<()> {
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
        let mut output = self.allocate_output_frame(available_samples)?;
        resampler
            .convert_frame(Some(input), &mut output)
            .map_err(|error| MediaError::Camera(format!("音频重采样失败：{error}")))?;
        self.resampler = Some(resampler);
        if output.nb_samples <= 0 {
            return Ok(());
        }
        if input.pts != ffi::AV_NOPTS_VALUE && self.fifo.size() == 0 {
            self.next_pts = source_time_base.rescale(input.pts, self.time_base);
        }
        gblab_ffmpeg_device::audio_fifo_write(&mut self.fifo, &output)
            .map_err(|error| MediaError::Camera(format!("写入音频 FIFO 失败：{error}")))?;
        self.submit_fifo_frames(false)
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
            let mut output = self.allocate_output_frame(delayed_samples)?;
            resampler
                .convert_frame(None, &mut output)
                .map_err(|error| MediaError::Camera(format!("排空音频重采样器失败：{error}")))?;
            if output.nb_samples <= 0 {
                break;
            }
            gblab_ffmpeg_device::audio_fifo_write(&mut self.fifo, &output)
                .map_err(|error| MediaError::Camera(format!("写入排空音频 FIFO 失败：{error}")))?;
        }
        self.resampler = Some(resampler);
        Ok(())
    }

    fn submit_fifo_frames(&mut self, flushing: bool) -> MediaResult<()> {
        let preferred = self.frame_size.max(1);
        while self.fifo.size() >= preferred || (flushing && self.fifo.size() > 0) {
            let samples = if self.fifo.size() >= preferred {
                preferred
            } else {
                self.fifo.size()
            };
            let mut output = self.allocate_output_frame(samples)?;
            gblab_ffmpeg_device::audio_fifo_read(&mut self.fifo, &mut output)
                .map_err(|error| MediaError::Camera(format!("读取音频 FIFO 失败：{error}")))?;
            self.submit_output_frame(&output)?;
        }
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
        self.next_pts = output.pts.saturating_add(i64::from(output.nb_samples));
        self.drain_packets()
    }

    fn drain_packets(&mut self) -> MediaResult<()> {
        loop {
            match self.encoder.receive_packet() {
                Ok(packet) => {
                    let codec_configuration = if self.config_sent {
                        None
                    } else {
                        self.config_sent = true;
                        self.codec_configuration.clone()
                    };
                    self.pending.push_back(EncodedMediaPacket {
                        track: MediaTrackKind::Audio,
                        codec: EncodedMediaCodec::Audio(self.codec),
                        data: Bytes::from(gblab_ffmpeg_device::copy_packet_data(&packet)),
                        pts: (packet.pts != ffi::AV_NOPTS_VALUE).then_some(packet.pts),
                        dts: (packet.dts != ffi::AV_NOPTS_VALUE).then_some(packet.dts),
                        duration: packet.duration.max(1),
                        time_base: self.time_base,
                        is_keyframe: true,
                        codec_configuration,
                        output_info: Some(EncodedOutputInfo {
                            width: None,
                            height: None,
                            frame_rate: None,
                            sample_rate: u32::try_from(self.sample_rate).ok(),
                            channels: u32::try_from(self.output_layout.nb_channels).ok(),
                            bitrate: u64::try_from(self.encoder.bit_rate).ok(),
                        }),
                    });
                }
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
        media::{
            AudioCodec, CameraCaptureSettings, MediaTimeBase, VideoCodec,
            audio_preview::{AudioOutputFormat, AudioPcmFrame, AudioPreviewDecoder},
        },
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
            let time_base =
                MediaTimeBase::new(audio_stream.time_base.num, audio_stream.time_base.den)
                    .ok_or("invalid audio time base")?;
            let mut decoder = AudioPreviewDecoder::new(
                &audio_stream.codecpar(),
                time_base,
                AudioOutputFormat {
                    sample_rate: 48_000,
                    channels: 2,
                },
            )?;
            let mut encoder = CameraAudioEncoder::new(&settings(codec))?;
            let mut encoded_packet = None;
            while let Some(packet) = context.read_packet()? {
                if packet.stream_index != audio_stream_index {
                    continue;
                }
                for frame in decoder.decode(&packet)? {
                    encoder.encode_pcm(&frame)?;
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
            let packet = encoded_packet.ok_or("audio encoder produced no packet")?;
            assert_eq!(packet.codec, crate::media::EncodedMediaCodec::Audio(codec));
            assert!(!packet.data.is_empty());
        }
        Ok(())
    }

    #[test]
    fn missing_audio_pts_should_continue_after_the_latest_valid_jump()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = CameraAudioEncoder::new(&settings(AudioCodec::Aac))?;
        let frame = |pts| AudioPcmFrame {
            samples: vec![0.0; 2_048],
            sample_rate: 48_000,
            channels: 2,
            pts,
            time_base: MediaTimeBase::new(1, 48_000).unwrap_or(MediaTimeBase::MPEG_CLOCK),
        };
        encoder.encode_pcm(&frame(Some(0)))?;
        encoder.encode_pcm(&frame(Some(48_000)))?;
        let after_jump = encoder.next_pts;
        encoder.encode_pcm(&frame(None))?;

        assert!(after_jump > 48_000);
        assert!(encoder.next_pts > after_jump);
        encoder.finish()?;
        Ok(())
    }
}
