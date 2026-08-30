//! Audited native `FFmpeg` FFI helpers used by the MP4 media pipeline.

#![deny(unsafe_op_in_unsafe_fn)]

use std::slice;

use rsmpeg::{
    avcodec::{AVCodecParameters, AVCodecParametersRef},
    avutil::{AVAudioFifo, AVFrame},
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
