//! Decoded PCM preview, level metering and bounded native speaker output.

use std::collections::VecDeque;

use rsmpeg::{
    avcodec::{AVCodec, AVCodecContext, AVCodecParameters, AVPacket},
    avutil::{AVChannelLayout, AVFrame, AVRational},
    error::RsmpegError,
    ffi,
    swresample::SwrContext,
};

use super::types::AudioSinkInfo;
use super::{MediaError, MediaResult, MediaTimeBase};

/// Native PCM format consumed by the local preview sink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AudioOutputFormat {
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

/// One bounded block of interleaved normalized PCM samples.
#[derive(Debug)]
pub(super) struct AudioPcmFrame {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    pub(crate) pts: Option<i64>,
    /// PCM timestamp units are explicit: one unit is one sample at `sample_rate`.
    pub(crate) time_base: MediaTimeBase,
}

/// Decodes one audio stream and converts it to packed `f32` PCM for metering and playback.
pub(super) struct AudioPreviewDecoder {
    decoder: AVCodecContext,
    resampler: Option<SwrContext>,
    output_layout: AVChannelLayout,
    output_format: AudioOutputFormat,
    source_time_base: MediaTimeBase,
    next_output_pts: i64,
}

impl AudioPreviewDecoder {
    pub(crate) fn new(
        parameters: &AVCodecParameters,
        source_time_base: MediaTimeBase,
        output_format: AudioOutputFormat,
    ) -> MediaResult<Self> {
        let codec = AVCodec::find_decoder(parameters.codec_id)
            .ok_or_else(|| MediaError::AudioPreview("未找到音频解码器".to_owned()))?;
        let mut decoder = AVCodecContext::new(&codec);
        let mut owned = AVCodecParameters::new();
        owned.copy(parameters);
        decoder
            .apply_codecpar(&owned)
            .map_err(|error| MediaError::AudioPreview(format!("初始化音频解码器失败：{error}")))?;
        decoder.set_pkt_timebase(AVRational {
            num: source_time_base.numerator,
            den: source_time_base.denominator,
        });
        decoder
            .open(None)
            .map_err(|error| MediaError::AudioPreview(format!("打开音频解码器失败：{error}")))?;
        Ok(Self {
            decoder,
            resampler: None,
            output_layout: AVChannelLayout::from_nb_channels(i32::from(output_format.channels)),
            output_format,
            source_time_base,
            next_output_pts: 0,
        })
    }

    pub(crate) fn decode(&mut self, packet: &AVPacket) -> MediaResult<Vec<AudioPcmFrame>> {
        self.decoder
            .send_packet(Some(packet))
            .map_err(|error| MediaError::AudioPreview(format!("提交音频 packet 失败：{error}")))?;
        self.receive_frames(false)
    }

    pub(crate) fn finish(&mut self) -> MediaResult<Vec<AudioPcmFrame>> {
        let mut output = match self.decoder.send_packet(None) {
            Ok(()) | Err(RsmpegError::DecoderFlushedError) => self.receive_frames(true)?,
            Err(error) => {
                return Err(MediaError::AudioPreview(format!(
                    "结束音频解码器失败：{error}"
                )));
            }
        };
        // Decoder drain does not necessarily emit the samples buffered by the
        // resampler. Flush that state before a loop/EOF reset so the final audio
        // tail is not discarded.
        output.extend(self.flush_resampler()?);
        Ok(output)
    }

    pub(crate) fn flush(&mut self) {
        self.decoder.flush_buffers();
        self.resampler = None;
        self.next_output_pts = 0;
    }

    fn receive_frames(&mut self, flushing: bool) -> MediaResult<Vec<AudioPcmFrame>> {
        let mut output = Vec::new();
        loop {
            match self.decoder.receive_frame() {
                Ok(frame) => {
                    if let Some(frame) = self.convert(&frame)? {
                        output.push(frame);
                    }
                }
                Err(RsmpegError::DecoderDrainError | RsmpegError::DecoderFlushedError) => break,
                Err(error) if flushing => {
                    return Err(MediaError::AudioPreview(format!(
                        "排空音频解码器失败：{error}"
                    )));
                }
                Err(error) => {
                    return Err(MediaError::AudioPreview(format!("读取音频帧失败：{error}")));
                }
            }
        }
        Ok(output)
    }

    fn convert(&mut self, input: &AVFrame) -> MediaResult<Option<AudioPcmFrame>> {
        let sample_rate = i32::try_from(self.output_format.sample_rate)
            .map_err(|_| MediaError::AudioPreview("输出采样率超出范围".to_owned()))?;
        let resampler = if let Some(resampler) = self.resampler.take() {
            resampler
        } else {
            let mut resampler = SwrContext::new(
                &self.output_layout,
                ffi::AV_SAMPLE_FMT_FLT,
                sample_rate,
                &input.ch_layout(),
                input.format,
                input.sample_rate,
            )
            .map_err(|error| MediaError::AudioPreview(format!("创建预览重采样器失败：{error}")))?;
            resampler.init().map_err(|error| {
                MediaError::AudioPreview(format!("初始化预览重采样器失败：{error}"))
            })?;
            resampler
        };
        let capacity = resampler.get_out_samples(input.nb_samples).max(1);
        let mut output = AVFrame::new();
        output.set_format(ffi::AV_SAMPLE_FMT_FLT);
        output.set_sample_rate(sample_rate);
        output.set_ch_layout(self.output_layout.clone().into_inner());
        output.set_nb_samples(capacity);
        output
            .alloc_buffer()
            .map_err(|error| MediaError::AudioPreview(format!("分配预览 PCM 失败：{error}")))?;
        resampler
            .convert_frame(Some(input), &mut output)
            .map_err(|error| MediaError::AudioPreview(format!("转换预览 PCM 失败：{error}")))?;
        self.resampler = Some(resampler);
        if output.nb_samples <= 0 {
            return Ok(None);
        }
        let samples = gblab_ffmpeg_device::copy_interleaved_f32(&output)
            .ok_or_else(|| MediaError::AudioPreview("读取预览 PCM 失败".to_owned()))?;
        let output_sample_rate = i32::try_from(self.output_format.sample_rate)
            .map_err(|_| MediaError::AudioPreview("输出采样率超出范围".to_owned()))?;
        let output_time_base = MediaTimeBase::new(1, output_sample_rate)
            .ok_or_else(|| MediaError::AudioPreview("输出 PCM 时间基无效".to_owned()))?;
        let pts = if input.pts == ffi::AV_NOPTS_VALUE {
            self.next_output_pts
        } else {
            self.source_time_base.rescale(input.pts, output_time_base)
        };
        self.next_output_pts = pts.saturating_add(i64::from(output.nb_samples));
        Ok(Some(AudioPcmFrame {
            samples,
            sample_rate: self.output_format.sample_rate,
            channels: self.output_format.channels,
            pts: Some(pts),
            time_base: output_time_base,
        }))
    }

    fn flush_resampler(&mut self) -> MediaResult<Vec<AudioPcmFrame>> {
        let Some(resampler) = self.resampler.take() else {
            return Ok(Vec::new());
        };
        let sample_rate = i32::try_from(self.output_format.sample_rate)
            .map_err(|_| MediaError::AudioPreview("输出采样率超出范围".to_owned()))?;
        let output_time_base = MediaTimeBase::new(1, sample_rate)
            .ok_or_else(|| MediaError::AudioPreview("输出 PCM 时间基无效".to_owned()))?;
        let mut frames = Vec::new();
        loop {
            let delayed_samples = resampler.get_out_samples(0);
            if delayed_samples <= 0 {
                break;
            }
            let mut output = AVFrame::new();
            output.set_format(ffi::AV_SAMPLE_FMT_FLT);
            output.set_sample_rate(sample_rate);
            output.set_ch_layout(self.output_layout.clone().into_inner());
            output.set_nb_samples(delayed_samples);
            output
                .alloc_buffer()
                .map_err(|error| MediaError::AudioPreview(format!("分配排空 PCM 失败：{error}")))?;
            resampler
                .convert_frame(None, &mut output)
                .map_err(|error| {
                    MediaError::AudioPreview(format!("排空音频重采样器失败：{error}"))
                })?;
            if output.nb_samples <= 0 {
                break;
            }
            let samples = gblab_ffmpeg_device::copy_interleaved_f32(&output)
                .ok_or_else(|| MediaError::AudioPreview("读取排空 PCM 失败".to_owned()))?;
            let pts = self.next_output_pts;
            self.next_output_pts = pts.saturating_add(i64::from(output.nb_samples));
            frames.push(AudioPcmFrame {
                samples,
                sample_rate: self.output_format.sample_rate,
                channels: self.output_format.channels,
                pts: Some(pts),
                time_base: output_time_base,
            });
        }
        self.resampler = Some(resampler);
        Ok(frames)
    }
}

/// Computes normalized RMS and peak values for an interleaved PCM block.
#[must_use]
#[expect(clippy::cast_precision_loss, reason = "PCM block sizes are small")]
pub(super) fn audio_levels(samples: &[f32]) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut square_sum = 0.0_f64;
    let mut peak = 0.0_f64;
    for sample in samples {
        let value = f64::from(*sample).abs().min(1.0);
        square_sum += value * value;
        peak = peak.max(value);
    }
    ((square_sum / samples.len() as f64).sqrt(), peak)
}

fn controlled_sample(sample: f32, muted: bool, volume: f32) -> f32 {
    if muted {
        0.0
    } else {
        sample * volume.clamp(0.0, 1.0)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    };

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::super::types::AudioSinkStatus;
    use super::{
        AudioOutputFormat, AudioSinkInfo, MediaError, MediaResult, VecDeque, controlled_sample,
    };

    const MAX_BUFFER_SECONDS: usize = 2;
    pub(super) const STATUS_PAUSED: u8 = 0;
    pub(super) const STATUS_PLAYING: u8 = 1;
    pub(super) const STATUS_ERROR: u8 = 2;

    pub(super) struct SinkState {
        pub(super) status: AtomicU8,
        pub(super) queued_samples: AtomicUsize,
        pub(super) played_samples: AtomicU64,
        pub(super) underruns: AtomicU64,
        pub(super) dropped_samples: AtomicU64,
        pub(super) last_error: Mutex<Option<String>>,
    }

    impl SinkState {
        pub(super) const fn new() -> Self {
            Self {
                status: AtomicU8::new(STATUS_PAUSED),
                queued_samples: AtomicUsize::new(0),
                played_samples: AtomicU64::new(0),
                underruns: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                last_error: Mutex::new(None),
            }
        }

        pub(super) fn set_error(&self, error: String) {
            self.status.store(STATUS_ERROR, Ordering::Release);
            // This is CPAL's error callback, not the realtime sample callback.  A short lock
            // here guarantees that the actionable native error is never silently discarded.
            match self.last_error.lock() {
                Ok(mut last_error) => *last_error = Some(error),
                Err(poisoned) => *poisoned.into_inner() = Some(error),
            }
        }

        pub(super) fn clear_error(&self) {
            match self.last_error.lock() {
                Ok(mut last_error) => *last_error = None,
                Err(poisoned) => *poisoned.into_inner() = None,
            }
        }

        pub(super) fn diagnostics(&self) -> AudioSinkInfo {
            let status = match self.status.load(Ordering::Acquire) {
                STATUS_PLAYING => AudioSinkStatus::Playing,
                STATUS_ERROR => AudioSinkStatus::Error,
                _ => AudioSinkStatus::Paused,
            };
            AudioSinkInfo {
                status,
                queued_samples: u64::try_from(self.queued_samples.load(Ordering::Acquire))
                    .unwrap_or(u64::MAX),
                played_samples: self.played_samples.load(Ordering::Acquire),
                underruns: self.underruns.load(Ordering::Acquire),
                dropped_samples: self.dropped_samples.load(Ordering::Acquire),
                last_error: match self.last_error.lock() {
                    Ok(error) => error.clone(),
                    Err(poisoned) => poisoned.into_inner().clone(),
                },
            }
        }
    }

    /// Bounded CPAL speaker sink. The callback never blocks on source production.
    pub(in crate::media) struct AudioPreviewSink {
        stream: cpal::Stream,
        queue: Arc<Mutex<VecDeque<f32>>>,
        muted: Arc<AtomicBool>,
        volume_bits: Arc<AtomicU32>,
        capacity: usize,
        format: AudioOutputFormat,
        state: Arc<SinkState>,
    }

    impl AudioPreviewSink {
        pub(crate) fn open() -> MediaResult<Self> {
            let device = cpal::default_host()
                .default_output_device()
                .ok_or_else(|| MediaError::AudioPreview("未找到系统音频输出设备".to_owned()))?;
            let supported = device.default_output_config().map_err(|error| {
                MediaError::AudioPreview(format!("读取音频输出配置失败：{error}"))
            })?;
            let config = supported.config();
            let format = AudioOutputFormat {
                sample_rate: config.sample_rate.0,
                channels: config.channels,
            };
            let capacity = usize::try_from(format.sample_rate)
                .unwrap_or(48_000)
                .saturating_mul(usize::from(format.channels))
                .saturating_mul(MAX_BUFFER_SECONDS);
            let queue = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
            let muted = Arc::new(AtomicBool::new(false));
            let volume_bits = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
            let state = Arc::new(SinkState::new());
            let stream = match supported.sample_format() {
                cpal::SampleFormat::F32 => {
                    build_stream::<f32>(&device, &config, &queue, &muted, &volume_bits, &state)
                }
                cpal::SampleFormat::I16 => {
                    build_stream::<i16>(&device, &config, &queue, &muted, &volume_bits, &state)
                }
                cpal::SampleFormat::U16 => {
                    build_stream::<u16>(&device, &config, &queue, &muted, &volume_bits, &state)
                }
                format => Err(MediaError::AudioPreview(format!(
                    "不支持的系统音频采样格式：{format:?}"
                ))),
            }?;
            if let Err(error) = stream.play() {
                state.set_error(error.to_string());
                return Err(MediaError::AudioPreview(format!(
                    "启动音频输出失败：{error}"
                )));
            }
            state.status.store(STATUS_PLAYING, Ordering::Release);
            Ok(Self {
                stream,
                queue,
                muted,
                volume_bits,
                capacity,
                format,
                state,
            })
        }

        pub(crate) const fn format(&self) -> AudioOutputFormat {
            self.format
        }

        #[expect(
            clippy::unnecessary_wraps,
            reason = "The non-native implementation returns None for the same runtime API"
        )]
        pub(crate) fn diagnostics(&self) -> Option<AudioSinkInfo> {
            Some(self.state.diagnostics())
        }

        pub(crate) fn push(&self, samples: &[f32]) {
            if let Ok(mut queue) = self.queue.lock() {
                let available = self.capacity.saturating_sub(queue.len());
                let accepted = samples.len().min(available);
                queue.extend(samples.iter().copied().take(accepted));
                self.state
                    .queued_samples
                    .store(queue.len(), Ordering::Release);
                self.state.dropped_samples.fetch_add(
                    u64::try_from(samples.len().saturating_sub(accepted)).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            } else {
                self.state.dropped_samples.fetch_add(
                    u64::try_from(samples.len()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
        }

        pub(crate) fn clear(&self) {
            if let Ok(mut queue) = self.queue.lock() {
                queue.clear();
                self.state.queued_samples.store(0, Ordering::Release);
            }
        }

        #[expect(
            clippy::cast_possible_truncation,
            reason = "CPAL sample gain is f32 and input is bounded to 0..=1"
        )]
        pub(crate) fn set_control(&self, muted: bool, volume: f64) {
            self.muted.store(muted, Ordering::Release);
            self.volume_bits
                .store((volume.clamp(0.0, 1.0) as f32).to_bits(), Ordering::Release);
        }

        pub(crate) fn pause(&self) -> MediaResult<()> {
            self.clear();
            self.stream.pause().map_err(|error| {
                self.state.set_error(error.to_string());
                MediaError::AudioPreview(format!("暂停音频输出失败：{error}"))
            })?;
            self.state.status.store(STATUS_PAUSED, Ordering::Release);
            Ok(())
        }

        pub(crate) fn resume(&self) -> MediaResult<()> {
            self.stream.play().map_err(|error| {
                self.state.set_error(error.to_string());
                MediaError::AudioPreview(format!("恢复音频输出失败：{error}"))
            })?;
            self.state.clear_error();
            self.state.status.store(STATUS_PLAYING, Ordering::Release);
            Ok(())
        }
    }

    fn build_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        queue: &Arc<Mutex<VecDeque<f32>>>,
        muted: &Arc<AtomicBool>,
        volume_bits: &Arc<AtomicU32>,
        state: &Arc<SinkState>,
    ) -> MediaResult<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let queue = Arc::clone(queue);
        let muted = Arc::clone(muted);
        let volume_bits = Arc::clone(volume_bits);
        let state = Arc::clone(state);
        let error_state = Arc::clone(&state);
        device
            .build_output_stream(
                config,
                move |output: &mut [T], _| {
                    let is_muted = muted.load(Ordering::Acquire);
                    let gain = f32::from_bits(volume_bits.load(Ordering::Acquire));
                    let mut played = 0_u64;
                    let mut underruns = 0_u64;
                    if let Ok(mut queue) = queue.try_lock() {
                        for target in output {
                            let sample = queue.pop_front().map_or_else(
                                || {
                                    underruns = underruns.saturating_add(1);
                                    0.0
                                },
                                |sample| {
                                    played = played.saturating_add(1);
                                    controlled_sample(sample, is_muted, gain)
                                },
                            );
                            *target = T::from_sample(sample);
                        }
                        state.queued_samples.store(queue.len(), Ordering::Release);
                    } else {
                        underruns = u64::try_from(output.len()).unwrap_or(u64::MAX);
                        for target in output {
                            *target = T::from_sample(0.0);
                        }
                    }
                    state.played_samples.fetch_add(played, Ordering::Relaxed);
                    state.underruns.fetch_add(underruns, Ordering::Relaxed);
                },
                move |error| error_state.set_error(error.to_string()),
                None,
            )
            .map_err(|error| MediaError::AudioPreview(format!("创建音频输出失败：{error}")))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) use native::AudioPreviewSink;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) struct AudioPreviewSink;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl AudioPreviewSink {
    pub(crate) fn open() -> MediaResult<Self> {
        Err(MediaError::AudioPreview(
            "当前平台不支持本地音频输出".to_owned(),
        ))
    }
    pub(crate) fn push(&self, _samples: &[f32]) {}
    #[expect(
        clippy::unnecessary_wraps,
        reason = "A non-native platform has no sink diagnostics"
    )]
    pub(crate) fn diagnostics(&self) -> Option<AudioSinkInfo> {
        None
    }
    pub(crate) fn clear(&self) {}
    pub(crate) fn set_control(&self, _muted: bool, _volume: f64) {}
    pub(crate) fn pause(&self) -> MediaResult<()> {
        Ok(())
    }
    pub(crate) fn resume(&self) -> MediaResult<()> {
        Ok(())
    }
    pub(crate) const fn format(&self) -> AudioOutputFormat {
        AudioOutputFormat {
            sample_rate: 48_000,
            channels: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{audio_levels, controlled_sample};

    #[test]
    fn silence_should_have_zero_rms_and_peak() {
        assert_eq!(audio_levels(&[0.0; 32]), (0.0, 0.0));
    }

    #[test]
    fn non_zero_pcm_should_report_rms_and_peak() {
        let (rms, peak) = audio_levels(&[0.5, -0.5, 1.0, -1.0]);
        assert!((rms - 0.790_569_415).abs() < 0.000_001);
        assert!((peak - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mute_and_volume_should_change_native_output_samples() {
        assert!(controlled_sample(0.8, true, 1.0).abs() < f32::EPSILON);
        assert!((controlled_sample(0.8, false, 0.25) - 0.2).abs() < f32::EPSILON);
        assert!((controlled_sample(0.8, false, 2.0) - 0.8).abs() < f32::EPSILON);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[cfg(test)]
mod sink_state_tests {
    use super::native::{STATUS_ERROR, STATUS_PLAYING, SinkState};
    use crate::media::AudioSinkStatus;
    use std::sync::atomic::Ordering;

    #[test]
    fn sink_diagnostics_should_expose_queue_counters_and_errors() {
        let state = SinkState::new();
        state.status.store(STATUS_PLAYING, Ordering::Release);
        state.queued_samples.store(12, Ordering::Release);
        state.played_samples.store(30, Ordering::Release);
        state.underruns.store(2, Ordering::Release);
        state.dropped_samples.store(4, Ordering::Release);
        state.set_error("device lost".to_owned());
        assert_eq!(state.status.load(Ordering::Acquire), STATUS_ERROR);
        let diagnostics = state.diagnostics();
        assert_eq!(diagnostics.status, AudioSinkStatus::Error);
        assert_eq!(diagnostics.queued_samples, 12);
        assert_eq!(diagnostics.played_samples, 30);
        assert_eq!(diagnostics.underruns, 2);
        assert_eq!(diagnostics.dropped_samples, 4);
        assert_eq!(diagnostics.last_error.as_deref(), Some("device lost"));
    }
}
