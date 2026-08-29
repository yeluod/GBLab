//! Audited native `FFmpeg` and capture-device FFI boundary.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    collections::BTreeMap,
    ffi::{CStr, c_void},
    ptr::{self, NonNull},
    slice,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use rsmpeg::{
    avcodec::{AVCodec, AVCodecParametersRef},
    avformat::{AVFormatContextInput, AVInputFormatRef},
    avutil::AVDictionary,
    error::RsmpegError,
    ffi,
};

#[cfg(target_os = "windows")]
mod windows_capture;

/// RAII guard connecting a Rust cancellation flag to an `AVFormatContext` interrupt callback.
pub struct InputInterruptGuard {
    context: NonNull<ffi::AVFormatContext>,
    opaque: NonNull<Arc<AtomicU8>>,
}

/// Why an FFmpeg input read was interrupted.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptReason {
    /// No interrupt is pending.
    None = 0,
    /// The preview consumer was detached.
    PreviewDetach = 1,
    /// Playback was paused.
    Pause = 2,
    /// Playback was stopped.
    Stop = 3,
    /// The source was closed.
    Close = 4,
    /// The runtime is shutting down.
    Shutdown = 5,
    /// A source is being replaced or reconfigured.
    Reconfigure = 6,
    /// The command response deadline elapsed.
    Timeout = 7,
}

impl InterruptReason {
    /// Decodes the atomic representation.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::PreviewDetach,
            2 => Self::Pause,
            3 => Self::Stop,
            4 => Self::Close,
            5 => Self::Shutdown,
            6 => Self::Reconfigure,
            7 => Self::Timeout,
            _ => Self::None,
        }
    }
}

impl Drop for InputInterruptGuard {
    fn drop(&mut self) {
        // SAFETY: The guard is declared before the owning input context and therefore drops while
        // the context is still alive. The opaque allocation belongs exclusively to this guard.
        unsafe {
            let context = self.context.as_mut();
            context.interrupt_callback.callback = None;
            context.interrupt_callback.opaque = ptr::null_mut();
            drop(Box::from_raw(self.opaque.as_ptr()));
        }
    }
}

/// Opens an FFmpeg input with the interrupt callback active during both open and stream probing.
///
/// # Errors
///
/// Returns the original FFmpeg open or stream-info error. All partially created FFmpeg and
/// dictionary allocations are released before returning an error.
pub fn open_input_with_interrupt(
    url: &CStr,
    format: &AVInputFormatRef<'_>,
    options: Option<AVDictionary>,
    flag: Arc<AtomicU8>,
) -> Result<(AVFormatContextInput, InputInterruptGuard), RsmpegError> {
    let mut context = unsafe { ffi::avformat_alloc_context() };
    let Some(context_pointer) = NonNull::new(context) else {
        return Err(RsmpegError::Unknown);
    };
    let opaque = attach_raw_interrupt(context_pointer, flag);
    let mut options_pointer =
        options.map_or(ptr::null_mut(), |dictionary| dictionary.into_raw().as_ptr());

    // SAFETY: Context, URL, input format, option dictionary and callback opaque allocation remain
    // valid for the complete avformat_open_input call. FFmpeg takes ownership of consumed options.
    let open_result = unsafe {
        ffi::avformat_open_input(
            &mut context,
            url.as_ptr(),
            format.as_ptr(),
            &mut options_pointer,
        )
    };
    drop_dictionary_pointer(options_pointer);
    if open_result < 0 {
        if !context.is_null() {
            // SAFETY: A non-null context after failed open remains owned by this function. The
            // interrupt opaque stays alive until FFmpeg has finished closing the context.
            unsafe { ffi::avformat_close_input(&mut context) };
        }
        drop_opaque(opaque);
        return Err(RsmpegError::OpenInputError(open_result));
    }

    let Some(context_pointer) = NonNull::new(context) else {
        drop_opaque(opaque);
        return Err(RsmpegError::Unknown);
    };
    // SAFETY: Successful avformat_open_input returns an owned initialized context.
    let mut input = unsafe { AVFormatContextInput::from_raw(context_pointer) };
    // SAFETY: The input context and interrupt callback remain valid during stream probing.
    let stream_info_result =
        unsafe { ffi::avformat_find_stream_info(input.as_mut_ptr(), ptr::null_mut()) };
    if stream_info_result < 0 {
        drop(input);
        drop_opaque(opaque);
        return Err(RsmpegError::FindStreamInfoError(stream_info_result));
    }

    Ok((
        input,
        InputInterruptGuard {
            context: context_pointer,
            opaque,
        },
    ))
}

fn attach_raw_interrupt(
    context_ptr: NonNull<ffi::AVFormatContext>,
    flag: Arc<AtomicU8>,
) -> NonNull<Arc<AtomicU8>> {
    let opaque = NonNull::from(Box::leak(Box::new(flag)));
    // SAFETY: Both pointers remain valid until `InputInterruptGuard` clears the callback and frees
    // the opaque Arc. FFmpeg only reads the callback structure while operating on this context.
    unsafe {
        let native = context_ptr.as_ptr();
        (*native).interrupt_callback.callback = Some(media_interrupt_callback);
        (*native).interrupt_callback.opaque = opaque.as_ptr().cast::<c_void>();
    }
    opaque
}

fn drop_opaque(opaque: NonNull<Arc<AtomicU8>>) {
    // SAFETY: The pointer was allocated by attach_raw_interrupt and is released exactly once.
    unsafe { drop(Box::from_raw(opaque.as_ptr())) };
}

fn drop_dictionary_pointer(pointer: *mut ffi::AVDictionary) {
    if let Some(pointer) = NonNull::new(pointer) {
        // SAFETY: FFmpeg returned ownership of the remaining option dictionary entries.
        unsafe { drop(AVDictionary::from_raw(pointer)) };
    }
}

unsafe extern "C" fn media_interrupt_callback(opaque: *mut c_void) -> i32 {
    if opaque.is_null() {
        return 0;
    }
    // SAFETY: `open_input_with_interrupt` stores an `Arc<AtomicU8>` allocation at this address,
    // and its guard keeps it alive until after the callback is detached.
    let flag = unsafe { &*opaque.cast::<Arc<AtomicU8>>() };
    i32::from(flag.load(Ordering::Acquire) != 0)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
use rsmpeg::avformat::AVInputFormat;

/// Native camera and microphone lists using identifiers accepted by FFmpeg.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeCaptureDeviceLists {
    /// Video input devices.
    pub video: Vec<NativeCaptureDevice>,
    /// Audio input devices.
    pub audio: Vec<NativeCaptureDevice>,
}

/// Failure while enumerating native capture devices.
#[derive(Debug, thiserror::Error)]
pub enum DeviceEnumerationError {
    /// The expected FFmpeg input format is not part of the linked build.
    #[error("FFmpeg input format {0} is unavailable")]
    InputFormatUnavailable(&'static str),
    /// The FFmpeg device backend rejected enumeration.
    #[error("FFmpeg input-device enumeration failed with error code {0}")]
    Ffmpeg(i32),
    /// An AVFoundation media-type symbol is unavailable on this system.
    #[cfg(target_os = "macos")]
    #[error("AVFoundation media type {0} is unavailable")]
    AvFoundationMediaTypeUnavailable(&'static str),
    /// A device identifier has the wrong native representation.
    #[error("invalid capture-device identifier: {0}")]
    InvalidDeviceIdentifier(String),
    /// The requested capture device is no longer present.
    #[error("capture device {0} was not found")]
    DeviceNotFound(String),
    /// The target platform has no implemented native capture backend.
    #[error("capture-device enumeration is unsupported on this platform")]
    UnsupportedPlatform,
    /// Windows DirectShow rejected native capability enumeration.
    #[cfg(target_os = "windows")]
    #[error("DirectShow capture capability enumeration failed: {0}")]
    DirectShow(String),
}

/// A native FFmpeg capture device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCaptureDevice {
    /// Identifier accepted by the corresponding FFmpeg input format.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Whether the device exposes video input.
    pub has_video: bool,
    /// Whether the device exposes audio input.
    pub has_audio: bool,
}

/// A native video capture mode exposed without opening the device.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeVideoCaptureMode {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Exact frame rates and continuous ranges reported by the backend.
    pub frame_rates: Vec<NativeFrameRateCapability>,
}

/// A native exact frame rate or continuous range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeFrameRateCapability {
    /// One exact rate.
    Exact(f64),
    /// Inclusive continuous range.
    Range { minimum: f64, maximum: f64 },
}

/// Encoded video codec family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeVideoCodec {
    /// H.264/AVC.
    H264,
    /// H.265/HEVC.
    H265,
}

/// Concrete encoder backend family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeEncoderBackend {
    /// Apple VideoToolbox.
    VideoToolbox,
    /// Windows Media Foundation.
    MediaFoundation,
    /// NVIDIA NVENC.
    Nvenc,
    /// Intel Quick Sync Video.
    Qsv,
    /// AMD AMF.
    Amf,
    /// FFmpeg built-in or other LGPL-compatible software encoder.
    Software,
}

/// A concrete video encoder present in the linked FFmpeg build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeVideoEncoderCapability {
    /// Output codec.
    pub codec: NativeVideoCodec,
    /// Backend family.
    pub backend: NativeEncoderBackend,
    /// Exact FFmpeg encoder name.
    pub encoder_name: String,
    /// Whether this is a hardware implementation.
    pub hardware: bool,
}

/// Enumerate video capture modes without starting a capture session.
///
/// # Errors
///
/// Returns an explicit error for an invalid device identifier or a platform whose native mode
/// enumeration boundary has not been implemented.
pub fn video_capture_modes(
    device_id: &str,
) -> Result<Vec<NativeVideoCaptureMode>, DeviceEnumerationError> {
    #[cfg(target_os = "macos")]
    {
        avfoundation_video_capture_modes(device_id)
    }
    #[cfg(target_os = "windows")]
    {
        windows_capture::video_capture_modes(device_id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = device_id;
        Err(DeviceEnumerationError::UnsupportedPlatform)
    }
}

/// Return video encoders that are actually present in the linked FFmpeg build.
#[must_use]
pub fn supported_video_encoders() -> Vec<NativeVideoEncoderCapability> {
    encoder_candidates()
        .iter()
        .filter_map(|candidate| {
            let name = std::ffi::CString::new(candidate.name).ok()?;
            AVCodec::find_encoder_by_name(name.as_c_str()).map(|_| NativeVideoEncoderCapability {
                codec: candidate.codec,
                backend: candidate.backend,
                encoder_name: candidate.name.to_owned(),
                hardware: candidate.hardware,
            })
        })
        .collect()
}

struct EncoderCandidate {
    name: &'static str,
    codec: NativeVideoCodec,
    backend: NativeEncoderBackend,
    hardware: bool,
}

fn encoder_candidates() -> &'static [EncoderCandidate] {
    #[cfg(target_os = "macos")]
    {
        &[
            EncoderCandidate {
                name: "h264_videotoolbox",
                codec: NativeVideoCodec::H264,
                backend: NativeEncoderBackend::VideoToolbox,
                hardware: true,
            },
            EncoderCandidate {
                name: "hevc_videotoolbox",
                codec: NativeVideoCodec::H265,
                backend: NativeEncoderBackend::VideoToolbox,
                hardware: true,
            },
        ]
    }
    #[cfg(target_os = "windows")]
    {
        &[
            EncoderCandidate {
                name: "h264_mf",
                codec: NativeVideoCodec::H264,
                backend: NativeEncoderBackend::MediaFoundation,
                hardware: true,
            },
            EncoderCandidate {
                name: "hevc_mf",
                codec: NativeVideoCodec::H265,
                backend: NativeEncoderBackend::MediaFoundation,
                hardware: true,
            },
            EncoderCandidate {
                name: "h264_nvenc",
                codec: NativeVideoCodec::H264,
                backend: NativeEncoderBackend::Nvenc,
                hardware: true,
            },
            EncoderCandidate {
                name: "hevc_nvenc",
                codec: NativeVideoCodec::H265,
                backend: NativeEncoderBackend::Nvenc,
                hardware: true,
            },
            EncoderCandidate {
                name: "h264_qsv",
                codec: NativeVideoCodec::H264,
                backend: NativeEncoderBackend::Qsv,
                hardware: true,
            },
            EncoderCandidate {
                name: "hevc_qsv",
                codec: NativeVideoCodec::H265,
                backend: NativeEncoderBackend::Qsv,
                hardware: true,
            },
            EncoderCandidate {
                name: "h264_amf",
                codec: NativeVideoCodec::H264,
                backend: NativeEncoderBackend::Amf,
                hardware: true,
            },
            EncoderCandidate {
                name: "hevc_amf",
                codec: NativeVideoCodec::H265,
                backend: NativeEncoderBackend::Amf,
                hardware: true,
            },
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        &[]
    }
}

/// Enumerate the current platform's camera and microphone devices.
///
/// macOS uses AVFoundation directly because its FFmpeg input driver does not implement
/// `avdevice_list_input_sources`. Persisted identifiers are stable AVFoundation unique IDs;
/// [`resolve_capture_device_input_id`] maps them to FFmpeg's current avfoundation index when a
/// capture session is opened. Other platforms use FFmpeg's native identifiers directly.
///
/// # Errors
///
/// Returns a backend-specific error when device discovery cannot run.
pub fn list_capture_devices() -> Result<NativeCaptureDeviceLists, DeviceEnumerationError> {
    #[cfg(target_os = "macos")]
    {
        return list_avfoundation_capture_devices();
    }
    #[cfg(target_os = "windows")]
    {
        return list_ffmpeg_capture_devices(c"dshow", "dshow");
    }
    #[cfg(target_os = "linux")]
    {
        let format = AVInputFormat::find(c"v4l2")
            .ok_or(DeviceEnumerationError::InputFormatUnavailable("v4l2"))?;
        let video = list_input_sources(&format)
            .map_err(DeviceEnumerationError::Ffmpeg)?
            .into_iter()
            .map(|mut device| {
                device.has_video = true;
                device.has_audio = false;
                device
            })
            .collect();
        return Ok(NativeCaptureDeviceLists {
            video,
            audio: Vec::new(),
        });
    }
    #[allow(unreachable_code)]
    Err(DeviceEnumerationError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
fn list_avfoundation_capture_devices() -> Result<NativeCaptureDeviceLists, DeviceEnumerationError> {
    use objc2_av_foundation::{AVMediaTypeAudio, AVMediaTypeVideo};

    // SAFETY: These are immutable AVFoundation framework symbols available on supported macOS.
    let video_media_type = unsafe { AVMediaTypeVideo }.ok_or(
        DeviceEnumerationError::AvFoundationMediaTypeUnavailable("video"),
    )?;
    // SAFETY: These are immutable AVFoundation framework symbols available on supported macOS.
    let audio_media_type = unsafe { AVMediaTypeAudio }.ok_or(
        DeviceEnumerationError::AvFoundationMediaTypeUnavailable("audio"),
    )?;

    Ok(NativeCaptureDeviceLists {
        video: list_avfoundation_devices(video_media_type, true),
        audio: list_avfoundation_devices(audio_media_type, false),
    })
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn list_avfoundation_devices(
    media_type: &objc2_av_foundation::AVMediaType,
    has_video: bool,
) -> Vec<NativeCaptureDevice> {
    use objc2_av_foundation::AVCaptureDevice;

    // `devicesWithMediaType` intentionally matches the ordering used by FFmpeg's avfoundation
    // input driver, so each generated index selects the same native device during capture.
    // SAFETY: `media_type` is one of AVFoundation's video/audio constants.
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(media_type) };
    devices
        .iter()
        .map(|device| {
            // SAFETY: AVFoundation returned a live AVCaptureDevice retained by the array iterator.
            let name = unsafe { device.localizedName() }.to_string();
            // SAFETY: `uniqueID` is an immutable string property of the retained device.
            let id = unsafe { device.uniqueID() }.to_string();
            NativeCaptureDevice {
                id,
                name,
                has_video,
                has_audio: !has_video,
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn avfoundation_video_capture_modes(
    device_id: &str,
) -> Result<Vec<NativeVideoCaptureMode>, DeviceEnumerationError> {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeVideo};
    use objc2_core_media::CMVideoFormatDescriptionGetDimensions;

    let resolved = resolve_avfoundation_device(device_id, true)?;
    let index = resolved
        .parse::<usize>()
        .map_err(|_| DeviceEnumerationError::InvalidDeviceIdentifier(device_id.to_owned()))?;
    // SAFETY: This is an immutable AVFoundation framework symbol available on supported macOS.
    let video_media_type = unsafe { AVMediaTypeVideo }.ok_or(
        DeviceEnumerationError::AvFoundationMediaTypeUnavailable("video"),
    )?;
    // Keep the same legacy enumeration order that FFmpeg's avfoundation input uses.
    // SAFETY: `video_media_type` is AVFoundation's video media-type constant.
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(video_media_type) };
    let device = devices
        .iter()
        .nth(index)
        .ok_or_else(|| DeviceEnumerationError::DeviceNotFound(device_id.to_owned()))?;
    // SAFETY: AVFoundation returned a live device and its immutable supported-format collection.
    let formats = unsafe { device.formats() };
    let mut modes = BTreeMap::<(u32, u32), Vec<(f64, f64)>>::new();
    for format in formats.iter() {
        // SAFETY: Both values are immutable properties of a retained AVCaptureDeviceFormat.
        let description = unsafe { format.formatDescription() };
        // SAFETY: The retained format description is a video description returned by a video
        // capture device format and remains alive for the duration of this call.
        let dimensions = unsafe { CMVideoFormatDescriptionGetDimensions(&description) };
        let (Ok(width), Ok(height)) = (
            u32::try_from(dimensions.width),
            u32::try_from(dimensions.height),
        ) else {
            continue;
        };
        if width == 0 || height == 0 {
            continue;
        }
        // SAFETY: The array and ranges are retained immutable AVFoundation values.
        let ranges = unsafe { format.videoSupportedFrameRateRanges() };
        let frame_rate_ranges = modes.entry((width, height)).or_default();
        frame_rate_ranges.extend(ranges.iter().filter_map(|range| {
            // SAFETY: Frame-rate bounds are immutable scalar properties.
            let min = unsafe { range.minFrameRate() };
            // SAFETY: Frame-rate bounds are immutable scalar properties.
            let max = unsafe { range.maxFrameRate() };
            (min.is_finite() && max.is_finite() && min > 0.0 && max >= min).then_some((min, max))
        }));
    }

    Ok(merge_capture_modes(modes))
}

fn merge_capture_modes(
    modes: BTreeMap<(u32, u32), Vec<(f64, f64)>>,
) -> Vec<NativeVideoCaptureMode> {
    modes
        .into_iter()
        .map(|((width, height), ranges)| NativeVideoCaptureMode {
            width,
            height,
            frame_rates: frame_rate_capabilities(&ranges),
        })
        .filter(|mode| !mode.frame_rates.is_empty())
        .collect()
}

fn frame_rate_capabilities(ranges: &[(f64, f64)]) -> Vec<NativeFrameRateCapability> {
    ranges
        .iter()
        .filter_map(|&(minimum, maximum)| {
            if !minimum.is_finite() || !maximum.is_finite() || minimum <= 0.0 || maximum < minimum {
                return None;
            }
            if (maximum - minimum).abs() < f64::EPSILON {
                Some(NativeFrameRateCapability::Exact(minimum))
            } else {
                Some(NativeFrameRateCapability::Range { minimum, maximum })
            }
        })
        .collect()
}

/// Resolves a persisted stable device identifier to the string accepted by FFmpeg's input.
///
/// # Errors
///
/// Returns an error when the stable identifier no longer maps to a connected device.
pub fn resolve_capture_device_input_id(
    device_id: &str,
    video: bool,
) -> Result<String, DeviceEnumerationError> {
    #[cfg(target_os = "macos")]
    {
        resolve_avfoundation_device(device_id, video)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = video;
        if device_id.trim().is_empty() {
            Err(DeviceEnumerationError::InvalidDeviceIdentifier(
                device_id.to_owned(),
            ))
        } else {
            Ok(device_id.to_owned())
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn resolve_avfoundation_device(
    device_id: &str,
    video: bool,
) -> Result<String, DeviceEnumerationError> {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo};

    let media_type = if video {
        // SAFETY: Immutable AVFoundation framework symbol.
        unsafe { AVMediaTypeVideo }.ok_or(
            DeviceEnumerationError::AvFoundationMediaTypeUnavailable("video"),
        )?
    } else {
        // SAFETY: Immutable AVFoundation framework symbol.
        unsafe { AVMediaTypeAudio }.ok_or(
            DeviceEnumerationError::AvFoundationMediaTypeUnavailable("audio"),
        )?
    };
    // SAFETY: `media_type` is one of AVFoundation's immutable media-type constants.
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(media_type) };
    if let Ok(index) = device_id.trim().parse::<usize>()
        && devices.iter().nth(index).is_some()
    {
        return Ok(index.to_string());
    }
    devices
        .iter()
        .enumerate()
        .find_map(|(index, device)| {
            // SAFETY: `uniqueID` is immutable for the retained device.
            (unsafe { device.uniqueID() }.to_string() == device_id).then(|| index.to_string())
        })
        .ok_or_else(|| DeviceEnumerationError::DeviceNotFound(device_id.to_owned()))
}

#[cfg(target_os = "windows")]
fn list_ffmpeg_capture_devices(
    format_name: &CStr,
    display_name: &'static str,
) -> Result<NativeCaptureDeviceLists, DeviceEnumerationError> {
    let format = AVInputFormat::find(format_name)
        .ok_or(DeviceEnumerationError::InputFormatUnavailable(display_name))?;
    let devices = list_input_sources(&format).map_err(DeviceEnumerationError::Ffmpeg)?;
    let mut lists = NativeCaptureDeviceLists::default();
    for device in devices {
        if device.has_video || !device.has_audio {
            lists.video.push(device.clone());
        }
        if device.has_audio || !device.has_video {
            lists.audio.push(device);
        }
    }
    Ok(lists)
}

/// Register FFmpeg's input and output device backends.
pub fn register_devices() {
    // SAFETY: FFmpeg's registration routine has no arguments and is process-global by design.
    unsafe { ffi::avdevice_register_all() };
}

/// Copies an FFmpeg packet payload into Rust-owned memory.
///
/// This audited boundary keeps pointer validation and slice construction out of `gblab-core`,
/// which forbids unsafe code. An empty or malformed FFmpeg packet produces an empty vector.
#[must_use]
pub fn copy_packet_data(packet: &rsmpeg::avcodec::AVPacket) -> Vec<u8> {
    let Ok(size) = usize::try_from(packet.size) else {
        return Vec::new();
    };
    if size == 0 || packet.data.is_null() {
        return Vec::new();
    }
    // SAFETY: FFmpeg guarantees that a packet with positive `size` exposes at least `size`
    // readable bytes at `data` for the lifetime of the packet. The bytes are copied immediately.
    unsafe { slice::from_raw_parts(packet.data.cast_const(), size) }.to_vec()
}

/// Copies codec initialization data (extradata) from FFmpeg-owned parameters.
///
/// This keeps raw-pointer validation inside the audited native boundary so higher-level media
/// code can expose H.264/H.265 parameter sets and AAC AudioSpecificConfig safely.
#[must_use]
pub fn copy_codec_extradata(parameters: &AVCodecParametersRef<'_>) -> Option<Vec<u8>> {
    copy_extradata(parameters.extradata, parameters.extradata_size)
}

/// Copies extradata from an owned codec-parameter allocation.
#[must_use]
pub fn copy_owned_codec_extradata(
    parameters: &rsmpeg::avcodec::AVCodecParameters,
) -> Option<Vec<u8>> {
    copy_extradata(parameters.extradata, parameters.extradata_size)
}

fn copy_extradata(pointer: *mut u8, size: i32) -> Option<Vec<u8>> {
    let size = usize::try_from(size).ok()?;
    if size == 0 || pointer.is_null() {
        return None;
    }
    // SAFETY: FFmpeg owns `extradata` for the lifetime of codec parameters and advertises its
    // readable length through `extradata_size`; this function copies it immediately.
    Some(unsafe { slice::from_raw_parts(pointer.cast_const(), size) }.to_vec())
}

/// Enumerate input sources for one FFmpeg device format.
///
/// # Errors
///
/// Returns the FFmpeg error code when the device backend cannot enumerate sources.
pub fn list_input_sources(format: &AVInputFormatRef<'_>) -> Result<Vec<NativeCaptureDevice>, i32> {
    let mut device_list = ptr::null_mut();
    register_devices();
    // SAFETY: `format` owns a valid FFmpeg format pointer. Null optional arguments are allowed,
    // and `device_list` is released below with FFmpeg's matching deallocator.
    let result = unsafe {
        ffi::avdevice_list_input_sources(
            format.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            &mut device_list,
        )
    };
    if result < 0 {
        return Err(result);
    }
    if device_list.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: A successful FFmpeg call returns an initialized AVDeviceInfoList. Pointer and
    // length checks are performed before creating slices, and strings are copied immediately.
    let devices = unsafe {
        let list = &*device_list;
        if list.nb_devices <= 0 || list.devices.is_null() {
            Vec::new()
        } else {
            slice::from_raw_parts(list.devices, list.nb_devices as usize)
                .iter()
                .filter_map(|device| device.as_ref())
                .filter_map(|device| {
                    let id = copy_c_string(device.device_name)?;
                    let name =
                        copy_c_string(device.device_description).unwrap_or_else(|| id.clone());
                    Some(NativeCaptureDevice {
                        id,
                        name,
                        has_video: has_media_type(device, ffi::AVMEDIA_TYPE_VIDEO),
                        has_audio: has_media_type(device, ffi::AVMEDIA_TYPE_AUDIO),
                    })
                })
                .collect()
        }
    };
    // SAFETY: `device_list` was allocated by `avdevice_list_input_sources` and is freed once.
    unsafe { ffi::avdevice_free_list_devices(&mut device_list) };
    Ok(devices)
}

unsafe fn has_media_type(device: &ffi::AVDeviceInfo, media_type: ffi::AVMediaType) -> bool {
    if device.media_types.is_null() || device.nb_media_types <= 0 {
        return false;
    }
    // SAFETY: FFmpeg guarantees `nb_media_types` elements when media_types is non-null.
    unsafe { slice::from_raw_parts(device.media_types, device.nb_media_types as usize) }
        .contains(&media_type)
}

unsafe fn copy_c_string(value: *const std::os::raw::c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: FFmpeg device info fields are null-terminated for the lifetime of the list.
    Some(
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{DeviceEnumerationError, list_capture_devices, video_capture_modes};

    #[test]
    fn macos_enumeration_should_succeed_when_no_devices_are_connected() {
        assert!(list_capture_devices().is_ok());
    }

    #[test]
    fn macos_mode_enumeration_should_reject_unknown_stable_identifier() {
        assert!(matches!(
            video_capture_modes("not-an-index"),
            Err(DeviceEnumerationError::DeviceNotFound(_))
        ));
    }

    #[test]
    fn macos_connected_camera_should_expose_native_modes() {
        let Ok(devices) = list_capture_devices() else {
            return;
        };
        let Some(camera) = devices.video.first() else {
            return;
        };

        let modes = video_capture_modes(&camera.id);
        assert!(modes.is_ok(), "native mode enumeration failed: {modes:?}");
    }
}

#[cfg(test)]
mod pure_tests {
    use std::collections::BTreeMap;

    use super::{
        InterruptReason, NativeFrameRateCapability, NativeVideoCaptureMode,
        frame_rate_capabilities, merge_capture_modes,
    };

    #[test]
    fn frame_rate_ranges_should_preserve_native_fractional_values() {
        assert_eq!(
            frame_rate_capabilities(&[(15.0, 29.97)]),
            [NativeFrameRateCapability::Range {
                minimum: 15.0,
                maximum: 29.97,
            }]
        );
    }

    #[test]
    fn duplicate_dimensions_should_merge_frame_rate_ranges() {
        let modes = BTreeMap::from([((1920, 1080), vec![(25.0, 30.0), (50.0, 60.0)])]);

        assert_eq!(
            merge_capture_modes(modes),
            [NativeVideoCaptureMode {
                width: 1920,
                height: 1080,
                frame_rates: vec![
                    NativeFrameRateCapability::Range {
                        minimum: 25.0,
                        maximum: 30.0,
                    },
                    NativeFrameRateCapability::Range {
                        minimum: 50.0,
                        maximum: 60.0,
                    },
                ],
            }]
        );
    }

    #[test]
    fn interrupt_reason_wire_values_should_round_trip_and_ignore_unknown_values() {
        for reason in [
            InterruptReason::None,
            InterruptReason::PreviewDetach,
            InterruptReason::Pause,
            InterruptReason::Stop,
            InterruptReason::Close,
            InterruptReason::Shutdown,
            InterruptReason::Reconfigure,
            InterruptReason::Timeout,
        ] {
            assert_eq!(InterruptReason::from_u8(reason as u8), reason);
        }
        assert_eq!(InterruptReason::from_u8(255), InterruptReason::None);
    }
}
