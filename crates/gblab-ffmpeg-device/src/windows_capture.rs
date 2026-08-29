//! Audited DirectShow capture-capability enumeration for Windows.

use std::{collections::BTreeMap, ffi::c_void, mem, ptr};

use windows::{
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
        Media::{
            DirectShow::{
                IAMStreamConfig, IBaseFilter, ICaptureGraphBuilder2, ICreateDevEnum, IGraphBuilder,
                VIDEO_STREAM_CONFIG_CAPS,
            },
            MediaFoundation::{
                AM_MEDIA_TYPE, CLSID_CaptureGraphBuilder2, CLSID_FilterGraph,
                CLSID_SystemDeviceEnum, CLSID_VideoInputDeviceCategory, FORMAT_VideoInfo,
                FORMAT_VideoInfo2, MEDIATYPE_Video, PIN_CATEGORY_CAPTURE, VIDEOINFOHEADER,
                VIDEOINFOHEADER2,
            },
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, IEnumMoniker, IMoniker,
                StructuredStorage::IPropertyBag,
            },
            Variant::{VARIANT, VT_BSTR, VariantClear},
        },
    },
    core::{BSTR, Interface, w},
};

use crate::{DeviceEnumerationError, NativeVideoCaptureMode, merge_capture_modes};

const REFERENCE_TIME_PER_SECOND: f64 = 10_000_000.0;

pub(super) fn video_capture_modes(
    device_id: &str,
) -> Result<Vec<NativeVideoCaptureMode>, DeviceEnumerationError> {
    let device_id = device_id.trim();
    if device_id.is_empty() {
        return Err(DeviceEnumerationError::InvalidDeviceIdentifier(
            device_id.to_owned(),
        ));
    }

    // SAFETY: Every COM interface and allocation is kept inside this function and released by its
    // typed wrapper or the explicit media-type cleanup before the apartment is uninitialized.
    unsafe { enumerate_modes(device_id) }
}

unsafe fn enumerate_modes(
    device_id: &str,
) -> Result<Vec<NativeVideoCaptureMode>, DeviceEnumerationError> {
    let _apartment = unsafe { ComApartment::initialize() }?;
    let moniker = unsafe { find_video_moniker(device_id) }?;
    let filter: IBaseFilter = unsafe { moniker.BindToObject(None, None) }.map_err(direct_show)?;
    let graph: IGraphBuilder =
        unsafe { CoCreateInstance(&CLSID_FilterGraph, None, CLSCTX_INPROC_SERVER) }
            .map_err(direct_show)?;
    let capture_graph: ICaptureGraphBuilder2 =
        unsafe { CoCreateInstance(&CLSID_CaptureGraphBuilder2, None, CLSCTX_INPROC_SERVER) }
            .map_err(direct_show)?;
    unsafe { graph.AddFilter(&filter, w!("GBLab Camera")) }.map_err(direct_show)?;
    unsafe { capture_graph.SetFiltergraph(&graph) }.map_err(direct_show)?;

    let mut stream_config_raw = ptr::null_mut::<c_void>();
    unsafe {
        capture_graph.FindInterface(
            Some(&PIN_CATEGORY_CAPTURE),
            Some(&MEDIATYPE_Video),
            &filter,
            &IAMStreamConfig::IID,
            &mut stream_config_raw,
        )
    }
    .map_err(direct_show)?;
    if stream_config_raw.is_null() {
        return Err(DeviceEnumerationError::DirectShow(
            "capture pin did not expose IAMStreamConfig".to_owned(),
        ));
    }
    // SAFETY: FindInterface returned an owned IAMStreamConfig interface for the requested IID.
    let stream_config = unsafe { IAMStreamConfig::from_raw(stream_config_raw) };
    let modes = unsafe { stream_modes(&stream_config) }?;
    Ok(modes)
}

unsafe fn stream_modes(
    stream_config: &IAMStreamConfig,
) -> Result<Vec<NativeVideoCaptureMode>, DeviceEnumerationError> {
    let mut count = 0;
    let mut capability_size = 0;
    unsafe { stream_config.GetNumberOfCapabilities(&mut count, &mut capability_size) }
        .map_err(direct_show)?;
    let expected_size = i32::try_from(mem::size_of::<VIDEO_STREAM_CONFIG_CAPS>())
        .map_err(|_| DeviceEnumerationError::DirectShow("capability size overflow".to_owned()))?;
    if count < 0 || capability_size != expected_size {
        return Err(DeviceEnumerationError::DirectShow(format!(
            "unexpected capability layout: count={count}, size={capability_size}"
        )));
    }

    let mut modes = BTreeMap::<(u32, u32), Vec<(f64, f64)>>::new();
    for index in 0..count {
        let mut media_type = ptr::null_mut::<AM_MEDIA_TYPE>();
        let mut capabilities = VIDEO_STREAM_CONFIG_CAPS::default();
        unsafe {
            stream_config.GetStreamCaps(
                index,
                &mut media_type,
                ptr::from_mut(&mut capabilities).cast::<u8>(),
            )
        }
        .map_err(direct_show)?;
        let owned = OwnedMediaType::new(media_type);
        let Some((width, height)) = (unsafe { owned.dimensions() }) else {
            continue;
        };
        let ranges = capture_frame_rate_ranges(&capabilities);
        if !ranges.is_empty() {
            modes.entry((width, height)).or_default().extend(ranges);
        }
    }

    Ok(merge_capture_modes(modes))
}

fn capture_frame_rate_ranges(capabilities: &VIDEO_STREAM_CONFIG_CAPS) -> Vec<(f64, f64)> {
    let minimum_interval = capabilities.MinFrameInterval;
    let maximum_interval = capabilities.MaxFrameInterval;
    if minimum_interval <= 0 || maximum_interval < minimum_interval {
        return Vec::new();
    }
    let minimum = REFERENCE_TIME_PER_SECOND / maximum_interval as f64;
    let maximum = REFERENCE_TIME_PER_SECOND / minimum_interval as f64;
    if !minimum.is_finite() || !maximum.is_finite() || minimum <= 0.0 || maximum < minimum {
        return Vec::new();
    }
    vec![(minimum, maximum)]
}

unsafe fn find_video_moniker(device_id: &str) -> Result<IMoniker, DeviceEnumerationError> {
    let enumerator: ICreateDevEnum =
        unsafe { CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER) }
            .map_err(direct_show)?;
    let mut monikers: Option<IEnumMoniker> = None;
    unsafe { enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut monikers, 0) }
        .map_err(direct_show)?;
    let monikers =
        monikers.ok_or_else(|| DeviceEnumerationError::DeviceNotFound(device_id.to_owned()))?;

    loop {
        let mut next = [None];
        let mut fetched = 0;
        let result = unsafe { monikers.Next(&mut next, Some(&mut fetched)) };
        if result == S_FALSE || fetched == 0 {
            break;
        }
        if result != S_OK {
            return Err(direct_show(result.into()));
        }
        let Some(moniker) = next[0].take() else {
            continue;
        };
        if unsafe { friendly_name(&moniker) }.is_some_and(|name| name == device_id) {
            return Ok(moniker);
        }
    }

    Err(DeviceEnumerationError::DeviceNotFound(device_id.to_owned()))
}

unsafe fn friendly_name(moniker: &IMoniker) -> Option<String> {
    let property_bag: IPropertyBag = unsafe { moniker.BindToStorage(None, None) }.ok()?;
    let mut value = VARIANT::default();
    if unsafe { property_bag.Read(w!("FriendlyName"), &mut value, None) }.is_err() {
        return None;
    }
    // SAFETY: IPropertyBag returned a valid VARIANT. A BSTR is borrowed only until VariantClear.
    let name = unsafe {
        let variant = &value.Anonymous.Anonymous;
        if variant.vt != VT_BSTR {
            None
        } else {
            let value = &variant.Anonymous.bstrVal;
            let value = &*(ptr::from_ref(value).cast::<BSTR>());
            Some(value.to_string())
        }
    };
    let _ = unsafe { VariantClear(&mut value) };
    name
}

struct OwnedMediaType(*mut AM_MEDIA_TYPE);

impl OwnedMediaType {
    const fn new(value: *mut AM_MEDIA_TYPE) -> Self {
        Self(value)
    }

    unsafe fn dimensions(&self) -> Option<(u32, u32)> {
        let media_type = unsafe { self.0.as_ref() }?;
        if media_type.pbFormat.is_null() {
            return None;
        }
        let (width, height) = if media_type.formattype == FORMAT_VideoInfo
            && usize::try_from(media_type.cbFormat).ok()? >= mem::size_of::<VIDEOINFOHEADER>()
        {
            // SAFETY: The format GUID and byte length establish a complete VIDEOINFOHEADER.
            let header = unsafe { &*media_type.pbFormat.cast::<VIDEOINFOHEADER>() };
            (header.bmiHeader.biWidth, header.bmiHeader.biHeight)
        } else if media_type.formattype == FORMAT_VideoInfo2
            && usize::try_from(media_type.cbFormat).ok()? >= mem::size_of::<VIDEOINFOHEADER2>()
        {
            // SAFETY: The format GUID and byte length establish a complete VIDEOINFOHEADER2.
            let header = unsafe { &*media_type.pbFormat.cast::<VIDEOINFOHEADER2>() };
            (header.bmiHeader.biWidth, header.bmiHeader.biHeight)
        } else {
            return None;
        };
        let width = width.unsigned_abs();
        let height = height.unsigned_abs();
        (width > 0 && height > 0).then_some((width, height))
    }
}

impl Drop for OwnedMediaType {
    fn drop(&mut self) {
        // SAFETY: GetStreamCaps allocated both structures with COM allocation rules. The pUnk
        // field is an owned COM reference and must be released before freeing the outer block.
        unsafe {
            let Some(media_type) = self.0.as_mut() else {
                return;
            };
            if !media_type.pbFormat.is_null() {
                CoTaskMemFree(Some(media_type.pbFormat.cast_const().cast::<c_void>()));
                media_type.pbFormat = ptr::null_mut();
            }
            mem::ManuallyDrop::drop(&mut media_type.pUnk);
            CoTaskMemFree(Some(self.0.cast_const().cast::<c_void>()));
        }
    }
}

struct ComApartment {
    should_uninitialize: bool,
}

impl ComApartment {
    unsafe fn initialize() -> Result<Self, DeviceEnumerationError> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                should_uninitialize: false,
            });
        }
        match result.ok() {
            Ok(()) => Ok(Self {
                should_uninitialize: true,
            }),
            Err(error) => Err(direct_show(error)),
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninitialize {
            // SAFETY: This guard owns one successful CoInitializeEx call on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

fn direct_show(error: windows::core::Error) -> DeviceEnumerationError {
    DeviceEnumerationError::DirectShow(error.to_string())
}

#[cfg(test)]
mod tests {
    use windows::Win32::Media::DirectShow::VIDEO_STREAM_CONFIG_CAPS;

    use super::capture_frame_rate_ranges;
    use crate::{NativeFrameRateCapability, frame_rate_capabilities};

    #[test]
    fn frame_interval_range_should_preserve_fractional_fps() {
        let capabilities = VIDEO_STREAM_CONFIG_CAPS {
            MinFrameInterval: 333_667,
            MaxFrameInterval: 666_667,
            ..Default::default()
        };

        let values = frame_rate_capabilities(&capture_frame_rate_ranges(&capabilities));

        assert!(matches!(
            values.as_slice(),
            [NativeFrameRateCapability::Range { minimum, maximum }]
                if (*minimum - 15.0).abs() < 0.001 && (*maximum - 29.97).abs() < 0.001
        ));
    }
}
