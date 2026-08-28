use std::ffi::CString;

use rsmpeg::{
    avformat::{AVFormatContextInput, AVInputFormat, AVInputFormatRef},
    avutil::AVDictionary,
};

use super::{
    AudioCodec, AudioStreamInfo, CameraCaptureSettings, CaptureDeviceInfo, CaptureDeviceLists,
    MediaError, MediaPacket, MediaResult, MediaSource, MediaSourceSession, Mp4ProbeResult,
    VideoCodec, VideoStreamInfo,
};

/// 使用当前平台的原生 API 枚举 `FFmpeg` 可采集的输入设备。
pub fn list_capture_devices() -> MediaResult<CaptureDeviceLists> {
    let devices = gblab_ffmpeg_device::list_capture_devices()
        .map_err(|error| MediaError::Camera(format!("无法枚举本机采集设备：{error}")))?;
    Ok(CaptureDeviceLists {
        video: devices.video.into_iter().map(capture_device_info).collect(),
        audio: devices.audio.into_iter().map(capture_device_info).collect(),
    })
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
}

impl CameraMediaSource {
    /// 创建摄像头源描述。
    #[must_use]
    pub const fn new(settings: CameraCaptureSettings) -> Self {
        Self { settings }
    }

    fn input_description(&self) -> MediaResult<(CString, AVInputFormatRef<'static>)> {
        if self.settings.video_device_id.trim().is_empty() {
            return Err(MediaError::Camera("未设置摄像头设备标识".to_owned()));
        }
        let url = platform_device_url(&self.settings)?;
        let format = platform_input_format()?;
        Ok((url, format))
    }

    fn open_context(&self) -> MediaResult<AVFormatContextInput> {
        let (url, format) = self.input_description()?;
        let mut options = if self.settings.frames_per_second > 0 {
            Some(AVDictionary::new(
                c"framerate",
                CString::new(self.settings.frames_per_second.to_string())
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
        AVFormatContextInput::builder()
            .url(url.as_c_str())
            .format(&format)
            .options(&mut options)
            .open()
            .map_err(|error| MediaError::Camera(error.to_string()))
    }

    fn probe_context(&self, context: &AVFormatContextInput) -> MediaResult<Mp4ProbeResult> {
        let video_stream = context
            .streams()
            .iter()
            .find(|stream| stream.codecpar().codec_type().is_video())
            .ok_or(MediaError::MissingVideoStream)?;
        let video_parameters = video_stream.codecpar();
        let fps = video_stream.guess_framerate().map_or_else(
            || f64::from(self.settings.frames_per_second),
            |rate| {
                if rate.den == 0 {
                    0.0
                } else {
                    f64::from(rate.num) / f64::from(rate.den)
                }
            },
        );
        let video = VideoStreamInfo {
            codec: VideoCodec::RawVideo,
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
                        codec: AudioCodec::Pcm,
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
        let context = self.open_context()?;
        self.probe_context(&context)
    }

    fn open(&self, _looping: bool) -> MediaResult<MediaSourceSession> {
        let context = self.open_context()?;
        let probe = self.probe_context(&context)?;
        Ok(MediaSourceSession::Camera(CameraSession {
            context,
            probe,
            playing: false,
        }))
    }
}

/// 已打开的摄像头采集会话。
pub struct CameraSession {
    context: AVFormatContextInput,
    probe: Mp4ProbeResult,
    playing: bool,
}

impl CameraSession {
    pub(crate) const fn probe(&self) -> &Mp4ProbeResult {
        &self.probe
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

    pub(crate) fn next_packet(&mut self) -> MediaResult<Option<MediaPacket>> {
        if !self.playing {
            return Ok(None);
        }
        let packet = self
            .context
            .read_packet()
            .map_err(|error| MediaError::Camera(error.to_string()))?;
        let Some(packet) = packet else {
            return Ok(None);
        };
        let stream_index = usize::try_from(packet.stream_index)
            .map_err(|_| MediaError::Camera("FFmpeg 返回了无效的 stream index".to_owned()))?;
        Ok(Some(MediaPacket {
            stream_index,
            pts: (packet.pts != rsmpeg::ffi::AV_NOPTS_VALUE).then_some(packet.pts),
            dts: (packet.dts != rsmpeg::ffi::AV_NOPTS_VALUE).then_some(packet.dts),
            duration: packet.duration,
            size: usize::try_from(packet.size).unwrap_or(0),
            is_keyframe: packet.flags & rsmpeg::ffi::AV_PKT_FLAG_KEY.cast_signed() != 0,
            position_seconds: 0.0,
        }))
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
        let video = settings.video_device_id.trim();
        let url = if settings.audio_enabled && !settings.audio_device_id.trim().is_empty() {
            format!("{}:{}", video, settings.audio_device_id.trim())
        } else {
            video.to_owned()
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
