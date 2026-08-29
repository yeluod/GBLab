//! Narrowly scoped native input-device enumeration boundary.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{collections::BTreeMap, ffi::CStr, ptr, slice};

use rsmpeg::{avcodec::AVCodec, avformat::AVInputFormatRef, ffi};

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeVideoCaptureMode {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Practical integer frame rates supported by this mode.
    pub frame_rates: Vec<u32>,
}

/// Video encoders provided by the linked FFmpeg libraries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeVideoEncoder {
    /// H.264/AVC encoder.
    H264,
    /// H.265/HEVC encoder.
    H265,
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = device_id;
        Err(DeviceEnumerationError::UnsupportedPlatform)
    }
}

/// Return video encoders that are actually present in the linked FFmpeg build.
#[must_use]
pub fn supported_video_encoders() -> Vec<NativeVideoEncoder> {
    let mut encoders = Vec::with_capacity(2);
    if AVCodec::find_encoder(ffi::AV_CODEC_ID_H264).is_some() {
        encoders.push(NativeVideoEncoder::H264);
    }
    if AVCodec::find_encoder(ffi::AV_CODEC_ID_HEVC).is_some() {
        encoders.push(NativeVideoEncoder::H265);
    }
    encoders
}

/// Enumerate the current platform's camera and microphone devices.
///
/// macOS uses AVFoundation directly because its FFmpeg input driver does not implement
/// `avdevice_list_input_sources`. Device identifiers remain the per-media-type indices that
/// FFmpeg's `avfoundation` input accepts. Other supported platforms use FFmpeg's native device
/// enumeration API.
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
        .enumerate()
        .map(|(index, device)| {
            // SAFETY: AVFoundation returned a live AVCaptureDevice retained by the array iterator.
            let name = unsafe { device.localizedName() }.to_string();
            NativeCaptureDevice {
                id: index.to_string(),
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

    let index = device_id
        .trim()
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
            frame_rates: practical_frame_rates(&ranges),
        })
        .filter(|mode| !mode.frame_rates.is_empty())
        .collect()
}

fn practical_frame_rates(ranges: &[(f64, f64)]) -> Vec<u32> {
    let mut frame_rates = std::collections::BTreeSet::new();
    for &(min, max) in ranges {
        // FFmpeg's avfoundation input validates the range endpoints as discrete accepted values;
        // interpolating common rates inside the interval can produce modes the driver rejects.
        for endpoint in [min, max] {
            let rounded = endpoint.round();
            if rounded > 0.0 && rounded <= f64::from(u32::MAX) {
                frame_rates.insert(rounded as u32);
            }
        }
    }
    frame_rates.into_iter().collect()
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
    fn macos_mode_enumeration_should_reject_non_numeric_identifier() {
        assert!(matches!(
            video_capture_modes("not-an-index"),
            Err(DeviceEnumerationError::InvalidDeviceIdentifier(_))
        ));
    }
}

#[cfg(test)]
mod pure_tests {
    use std::collections::BTreeMap;

    use super::{NativeVideoCaptureMode, merge_capture_modes, practical_frame_rates};

    #[test]
    fn frame_rate_ranges_should_only_include_driver_reported_endpoints() {
        assert_eq!(practical_frame_rates(&[(15.0, 29.97)]), [15, 30]);
    }

    #[test]
    fn duplicate_dimensions_should_merge_frame_rate_ranges() {
        let modes = BTreeMap::from([((1920, 1080), vec![(25.0, 30.0), (50.0, 60.0)])]);

        assert_eq!(
            merge_capture_modes(modes),
            [NativeVideoCaptureMode {
                width: 1920,
                height: 1080,
                frame_rates: vec![25, 30, 50, 60],
            }]
        );
    }
}
