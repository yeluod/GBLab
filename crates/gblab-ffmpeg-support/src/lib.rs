//! Audited native `FFmpeg` FFI helpers used by the MP4 media pipeline.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    ffi::{CStr, c_void},
    ptr::{self, NonNull},
    slice,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use rsmpeg::{
    avcodec::{AVCodecParameters, AVCodecParametersRef},
    avformat::{AVFormatContextInput, AVInputFormatRef},
    avutil::{AVAudioFifo, AVDictionary, AVFrame},
    error::RsmpegError,
    ffi,
};

/// Copies packed interleaved `f32` samples from an FFmpeg frame.
#[must_use]
pub fn copy_interleaved_f32(frame: &AVFrame) -> Option<Vec<f32>> {
    if frame.format != ffi::AV_SAMPLE_FMT_FLT || frame.nb_samples <= 0 {
        return None;
    }
    let channels = usize::try_from(frame.ch_layout().nb_channels).ok()?;
    let samples = usize::try_from(frame.nb_samples)
        .ok()?
        .checked_mul(channels)?;
    let pointer = frame.data[0].cast::<f32>();
    if pointer.is_null() {
        return None;
    }
    // SAFETY: FFmpeg owns a packed FLT buffer for the frame lifetime. Samples are copied now.
    Some(unsafe { slice::from_raw_parts(pointer.cast_const(), samples) }.to_vec())
}

/// Copies packed `f32` samples into an allocated FFmpeg audio frame.
pub fn write_interleaved_f32(frame: &mut AVFrame, samples: &[f32]) -> bool {
    if frame.format != ffi::AV_SAMPLE_FMT_FLT || frame.nb_samples <= 0 {
        return false;
    }
    let Ok(sample_count) = usize::try_from(frame.nb_samples) else {
        return false;
    };
    let Ok(channels) = usize::try_from(frame.ch_layout().nb_channels) else {
        return false;
    };
    let Some(expected) = sample_count.checked_mul(channels) else {
        return false;
    };
    if samples.len() != expected {
        return false;
    }
    let destination = frame.data[0].cast::<f32>();
    if destination.is_null() {
        return false;
    }
    // SAFETY: alloc_buffer allocated at least expected packed f32 values.
    unsafe { std::ptr::copy_nonoverlapping(samples.as_ptr(), destination, expected) };
    true
}

/// Writes all samples from an allocated frame into a matching audio FIFO.
pub fn audio_fifo_write(fifo: &mut AVAudioFifo, frame: &AVFrame) -> Result<(), String> {
    // SAFETY: The frame owns valid sample planes and matches the FIFO format.
    unsafe { fifo.write(frame.data.as_ptr(), frame.nb_samples) }.map_err(|error| error.to_string())
}

/// Reads exactly one allocated frame from a matching audio FIFO.
pub fn audio_fifo_read(fifo: &mut AVAudioFifo, frame: &mut AVFrame) -> Result<(), String> {
    // SAFETY: alloc_buffer created writable sample planes for nb_samples.
    let read = unsafe { fifo.read(frame.data.as_ptr(), frame.nb_samples) }
        .map_err(|error| error.to_string())?;
    if read == frame.nb_samples {
        Ok(())
    } else {
        Err(format!(
            "audio FIFO short read: expected {}, got {read}",
            frame.nb_samples
        ))
    }
}

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
        // SAFETY: The guard drops before the owning input context and exclusively owns opaque.
        unsafe {
            let context = self.context.as_mut();
            context.interrupt_callback.callback = None;
            context.interrupt_callback.opaque = ptr::null_mut();
            drop(Box::from_raw(self.opaque.as_ptr()));
        }
    }
}

/// Opens an FFmpeg input with the interrupt callback active during open and probing.
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
    // SAFETY: All pointers remain valid for the complete avformat_open_input call.
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
            // SAFETY: A non-null context after failed open remains owned here.
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
    // SAFETY: The input context and callback remain valid during stream probing.
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
    // SAFETY: Both pointers remain valid until InputInterruptGuard clears and frees opaque.
    unsafe {
        let native = context_ptr.as_ptr();
        (*native).interrupt_callback.callback = Some(media_interrupt_callback);
        (*native).interrupt_callback.opaque = opaque.as_ptr().cast::<c_void>();
    }
    opaque
}

fn drop_opaque(opaque: NonNull<Arc<AtomicU8>>) {
    // SAFETY: Pointer was allocated by attach_raw_interrupt and is released exactly once.
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
    // SAFETY: open_input_with_interrupt stores an Arc allocation at this address and the guard
    // keeps it alive until after the callback is detached.
    let flag = unsafe { &*opaque.cast::<Arc<AtomicU8>>() };
    i32::from(flag.load(Ordering::Acquire) != 0)
}

/// Copies an FFmpeg packet payload into Rust-owned memory.
#[must_use]
pub fn copy_packet_data(packet: &rsmpeg::avcodec::AVPacket) -> Vec<u8> {
    let Ok(size) = usize::try_from(packet.size) else {
        return Vec::new();
    };
    if size == 0 || packet.data.is_null() {
        return Vec::new();
    }
    // SAFETY: FFmpeg guarantees positive-size packets expose readable bytes for packet lifetime.
    unsafe { slice::from_raw_parts(packet.data.cast_const(), size) }.to_vec()
}

/// Copies codec initialization data from FFmpeg-owned parameters.
#[must_use]
pub fn copy_codec_extradata(parameters: &AVCodecParametersRef<'_>) -> Option<Vec<u8>> {
    copy_extradata(parameters.extradata, parameters.extradata_size)
}

/// Copies extradata from an owned codec-parameter allocation.
#[must_use]
pub fn copy_owned_codec_extradata(parameters: &AVCodecParameters) -> Option<Vec<u8>> {
    copy_extradata(parameters.extradata, parameters.extradata_size)
}

fn copy_extradata(pointer: *mut u8, size: i32) -> Option<Vec<u8>> {
    let size = usize::try_from(size).ok()?;
    if size == 0 || pointer.is_null() {
        return None;
    }
    // SAFETY: FFmpeg owns extradata for the parameter lifetime and advertises its readable length.
    Some(unsafe { slice::from_raw_parts(pointer.cast_const(), size) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::InterruptReason;

    #[test]
    fn interrupt_reason_wire_values_round_trip() {
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
