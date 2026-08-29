use std::{
    collections::VecDeque,
    ffi::CString,
    sync::{Arc, atomic::AtomicU8},
};

use rsmpeg::{
    avformat::{AVFormatContextInput, AVInputFormat, AVInputFormatRef},
    avutil::AVDictionary,
};

use super::{
    AudioStreamInfo, CameraCaptureSettings, CaptureDeviceInfo, CaptureDeviceLists, FrameRate,
    FrameRateCapability, MediaError, MediaResult, MediaTimeBase, Mp4ProbeResult,
    VideoCaptureCapabilities, VideoCaptureMode, VideoEncoderCapabilities, VideoEncoderCapability,
    VideoStreamInfo,
    audio_encoder::CameraAudioEncoder,
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

    fn input_description(&self) -> MediaResult<(CString, AVInputFormatRef<'static>)> {
        self.validate_settings()?;
        if self.settings.video_device_id.trim().is_empty() {
            return Err(MediaError::Camera("未设置摄像头设备标识".to_owned()));
        }
        let url = platform_device_url(&self.settings)?;
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
}

impl MediaSource for CameraMediaSource {
    fn probe(&self) -> MediaResult<Mp4ProbeResult> {
        let (url, format) = self.input_description()?;
        let options = self.capture_options()?;
        let (context, _interrupt_guard) = gblab_ffmpeg_device::open_input_with_interrupt(
            url.as_c_str(),
            &format,
            options,
            Arc::clone(&self.interrupt),
        )
        .map_err(|error| MediaError::Camera(error.to_string()))?;
        self.probe_context(&context)
    }

    fn open(&self, _looping: bool) -> MediaResult<MediaSourceSession> {
        let (url, format) = self.input_description()?;
        let options = self.capture_options()?;
        let (context, interrupt_guard) = gblab_ffmpeg_device::open_input_with_interrupt(
            url.as_c_str(),
            &format,
            options,
            Arc::clone(&self.interrupt),
        )
        .map_err(|error| MediaError::Camera(error.to_string()))?;
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
        let encoder = CameraVideoEncoder::new(&self.settings)?;
        let (audio_stream_index, audio_encoder, audio_time_base) = if self.settings.audio_enabled {
            let audio_stream = context
                .streams()
                .iter()
                .find(|stream| stream.codecpar().codec_type().is_audio())
                .ok_or_else(|| {
                    MediaError::Camera("已启用麦克风，但采集输入没有音频流".to_owned())
                })?;
            let index = usize::try_from(audio_stream.index)
                .map_err(|_| MediaError::Camera("FFmpeg 返回了无效的音频流索引".to_owned()))?;
            let audio_time_base =
                MediaTimeBase::new(audio_stream.time_base.num, audio_stream.time_base.den)
                    .ok_or_else(|| MediaError::Camera("摄像头音频时间基无效".to_owned()))?;
            let encoder = CameraAudioEncoder::new(&self.settings, &audio_stream.codecpar())?;
            (Some(index), Some(encoder), Some(audio_time_base))
        } else {
            (None, None, None)
        };
        Ok(MediaSourceSession::Camera(Box::new(CameraSession {
            _interrupt_guard: interrupt_guard,
            context,
            probe,
            playing: false,
            decoder,
            encoder,
            pending_encoded: VecDeque::new(),
            video_stream_index,
            video_time_base,
            audio_stream_index,
            audio_encoder,
            audio_time_base,
            preview_enabled: true,
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
    encoder: CameraVideoEncoder,
    pending_encoded: VecDeque<super::EncodedMediaPacket>,
    video_stream_index: usize,
    video_time_base: MediaTimeBase,
    audio_stream_index: Option<usize>,
    audio_encoder: Option<CameraAudioEncoder>,
    audio_time_base: Option<MediaTimeBase>,
    preview_enabled: bool,
}

impl CameraSession {
    pub(crate) const fn probe(&self) -> &Mp4ProbeResult {
        &self.probe
    }

    pub(crate) const fn set_preview_enabled(&mut self, enabled: bool) {
        self.preview_enabled = enabled;
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

    pub(crate) fn read_source_output(&mut self) -> MediaResult<SourceReadOutput> {
        if let Some(packet) = self.pending_encoded.pop_front() {
            return Ok(SourceReadOutput {
                packet: Some(packet),
                preview_frames: Vec::new(),
                looped: false,
                end_of_stream: false,
            });
        }
        let packet = self
            .context
            .read_packet()
            .map_err(|error| MediaError::Camera(error.to_string()))?;
        let Some(packet) = packet else {
            return Ok(SourceReadOutput::end_of_stream());
        };
        let stream_index = usize::try_from(packet.stream_index).ok();
        if stream_index == self.audio_stream_index {
            let audio_encoder = self
                .audio_encoder
                .as_mut()
                .ok_or_else(|| MediaError::Camera("音频流缺少编码器运行时".to_owned()))?;
            audio_encoder.encode_packet(
                &packet,
                self.audio_time_base.unwrap_or(MediaTimeBase::MPEG_CLOCK),
            )?;
            while let Some(packet) = audio_encoder.take_pending() {
                self.pending_encoded.push_back(packet);
            }
            return Ok(SourceReadOutput {
                packet: self.pending_encoded.pop_front(),
                preview_frames: Vec::new(),
                looped: false,
                end_of_stream: false,
            });
        }
        if stream_index != Some(self.video_stream_index) {
            return Ok(SourceReadOutput {
                packet: None,
                preview_frames: Vec::new(),
                looped: false,
                end_of_stream: false,
            });
        }
        let raw_frames = self
            .decoder
            .decode_raw_frames(&packet)
            .map_err(|error| MediaError::Camera(error.to_string()))?;
        let mut preview_frames = Vec::with_capacity(raw_frames.len());
        for frame in raw_frames {
            if self.preview_enabled {
                preview_frames.push(
                    self.decoder
                        .preview_frame(&frame)
                        .map_err(|error| MediaError::Camera(error.to_string()))?,
                );
            }
            self.encoder.encode(&frame, self.video_time_base)?;
            while let Some(packet) = self.encoder.take_pending() {
                self.pending_encoded.push_back(packet);
            }
        }
        Ok(SourceReadOutput {
            packet: self.pending_encoded.pop_front(),
            preview_frames,
            looped: false,
            end_of_stream: false,
        })
    }

    pub(crate) fn finish_encoded_packets(&mut self) -> MediaResult<Vec<super::EncodedMediaPacket>> {
        self.encoder.finish()?;
        if let Some(audio_encoder) = self.audio_encoder.as_mut() {
            audio_encoder.finish()?;
            while let Some(packet) = audio_encoder.take_pending() {
                self.pending_encoded.push_back(packet);
            }
        }
        while let Some(packet) = self.encoder.take_pending() {
            self.pending_encoded.push_back(packet);
        }
        Ok(self.pending_encoded.drain(..).collect())
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

fn platform_device_url(settings: &CameraCaptureSettings) -> MediaResult<CString> {
    #[cfg(target_os = "macos")]
    {
        let video = gblab_ffmpeg_device::resolve_capture_device_input_id(
            settings.video_device_id.trim(),
            true,
        )
        .map_err(|error| MediaError::Camera(error.to_string()))?;
        let url = if settings.audio_enabled && !settings.audio_device_id.trim().is_empty() {
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
        if settings.audio_enabled && !settings.audio_device_id.trim().is_empty() {
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
        let Some(camera) = list_capture_devices()?.video.into_iter().next() else {
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
            audio_enabled: false,
            audio_device_id: String::new(),
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
        handle.attach_preview()?;
        let mut live =
            handle.subscribe(MediaConsumerKind::Live, 8, BackpressurePolicy::Disconnect)?;

        for _ in 0..2 {
            handle.open_camera(settings.clone())?;
            handle.play()?;
            for _ in 0..100 {
                if handle.status().decoded_frames > 0 {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            assert!(handle.status().decoded_frames > 0);
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
            let stopped = handle.stop()?;
            assert_eq!(stopped.source_status, MediaSourceStatus::Stopped);
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

        let result = platform_device_url(&settings);
        assert!(result.is_ok());
        if let Ok(url) = result {
            assert_eq!(url.as_bytes(), b"0:none");
        }
    }
}
