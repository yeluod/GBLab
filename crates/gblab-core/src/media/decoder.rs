use rsmpeg::{
    avcodec::{AVCodec, AVCodecContext, AVCodecParameters},
    avformat::AVStreamRef,
    avutil::{AVFrame, AVFrameWithImage, AVImage},
    ffi,
    swscale::SwsContext,
};
use std::collections::VecDeque;

use super::{MediaError, MediaResult, MediaVideoFrame};

/// `FFmpeg` 视频解码与 RGBA 转换器。
pub(super) struct VideoDecoder {
    context: AVCodecContext,
    scaler: Option<SwsContext>,
    pending_frames: VecDeque<MediaVideoFrame>,
    output_width: i32,
    output_height: i32,
    time_base_num: i32,
    time_base_den: i32,
}

impl VideoDecoder {
    pub(super) fn take_pending_frame(&mut self) -> Option<MediaVideoFrame> {
        self.pending_frames.pop_front()
    }

    pub(super) fn flush(&mut self) {
        self.context.flush_buffers();
        self.pending_frames.clear();
    }

    /// Flushes delayed decoder frames at end-of-stream.
    pub(super) fn finish_preview(&mut self) -> MediaResult<Vec<MediaVideoFrame>> {
        self.context
            .send_packet(None)
            .map_err(|error| MediaError::Playback(format!("结束视频解码器失败：{error}")))?;
        let mut frames = Vec::new();
        loop {
            match self.context.receive_frame() {
                Ok(frame) => frames.push(self.decode_frame(&frame)?),
                Err(
                    rsmpeg::error::RsmpegError::DecoderDrainError
                    | rsmpeg::error::RsmpegError::DecoderFlushedError,
                ) => break,
                Err(error) => {
                    return Err(MediaError::Playback(format!("排空视频解码器失败：{error}")));
                }
            }
        }
        Ok(frames)
    }

    pub(super) fn new(
        parameters: &rsmpeg::avcodec::AVCodecParametersRef<'_>,
        stream: &AVStreamRef<'_>,
    ) -> MediaResult<Self> {
        let codec = AVCodec::find_decoder(parameters.codec_id)
            .ok_or_else(|| MediaError::UnsupportedVideoCodec("未找到视频解码器".to_owned()))?;
        let mut context = AVCodecContext::new(&codec);
        let mut owned = AVCodecParameters::new();
        owned.copy(parameters);
        context
            .apply_codecpar(&owned)
            .map_err(|e| MediaError::Playback(format!("初始化视频解码器失败：{e}")))?;
        context
            .open(None)
            .map_err(|e| MediaError::Playback(format!("打开视频解码器失败：{e}")))?;
        Ok(Self {
            context,
            scaler: None,
            pending_frames: VecDeque::new(),
            output_width: i32::try_from(
                u32::try_from(parameters.width)
                    .map_err(|_| MediaError::Playback("视频宽度无效".to_owned()))?
                    .clamp(1, 480),
            )
            .map_err(|_| MediaError::Playback("预览宽度无效".to_owned()))?,
            output_height: {
                let width = u32::try_from(parameters.width)
                    .map_err(|_| MediaError::Playback("视频宽度无效".to_owned()))?;
                let height = u32::try_from(parameters.height)
                    .map_err(|_| MediaError::Playback("视频高度无效".to_owned()))?;
                let output_width = width.clamp(1, 480);
                i32::try_from(
                    (u64::from(height) * u64::from(output_width) / u64::from(width.max(1))).max(1),
                )
                .unwrap_or(1)
            },
            time_base_num: stream.time_base.num,
            time_base_den: stream.time_base.den,
        })
    }

    pub(super) fn decode_packet(
        &mut self,
        packet: &rsmpeg::avcodec::AVPacket,
    ) -> MediaResult<Option<MediaVideoFrame>> {
        self.context
            .send_packet(Some(packet))
            .map_err(|e| MediaError::Playback(format!("发送视频 packet 失败：{e}")))?;
        let first = match self.context.receive_frame() {
            Ok(frame) => Some(self.decode_frame(&frame)?),
            Err(rsmpeg::error::RsmpegError::DecoderDrainError) => None,
            Err(error) => return Err(MediaError::Playback(format!("读取视频帧失败：{error}"))),
        };
        while let Ok(frame) = self.context.receive_frame() {
            let decoded = self.decode_frame(&frame)?;
            self.pending_frames.push_back(decoded);
        }
        Ok(first)
    }

    /// Decodes all raw frames produced by one packet for a downstream encoder.
    pub(super) fn decode_raw_frames(
        &mut self,
        packet: &rsmpeg::avcodec::AVPacket,
    ) -> MediaResult<Vec<AVFrame>> {
        self.context
            .send_packet(Some(packet))
            .map_err(|error| MediaError::Playback(format!("发送视频 packet 失败：{error}")))?;
        let mut frames = Vec::new();
        loop {
            match self.context.receive_frame() {
                Ok(frame) => frames.push(frame),
                Err(rsmpeg::error::RsmpegError::DecoderDrainError) => break,
                Err(error) => {
                    return Err(MediaError::Playback(format!("读取视频帧失败：{error}")));
                }
            }
        }
        Ok(frames)
    }

    /// Converts one decoded raw frame into the bounded RGBA preview format.
    pub(super) fn preview_frame(&mut self, frame: &AVFrame) -> MediaResult<MediaVideoFrame> {
        self.decode_frame(frame)
    }

    fn decode_frame(&mut self, frame: &AVFrame) -> MediaResult<MediaVideoFrame> {
        let width = frame.width;
        let height = frame.height;
        if width <= 0 || height <= 0 {
            return Err(MediaError::Playback("解码帧尺寸无效".to_owned()));
        }
        let scaler = self
            .scaler
            .take()
            .or_else(|| {
                SwsContext::get_context(
                    width,
                    height,
                    ffi::AVPixelFormat::from(frame.format),
                    self.output_width,
                    self.output_height,
                    ffi::AV_PIX_FMT_RGBA,
                    2,
                    None,
                    None,
                    None,
                )
            })
            .ok_or_else(|| MediaError::Playback("创建像素格式转换器失败".to_owned()))?;
        let mut scaler = scaler;
        let image = AVImage::new(
            ffi::AV_PIX_FMT_RGBA,
            self.output_width,
            self.output_height,
            1,
        )
        .ok_or_else(|| MediaError::Playback("分配预览帧缓冲失败".to_owned()))?;
        let mut rgba = AVFrameWithImage::new(image);
        scaler
            .scale_frame(frame, 0, height, &mut rgba)
            .map_err(|e| MediaError::Playback(format!("转换预览帧失败：{e}")))?;
        let width = usize::try_from(self.output_width).unwrap_or(0);
        let height = usize::try_from(self.output_height).unwrap_or(0);
        let size = width.saturating_mul(height).saturating_mul(4);
        let mut bytes = vec![0u8; size];
        rgba.image_copy_to_buffer(&mut bytes, 1)
            .map_err(|e| MediaError::Playback(format!("读取预览帧缓冲失败：{e}")))?;
        self.scaler = Some(scaler);
        let position_seconds = if frame.pts == ffi::AV_NOPTS_VALUE || self.time_base_den <= 0 {
            0.0
        } else {
            #[expect(clippy::cast_precision_loss, reason = "预览时间轴使用 f64 秒精度")]
            let pts = frame.pts as f64;
            pts * f64::from(self.time_base_num) / f64::from(self.time_base_den)
        };
        Ok(MediaVideoFrame {
            width: u32::try_from(self.output_width).unwrap_or(0),
            height: u32::try_from(self.output_height).unwrap_or(0),
            rgba: bytes,
            position_seconds,
        })
    }
}
