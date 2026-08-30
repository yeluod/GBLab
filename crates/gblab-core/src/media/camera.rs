use std::{
    collections::VecDeque,
    ffi::CString,
    sync::{Arc, atomic::AtomicU8},
};

use rsmpeg::{
    avcodec::AVCodecParameters,
    avformat::{AVFormatContextInput, AVInputFormat, AVInputFormatRef},
    avutil::AVDictionary,
    error::RsmpegError,
    ffi,
};

use super::{
    AudioStreamInfo, CameraCaptureSettings, CaptureDeviceInfo, CaptureDeviceLists, FrameRate,
    FrameRateCapability, MediaError, MediaResult, MediaRuntimeMetrics, MediaTimeBase,
    Mp4ProbeResult, VideoCaptureCapabilities, VideoCaptureMode, VideoEncoderCapabilities,
    VideoEncoderCapability, VideoStreamInfo,
    audio_encoder::CameraAudioEncoder,
    audio_preview::{AudioOutputFormat, AudioPreviewDecoder, audio_levels},
    decoder::VideoDecoder,
    types::{MediaSource, MediaSourceSession, SourceReadOutput},
    video_encoder::CameraVideoEncoder,
};
use crate::configuration::EncoderBackend;

/// 使用当前平台的原生 API 枚举 `FFmpeg` 可采集的输入设备。
pub fn list_capture_devices() -> MediaResult<CaptureDeviceLists> {
    let devices = gblab_ffmpeg_device::list_capture_devices()
        .map_err(|error| MediaError::Camera(format!("无法枚举本机采集设备：{error}")))?;
    Ok(CaptureDeviceLists {
        video: devices.video.into_iter().map(capture_device_info).collect(),
        audio: devices.audio.into_iter().map(capture_device_info).collect(),
    })
}

/// 不打开设备，读取指定摄像头的原生分辨率和帧率能力。
pub fn video_capture_capabilities(device_id: &str) -> MediaResult<VideoCaptureCapabilities> {
    let modes = gblab_ffmpeg_device::video_capture_modes(device_id)
        .map_err(|error| MediaError::Camera(format!("无法读取摄像头采集能力：{error}")))?;
    Ok(VideoCaptureCapabilities {
        device_id: device_id.to_owned(),
        modes: modes
            .into_iter()
            .map(|mode| VideoCaptureMode {
                width: mode.width,
                height: mode.height,
                frame_rates: mode
                    .frame_rates
                    .into_iter()
                    .map(|rate| match rate {
                        gblab_ffmpeg_device::NativeFrameRateCapability::Exact(value) => {
                            FrameRateCapability::Exact { value }
                        }
                        gblab_ffmpeg_device::NativeFrameRateCapability::Range {
                            minimum,
                            maximum,
                        } => FrameRateCapability::Range { minimum, maximum },
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// 检查随应用链接的 `FFmpeg` 是否真实提供 H.264/H.265 编码器。
#[must_use]
pub fn video_encoder_capabilities() -> VideoEncoderCapabilities {
    let encoders = gblab_ffmpeg_device::supported_video_encoders()
        .into_iter()
        .map(|capability| VideoEncoderCapability {
            codec: match capability.codec {
                gblab_ffmpeg_device::NativeVideoCodec::H264 => super::VideoCodec::H264,
                gblab_ffmpeg_device::NativeVideoCodec::H265 => super::VideoCodec::H265,
            },
            backend: match capability.backend {
                gblab_ffmpeg_device::NativeEncoderBackend::VideoToolbox => {
                    EncoderBackend::Videotoolbox
                }
                gblab_ffmpeg_device::NativeEncoderBackend::MediaFoundation => {
                    EncoderBackend::MediaFoundation
                }
                gblab_ffmpeg_device::NativeEncoderBackend::Nvenc => EncoderBackend::Nvenc,
                gblab_ffmpeg_device::NativeEncoderBackend::Qsv => EncoderBackend::Qsv,
                gblab_ffmpeg_device::NativeEncoderBackend::Amf => EncoderBackend::Amf,
                gblab_ffmpeg_device::NativeEncoderBackend::Software => EncoderBackend::Auto,
            },
            encoder_name: capability.encoder_name,
            hardware: capability.hardware,
        })
        .collect();
    VideoEncoderCapabilities { encoders }
}

pub(super) fn video_encoder_candidates(
    settings: &CameraCaptureSettings,
) -> Vec<VideoEncoderCapability> {
    video_encoder_capabilities()
        .encoders
        .into_iter()
        .filter(|capability| {
            capability.codec == settings.video_codec
                && (settings.encoder_backend == EncoderBackend::Auto
                    || capability.backend == settings.encoder_backend)
        })
        .collect()
}

fn capture_device_info(device: gblab_ffmpeg_device::NativeCaptureDevice) -> CaptureDeviceInfo {
    CaptureDeviceInfo {
        id: device.id,
        name: device.name,
    }
}

/// `FFmpeg` 摄像头输入源。
pub struct CameraMediaSource {
    settings: CameraCaptureSettings,
    interrupt: Arc<AtomicU8>,
}

impl CameraMediaSource {
    /// 创建摄像头源描述。
    #[must_use]
    pub const fn new(settings: CameraCaptureSettings, interrupt: Arc<AtomicU8>) -> Self {
        Self {
            settings,
            interrupt,
        }
    }

    fn input_description(
        &self,
        include_audio: bool,
    ) -> MediaResult<(CString, AVInputFormatRef<'static>)> {
        self.validate_settings()?;
        if self.settings.video_device_id.trim().is_empty() {
            return Err(MediaError::Camera("未设置摄像头设备标识".to_owned()));
        }
        let url = platform_device_url(&self.settings, include_audio)?;
        let format = platform_input_format()?;
        Ok((url, format))
    }

    fn validate_settings(&self) -> MediaResult<()> {
        if self.settings.width == 0 || self.settings.height == 0 {
            return Err(MediaError::Camera("摄像头分辨率必须大于零".to_owned()));
        }
        if !self.settings.frames_per_second.is_finite()
            || self.settings.frames_per_second <= 0.0
            || self.settings.frames_per_second > 240.0
        {
            return Err(MediaError::Camera(
                "摄像头帧率必须介于 0 和 240 FPS 之间".to_owned(),
            ));
        }
        if self.settings.video_bitrate == 0 {
            return Err(MediaError::Camera("视频码率必须大于零".to_owned()));
        }
        if self.settings.audio_enabled {
            if self.settings.audio_device_id.trim().is_empty() {
                return Err(MediaError::Camera(
                    "启用音频后必须设置麦克风设备标识".to_owned(),
                ));
            }
            if self.settings.audio_codec == super::AudioCodec::Other {
                return Err(MediaError::Camera(
                    "摄像头目标音频编码必须选择 AAC 或 G.711".to_owned(),
                ));
            }
            if self.settings.audio_sample_rate == 0
                || self.settings.audio_channels == 0
                || self.settings.audio_bitrate == 0
            {
                return Err(MediaError::Camera(
                    "音频采样率、声道和码率必须大于零".to_owned(),
                ));
            }
            if matches!(
                self.settings.audio_codec,
                super::AudioCodec::G711a | super::AudioCodec::G711u
            ) && (self.settings.audio_sample_rate != 8_000
                || self.settings.audio_channels != 1
                || self.settings.audio_bitrate != 64_000)
            {
                return Err(MediaError::Camera(
                    "G.711 音频必须使用 8000 Hz、单声道、64000 bit/s".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn capture_options(&self) -> MediaResult<Option<AVDictionary>> {
        let mut options = if self.settings.frames_per_second > 0.0 {
            let frame_rate = FrameRate::from_f64(self.settings.frames_per_second)
                .ok_or_else(|| MediaError::Camera("帧率配置无效".to_owned()))?;
            Some(AVDictionary::new(
                c"framerate",
                CString::new(format!(
                    "{}/{}",
                    frame_rate.numerator, frame_rate.denominator
                ))
                .map_err(|_| MediaError::Camera("帧率配置无效".to_owned()))?
                .as_c_str(),
                0,
            ))
        } else {
            None
        };
        if self.settings.width > 0 && self.settings.height > 0 {
            let size = format!("{}x{}", self.settings.width, self.settings.height);
            let size =
                CString::new(size).map_err(|_| MediaError::Camera("分辨率配置无效".to_owned()))?;
            options = Some(options.take().map_or_else(
                || AVDictionary::new(c"video_size", size.as_c_str(), 0),
                |existing_options| existing_options.set(c"video_size", size.as_c_str(), 0),
            ));
        }
        Ok(options)
    }

    fn probe_context(&self, context: &AVFormatContextInput) -> MediaResult<Mp4ProbeResult> {
        let video_stream = context
            .streams()
            .iter()
            .find(|stream| stream.codecpar().codec_type().is_video())
            .ok_or(MediaError::MissingVideoStream)?;
        let video_parameters = video_stream.codecpar();
        let detected_frames_per_second = video_stream.guess_framerate().map(|rate| {
            if rate.den == 0 {
                0.0
            } else {
                f64::from(rate.num) / f64::from(rate.den)
            }
        });
        let fps = if self.settings.frames_per_second > 0.0 {
            self.settings.frames_per_second
        } else {
            detected_frames_per_second.unwrap_or(0.0)
        };
        let video = VideoStreamInfo {
            codec: self.settings.video_codec,
            width: u32::try_from(video_parameters.width)
                .map_err(|_| MediaError::Camera("摄像头宽度超出支持范围".to_owned()))?,
            height: u32::try_from(video_parameters.height)
                .map_err(|_| MediaError::Camera("摄像头高度超出支持范围".to_owned()))?,
            frames_per_second: fps,
            bitrate: None,
            duration_seconds: None,
        };
        let audio = if self.settings.audio_enabled {
            context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_audio())
                .map(|stream| {
                    let parameters = stream.codecpar();
                    Ok(AudioStreamInfo {
                        codec: self.settings.audio_codec,
                        sample_rate: u32::try_from(parameters.sample_rate)
                            .map_err(|_| MediaError::Camera("采样率超出支持范围".to_owned()))?,
                        channels: u32::try_from(parameters.ch_layout.nb_channels)
                            .map_err(|_| MediaError::Camera("声道数超出支持范围".to_owned()))?,
                        bitrate: None,
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(Mp4ProbeResult {
            file_path: self.settings.video_device_id.clone(),
            video,
            audio,
            duration_seconds: None,
            bitrate: None,
        })
    }

    fn open_context(
        &self,
        include_audio: bool,
    ) -> MediaResult<(
        AVFormatContextInput,
        gblab_ffmpeg_device::InputInterruptGuard,
    )> {
        let (url, format) = self.input_description(include_audio)?;
        let options = self.capture_options()?;
        gblab_ffmpeg_device::open_input_with_interrupt(
            url.as_c_str(),
            &format,
            options,
            Arc::clone(&self.interrupt),
        )
        .map_err(|error| MediaError::Camera(error.to_string()))
    }
}

impl MediaSource for CameraMediaSource {
    fn probe(&self) -> MediaResult<Mp4ProbeResult> {
        let (context, _interrupt_guard) = self.open_context(self.settings.audio_enabled)?;
        self.probe_context(&context)
    }

    fn open(&self, _looping: bool) -> MediaResult<MediaSourceSession> {
        let (context, interrupt_guard, fallback_error) = match self
            .open_context(self.settings.audio_enabled)
        {
            Ok((context, interrupt_guard)) => (context, interrupt_guard, None),
            Err(error) if self.settings.audio_enabled => {
                let (context, interrupt_guard) = self.open_context(false).map_err(|fallback| {
                    MediaError::Camera(format!(
                        "打开摄像头与麦克风失败：{error}；纯视频回退也失败：{fallback}"
                    ))
                })?;
                (
                    context,
                    interrupt_guard,
                    Some(format!("AudioCapture: {error}")),
                )
            }
            Err(error) => return Err(error),
        };
        let probe = self.probe_context(&context)?;
        let video_stream = context
            .streams()
            .iter()
            .find(|stream| stream.codecpar().codec_type().is_video())
            .ok_or(MediaError::MissingVideoStream)?;
        let video_stream_index = usize::try_from(video_stream.index)
            .map_err(|_| MediaError::Camera("FFmpeg 返回了无效的视频流索引".to_owned()))?;
        let video_time_base =
            MediaTimeBase::new(video_stream.time_base.num, video_stream.time_base.den)
                .ok_or_else(|| MediaError::Camera("摄像头视频时间基无效".to_owned()))?;
        let decoder = VideoDecoder::new(&video_stream.codecpar(), video_stream)?;
        let (
            audio_stream_index,
            audio_parameters,
            audio_preview_decoder,
            audio_time_base,
            audio_error,
        ) = if self.settings.audio_enabled {
            let setup = context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_audio())
                .ok_or_else(|| MediaError::Camera("已启用麦克风，但采集输入没有音频流".to_owned()))
                .and_then(|audio_stream| {
                    let index = usize::try_from(audio_stream.index).map_err(|_| {
                        MediaError::Camera("FFmpeg 返回了无效的音频流索引".to_owned())
                    })?;
                    let audio_time_base =
                        MediaTimeBase::new(audio_stream.time_base.num, audio_stream.time_base.den)
                            .ok_or_else(|| MediaError::Camera("摄像头音频时间基无效".to_owned()))?;
                    let mut parameters = AVCodecParameters::new();
                    parameters.copy(&audio_stream.codecpar());
                    let preview_decoder = AudioPreviewDecoder::new(
                        &audio_stream.codecpar(),
                        audio_time_base,
                        AudioOutputFormat {
                            sample_rate: 48_000,
                            channels: 2,
                        },
                    )?;
                    Ok((index, parameters, preview_decoder, audio_time_base))
                });
            match setup {
                Ok((index, parameters, decoder, time_base)) => (
                    Some(index),
                    Some(parameters),
                    Some(decoder),
                    Some(time_base),
                    fallback_error,
                ),
                Err(error) => (
                    None,
                    None,
                    None,
                    None,
                    Some(fallback_error.unwrap_or_else(|| format!("AudioCapture: {error}"))),
                ),
            }
        } else {
            (None, None, None, None, None)
        };
        Ok(MediaSourceSession::Camera(Box::new(CameraSession {
            _interrupt_guard: interrupt_guard,
            context,
            probe,
            playing: false,
            decoder,
            settings: self.settings.clone(),
            encoder: None,
            pending_encoded: VecDeque::new(),
            video_stream_index,
            video_time_base,
            audio_stream_index,
            audio_parameters,
            audio_preview_decoder,
            audio_encoder: None,
            audio_time_base,
            audio_error,
            preview_enabled: true,
            encoded_enabled: false,
            encoded_error: None,
        })))
    }
}

/// 已打开的摄像头采集会话。
pub struct CameraSession {
    // Drop the callback guard before the input context that it references.
    _interrupt_guard: gblab_ffmpeg_device::InputInterruptGuard,
    context: AVFormatContextInput,
    probe: Mp4ProbeResult,
    playing: bool,
    decoder: VideoDecoder,
    settings: CameraCaptureSettings,
    encoder: Option<CameraVideoEncoder>,
    pending_encoded: VecDeque<super::EncodedMediaPacket>,
    video_stream_index: usize,
    video_time_base: MediaTimeBase,
    audio_stream_index: Option<usize>,
    audio_parameters: Option<AVCodecParameters>,
    audio_preview_decoder: Option<AudioPreviewDecoder>,
    audio_encoder: Option<CameraAudioEncoder>,
    audio_time_base: Option<MediaTimeBase>,
    audio_error: Option<String>,
    preview_enabled: bool,
    encoded_enabled: bool,
    encoded_error: Option<String>,
}

impl CameraSession {
    pub(crate) const fn probe(&self) -> &Mp4ProbeResult {
        &self.probe
    }

    pub(crate) fn initial_pipeline_error(&self) -> Option<String> {
        self.audio_error.clone()
    }

    pub(crate) const fn audio_preview_available(&self) -> bool {
        self.audio_preview_decoder.is_some()
    }

    pub(crate) const fn set_preview_enabled(&mut self, enabled: bool) {
        self.preview_enabled = enabled;
    }

    pub(crate) fn set_audio_output_format(
        &mut self,
        output_format: AudioOutputFormat,
    ) -> MediaResult<()> {
        self.audio_preview_decoder = match (&self.audio_parameters, self.audio_time_base) {
            (Some(parameters), Some(time_base)) => Some(AudioPreviewDecoder::new(
                parameters,
                time_base,
                output_format,
            )?),
            _ => None,
        };
        Ok(())
    }

    pub(crate) fn set_encoded_enabled(&mut self, enabled: bool) -> MediaResult<()> {
        if !enabled {
            self.encoded_enabled = false;
            self.encoder = None;
            self.audio_encoder = None;
            self.pending_encoded.clear();
            self.encoded_error = None;
            return Ok(());
        }
        // Build every missing branch before committing any field so a failed audio
        // encoder cannot leave an apparently-enabled half-initialized pipeline.
        let requires_audio_encoder = self.settings.audio_enabled && self.audio_parameters.is_some();
        let prepared = (|| {
            let new_video_encoder = if self.encoder.is_none() {
                Some(CameraVideoEncoder::new(&self.settings)?)
            } else {
                None
            };
            let new_audio_encoder = if requires_audio_encoder && self.audio_encoder.is_none() {
                Some(CameraAudioEncoder::new(&self.settings)?)
            } else {
                None
            };
            Ok::<_, MediaError>((new_video_encoder, new_audio_encoder))
        })();
        let (new_video_encoder, new_audio_encoder) = match prepared {
            Ok(encoders) => encoders,
            Err(error) => {
                self.encoded_error = Some(error.to_string());
                // A previously failed branch may have left the enabled flag set while
                // the corresponding encoder was removed.  Normalize that state before
                // returning the error so consumers never observe a fake-live branch.
                if self.encoded_enabled
                    && (self.encoder.is_none()
                        || (requires_audio_encoder && self.audio_encoder.is_none()))
                {
                    self.encoded_enabled = false;
                    self.encoder = None;
                    self.audio_encoder = None;
                    self.pending_encoded.clear();
                }
                return Err(error);
            }
        };
        if let Some(encoder) = new_video_encoder {
            self.encoder = Some(encoder);
        }
        if let Some(encoder) = new_audio_encoder {
            self.audio_encoder = Some(encoder);
        }
        self.encoded_enabled = true;
        self.encoded_error = None;
        Ok(())
    }

    pub(crate) const fn play(&mut self) {
        self.playing = true;
    }

    pub(crate) const fn pause(&mut self) {
        self.playing = false;
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "媒体管线统一要求停止操作返回 MediaResult"
    )]
    pub(crate) const fn stop(&mut self) -> MediaResult<()> {
        self.playing = false;
        Ok(())
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "媒体管线统一要求重置操作返回 MediaResult"
    )]
    pub(crate) const fn reset(&mut self) -> MediaResult<()> {
        // Live device inputs are not seekable; reset only clears the local play flag.
        self.playing = false;
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "single capture read keeps preview and encoded branches visibly independent"
    )]
    pub(crate) fn read_source_output(&mut self) -> MediaResult<SourceReadOutput> {
        if let Some(packet) = self.pending_encoded.pop_front() {
            return Ok(SourceReadOutput {
                pacing_timestamp: packet
                    .dts
                    .or(packet.pts)
                    .map(|value| packet.time_base.rescale(value, MediaTimeBase::MPEG_CLOCK)),
                packet: Some(packet),
                preview_frames: Vec::new(),
                audio_frames: Vec::new(),
                metrics: MediaRuntimeMetrics::new(),
                branch_errors: Vec::new(),
                retry_after: None,
                looped: false,
                end_of_stream: false,
            });
        }
        let packet = match self.context.read_packet() {
            Ok(packet) => packet,
            Err(error) if is_transient_capture_error(&error) => {
                return Ok(SourceReadOutput {
                    pacing_timestamp: None,
                    packet: None,
                    preview_frames: Vec::new(),
                    audio_frames: Vec::new(),
                    metrics: MediaRuntimeMetrics::new(),
                    branch_errors: Vec::new(),
                    retry_after: Some(std::time::Duration::from_millis(10)),
                    looped: false,
                    end_of_stream: false,
                });
            }
            Err(error) => {
                return Err(MediaError::Camera(format!("FatalSource/Capture: {error}")));
            }
        };
        let Some(packet) = packet else {
            return Ok(SourceReadOutput::end_of_stream());
        };
        let stream_index = usize::try_from(packet.stream_index).ok();
        if stream_index == self.audio_stream_index {
            let mut metrics = MediaRuntimeMetrics::new();
            metrics.audio_packets_captured = 1;
            let mut branch_errors = Vec::new();
            let audio_frames =
                self.audio_preview_decoder
                    .as_mut()
                    .map_or_else(Vec::new, |decoder| match decoder.decode(&packet) {
                        Ok(frames) => frames,
                        Err(error) => {
                            branch_errors.push(format!("AudioDecode: {error}"));
                            Vec::new()
                        }
                    });
            metrics.audio_frames_decoded = audio_frames.len() as u64;
            if let Some(frame) = audio_frames.last() {
                (metrics.audio_rms, metrics.audio_peak) = audio_levels(&frame.samples);
            }
            if self.encoded_enabled
                && let Some(audio_encoder) = self.audio_encoder.as_mut()
            {
                let encode_result = audio_frames
                    .iter()
                    .try_for_each(|frame| audio_encoder.encode_pcm(frame));
                match encode_result {
                    Ok(()) => {
                        while let Some(packet) = audio_encoder.take_pending() {
                            metrics.audio_packets_encoded =
                                metrics.audio_packets_encoded.saturating_add(1);
                            self.pending_encoded.push_back(packet);
                        }
                    }
                    Err(error) => {
                        branch_errors.push(format!("AudioEncode: {error}"));
                        self.encoded_error = Some(error.to_string());
                        self.audio_encoder = None;
                        // Keep the independent video encoder alive so an audio
                        // failure cannot black out preview or discard already
                        // encoded video frames.  The runtime disconnects active
                        // encoded consumers for this failed branch and may
                        // disable the branch afterwards.
                        self.pending_encoded
                            .retain(|packet| packet.track == super::MediaTrackKind::Video);
                    }
                }
            }
            return Ok(SourceReadOutput {
                pacing_timestamp: valid_packet_timestamp(&packet, self.audio_time_base).or_else(
                    || {
                        audio_frames.first().and_then(|frame| {
                            frame.pts.map(|value| {
                                frame.time_base.rescale(value, MediaTimeBase::MPEG_CLOCK)
                            })
                        })
                    },
                ),
                packet: self.pending_encoded.pop_front(),
                preview_frames: Vec::new(),
                audio_frames,
                metrics,
                branch_errors,
                retry_after: None,
                looped: false,
                end_of_stream: false,
            });
        }
        if stream_index != Some(self.video_stream_index) {
            return Ok(SourceReadOutput {
                pacing_timestamp: None,
                packet: None,
                preview_frames: Vec::new(),
                audio_frames: Vec::new(),
                metrics: MediaRuntimeMetrics::new(),
                branch_errors: Vec::new(),
                retry_after: None,
                looped: false,
                end_of_stream: false,
            });
        }
        let raw_frames = self
            .decoder
            .decode_raw_frames(&packet)
            .map_err(|error| MediaError::Camera(format!("FatalSource/Decode: {error}")))?;
        let pacing_timestamp = raw_frames.first().and_then(|frame| {
            (frame.pts != ffi::AV_NOPTS_VALUE).then_some(
                self.video_time_base
                    .rescale(frame.pts, MediaTimeBase::MPEG_CLOCK),
            )
        });
        let mut metrics = MediaRuntimeMetrics::new();
        metrics.video_packets_captured = 1;
        metrics.video_frames_decoded = raw_frames.len() as u64;
        let mut branch_errors = Vec::new();
        let mut preview_frames = Vec::with_capacity(raw_frames.len());
        for frame in raw_frames {
            if self.preview_enabled {
                match self.decoder.preview_frame(&frame) {
                    Ok(frame) => preview_frames.push(frame),
                    Err(error) => branch_errors.push(format!("PreviewConversion: {error}")),
                }
            }
            if self.encoded_enabled
                && let Some(encoder) = self.encoder.as_mut()
            {
                match encoder.encode(&frame, self.video_time_base) {
                    Ok(()) => {
                        while let Some(packet) = encoder.take_pending() {
                            metrics.video_packets_encoded =
                                metrics.video_packets_encoded.saturating_add(1);
                            self.pending_encoded.push_back(packet);
                        }
                    }
                    Err(error) => {
                        branch_errors.push(format!("VideoEncode: {error}"));
                        self.encoded_error = Some(error.to_string());
                        self.encoded_enabled = false;
                        self.encoder = None;
                        self.audio_encoder = None;
                        self.pending_encoded.clear();
                    }
                }
            }
        }
        metrics.video_preview_frames = preview_frames.len() as u64;
        Ok(SourceReadOutput {
            pacing_timestamp,
            packet: self.pending_encoded.pop_front(),
            preview_frames,
            audio_frames: Vec::new(),
            metrics,
            branch_errors,
            retry_after: None,
            looped: false,
            end_of_stream: false,
        })
    }

    pub(crate) fn finish_encoded_packets(&mut self) -> MediaResult<Vec<super::EncodedMediaPacket>> {
        if let Some(encoder) = self.encoder.as_mut() {
            encoder.finish()?;
            while let Some(packet) = encoder.take_pending() {
                self.pending_encoded.push_back(packet);
            }
        }
        if let Some(audio_encoder) = self.audio_encoder.as_mut() {
            audio_encoder.finish()?;
            while let Some(packet) = audio_encoder.take_pending() {
                self.pending_encoded.push_back(packet);
            }
        }
        Ok(self.pending_encoded.drain(..).collect())
    }
}

fn valid_packet_timestamp(
    packet: &rsmpeg::avcodec::AVPacket,
    time_base: Option<MediaTimeBase>,
) -> Option<i64> {
    let time_base = time_base?;
    let timestamp = (packet.dts != ffi::AV_NOPTS_VALUE)
        .then_some(packet.dts)
        .or_else(|| (packet.pts != ffi::AV_NOPTS_VALUE).then_some(packet.pts))?;
    Some(time_base.rescale(timestamp, MediaTimeBase::MPEG_CLOCK))
}

fn is_transient_capture_error(error: &RsmpegError) -> bool {
    error.raw_error() == Some(ffi::AVERROR(ffi::EAGAIN))
}

#[cfg(test)]
mod capture_error_tests {
    use rsmpeg::{error::RsmpegError, ffi};

    use super::is_transient_capture_error;

    #[test]
    fn eagain_capture_error_should_be_retryable() {
        assert!(is_transient_capture_error(&RsmpegError::AVError(
            ffi::AVERROR(ffi::EAGAIN),
        )));
    }

    #[test]
    fn other_capture_error_should_remain_fatal() {
        assert!(!is_transient_capture_error(&RsmpegError::AVError(-1)));
    }
}

fn platform_input_format() -> MediaResult<AVInputFormatRef<'static>> {
    gblab_ffmpeg_device::register_devices();
    #[cfg(target_os = "macos")]
    {
        return AVInputFormat::find(c"avfoundation")
            .ok_or_else(|| MediaError::Camera("FFmpeg 未包含 avfoundation 输入设备".to_owned()));
    }
    #[cfg(target_os = "windows")]
    {
        return AVInputFormat::find(c"dshow")
            .ok_or_else(|| MediaError::Camera("FFmpeg 未包含 dshow 输入设备".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        return AVInputFormat::find(c"v4l2")
            .ok_or_else(|| MediaError::Camera("FFmpeg 未包含 v4l2 输入设备".to_owned()));
    }
    #[allow(unreachable_code)]
    Err(MediaError::UnsupportedSource(
        "当前平台不支持摄像头输入".to_owned(),
    ))
}

fn platform_device_url(
    settings: &CameraCaptureSettings,
    include_audio: bool,
) -> MediaResult<CString> {
    #[cfg(target_os = "macos")]
    {
        let video = gblab_ffmpeg_device::resolve_capture_device_input_id(
            settings.video_device_id.trim(),
            true,
        )
        .map_err(|error| MediaError::Camera(error.to_string()))?;
        let url = if include_audio && !settings.audio_device_id.trim().is_empty() {
            let audio = gblab_ffmpeg_device::resolve_capture_device_input_id(
                settings.audio_device_id.trim(),
                false,
            )
            .map_err(|error| MediaError::Camera(error.to_string()))?;
            format!("{video}:{audio}")
        } else {
            format!("{video}:none")
        };
        return CString::new(url).map_err(|_| MediaError::InvalidPath);
    }
    #[cfg(target_os = "windows")]
    {
        let mut value = format!("video={}", settings.video_device_id.trim());
        if include_audio && !settings.audio_device_id.trim().is_empty() {
            value.push_str(&format!(":audio={}", settings.audio_device_id.trim()));
        }
        return CString::new(value).map_err(|_| MediaError::InvalidPath);
    }
    #[cfg(target_os = "linux")]
    {
        return CString::new(settings.video_device_id.trim()).map_err(|_| MediaError::InvalidPath);
    }
    #[allow(unreachable_code)]
    Err(MediaError::UnsupportedSource(
        "当前平台不支持摄像头输入".to_owned(),
    ))
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod platform_tests {
    use std::{thread, time::Duration};

    use super::{list_capture_devices, video_capture_capabilities, video_encoder_capabilities};
    use crate::media::{
        AudioCodec, BackpressurePolicy, CameraCaptureSettings, FrameRateCapability,
        GlobalMediaRuntime, MediaConsumerKind, MediaSourceStatus,
    };

    #[test]
    #[ignore = "requires a connected camera and local camera permission"]
    fn camera_runtime_should_repeatedly_open_capture_preview_and_release()
    -> Result<(), Box<dyn std::error::Error>> {
        let devices = list_capture_devices()?;
        let microphone = devices.audio.into_iter().next();
        let Some(camera) = devices.video.into_iter().next() else {
            return Ok(());
        };
        let Some(mode) = video_capture_capabilities(&camera.id)?
            .modes
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        let Some(frame_rate) = mode.frame_rates.first().map(|capability| match capability {
            FrameRateCapability::Exact { value } => *value,
            FrameRateCapability::Range { maximum, .. } => *maximum,
        }) else {
            return Ok(());
        };
        let Some(encoder) = video_encoder_capabilities().encoders.into_iter().next() else {
            return Ok(());
        };
        let settings = CameraCaptureSettings {
            video_device_id: camera.id,
            video_codec: encoder.codec,
            video_bitrate: 1_000_000,
            encoder_backend: encoder.backend,
            audio_enabled: microphone.is_some(),
            audio_device_id: microphone.map_or_else(String::new, |device| device.id),
            audio_codec: AudioCodec::Aac,
            audio_sample_rate: 48_000,
            audio_channels: 2,
            audio_bitrate: 128_000,
            width: mode.width,
            height: mode.height,
            frames_per_second: frame_rate,
        };
        let runtime = GlobalMediaRuntime::start();
        let handle = runtime.handle();
        handle.open_camera(settings.clone())?;

        for _ in 0..2 {
            if handle.status().source_status == MediaSourceStatus::Stopped {
                handle.open_camera(settings.clone())?;
            }
            let mut live =
                handle.subscribe(MediaConsumerKind::Live, 512, BackpressurePolicy::Disconnect)?;
            handle.attach_preview()?;
            handle.play()?;
            for _ in 0..100 {
                if handle.status().decoded_frames > 0 {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            assert!(handle.status().decoded_frames > 0);
            if settings.audio_enabled {
                for _ in 0..100 {
                    let metrics = handle.status().metrics;
                    if metrics.audio_packets_captured > 0 && metrics.audio_frames_decoded > 0 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                let metrics = handle.status().metrics;
                assert!(metrics.audio_packets_captured > 0);
                assert!(metrics.audio_frames_decoded > 0);
            }
            handle.detach_preview()?;
            let mut received_live_packet = false;
            for _ in 0..100 {
                if live.try_recv().is_ok() {
                    received_live_packet = true;
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            assert!(received_live_packet);
            handle.unsubscribe(live.id)?;
            // With preview already detached, removing the last encoded consumer
            // releases the live capture on demand.
            for _ in 0..100 {
                if handle.status().source_status == MediaSourceStatus::Stopped {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(handle.status().source_status, MediaSourceStatus::Stopped);
        }

        runtime.shutdown()?;
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{CameraCaptureSettings, platform_device_url};

    #[test]
    fn macos_video_only_input_should_explicitly_disable_audio() {
        let settings = CameraCaptureSettings {
            video_device_id: "0".to_owned(),
            video_codec: super::super::VideoCodec::H264,
            video_bitrate: 4_096_000,
            encoder_backend: crate::configuration::EncoderBackend::Auto,
            audio_enabled: false,
            audio_device_id: String::new(),
            audio_codec: super::super::AudioCodec::Aac,
            audio_sample_rate: 48_000,
            audio_channels: 2,
            audio_bitrate: 128_000,
            width: 1920,
            height: 1080,
            frames_per_second: 25.0,
        };

        let result = platform_device_url(&settings, settings.audio_enabled);
        assert!(result.is_ok());
        if let Ok(url) = result {
            assert_eq!(url.as_bytes(), b"0:none");
        }
    }
}
