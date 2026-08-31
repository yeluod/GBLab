#![expect(
    clippy::panic,
    reason = "integration-test setup failures retain explicit fixture and runtime context"
)]

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gblab_core::media::{
    AudioCodec, BackpressurePolicy, CodecConfigurationFormat, EncodedMediaCodec,
    GlobalMediaRuntime, MediaConsumerKind, MediaSourceStatus, MediaTrackKind, VideoCodec,
    probe_mp4,
};
use tokio::sync::mpsc::error::TryRecvError;

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("assets")
        .join(name)
}

fn start_runtime() -> GlobalMediaRuntime {
    match GlobalMediaRuntime::start() {
        Ok(runtime) => runtime,
        Err(error) => panic!("failed to start media runtime: {error}"),
    }
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<usize> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(usize::from(u16::from_be_bytes([bytes[0], bytes[1]])))
}

fn skip_length_prefixed_units(data: &[u8], cursor: &mut usize, count: usize) -> bool {
    for _ in 0..count {
        let Some(length) = read_be_u16(data, *cursor) else {
            return false;
        };
        let Some(payload_start) = cursor.checked_add(2) else {
            return false;
        };
        let Some(next) = payload_start.checked_add(length) else {
            return false;
        };
        if length == 0 || next > data.len() {
            return false;
        }
        *cursor = next;
    }
    true
}

fn avcc_contains_sps_and_pps(data: &[u8]) -> bool {
    if data.first() != Some(&1) || data.len() < 7 {
        return false;
    }
    let sps_count = usize::from(data[5] & 0x1f);
    let mut cursor = 6;
    if sps_count == 0 || !skip_length_prefixed_units(data, &mut cursor, sps_count) {
        return false;
    }
    let Some(&pps_count) = data.get(cursor) else {
        return false;
    };
    cursor = cursor.saturating_add(1);
    pps_count > 0 && skip_length_prefixed_units(data, &mut cursor, usize::from(pps_count))
}

fn hvcc_contains_vps_sps_and_pps(data: &[u8]) -> bool {
    if data.first() != Some(&1) || data.len() < 23 {
        return false;
    }
    let array_count = usize::from(data[22]);
    let mut cursor = 23;
    let mut found = [false; 3];
    for _ in 0..array_count {
        let Some(&header) = data.get(cursor) else {
            return false;
        };
        let Some(unit_count) = read_be_u16(data, cursor.saturating_add(1)) else {
            return false;
        };
        cursor = cursor.saturating_add(3);
        match header & 0x3f {
            32 => found[0] = unit_count > 0,
            33 => found[1] = unit_count > 0,
            34 => found[2] = unit_count > 0,
            _ => {}
        }
        if !skip_length_prefixed_units(data, &mut cursor, unit_count) {
            return false;
        }
    }
    found.into_iter().all(|value| value)
}

#[test]
fn probe_should_cover_supported_video_and_optional_audio_variants() {
    let cases = [
        ("h264-aac.mp4", VideoCodec::H264, Some(AudioCodec::Aac)),
        ("h264-noaudio.mp4", VideoCodec::H264, None),
        ("h265-aac.mp4", VideoCodec::H265, Some(AudioCodec::Aac)),
        ("h265-noaudio.mp4", VideoCodec::H265, None),
        (
            "h264-unsupported-audio.mp4",
            VideoCodec::H264,
            Some(AudioCodec::Mp3),
        ),
    ];

    for (name, video_codec, audio_codec) in cases {
        let result = probe_mp4(&asset(name));
        assert!(result.is_ok(), "probe failed for {name}: {result:?}");
        if let Ok(probe) = result {
            assert_eq!(probe.video.codec, video_codec);
            assert_eq!(probe.audio.map(|audio| audio.codec), audio_codec);
        }
    }
}

#[test]
fn encoded_video_should_contain_real_annex_b_bytes() {
    for (name, codec) in [
        ("h264-noaudio.mp4", VideoCodec::H264),
        ("h265-noaudio.mp4", VideoCodec::H265),
    ] {
        let runtime = start_runtime();
        let handle = runtime.handle();
        let opened = handle.open_mp4(asset(name), false);
        assert!(opened.is_ok(), "open failed for {name}: {opened:?}");
        let subscription =
            handle.subscribe(MediaConsumerKind::Live, 32, BackpressurePolicy::Disconnect);
        assert!(subscription.is_ok());
        let Ok(mut subscription) = subscription else {
            let _ = runtime.shutdown();
            continue;
        };
        assert!(handle.play().is_ok());
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut found = None;
        while Instant::now() < deadline {
            match subscription.try_recv() {
                Ok(packet) if packet.track == MediaTrackKind::Video => {
                    found = Some(packet);
                    break;
                }
                Ok(_) | Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        assert!(runtime.shutdown().is_ok());
        assert!(found.is_some(), "no encoded video packet for {name}");
        if let Some(packet) = found {
            assert_eq!(packet.codec, EncodedMediaCodec::Video(codec));
            assert!(!packet.data.is_empty());
            assert!(packet.codec_configuration.is_some());
            assert!(
                packet.data.starts_with(&[0, 0, 0, 1]) || packet.data.starts_with(&[0, 0, 1]),
                "packet is not Annex-B for {name}"
            );
        }
    }
}

#[test]
fn aac_priming_timestamp_should_define_a_non_negative_shared_epoch() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-aac.mp4"), false).is_ok());
    let subscription =
        handle.subscribe(MediaConsumerKind::Live, 64, BackpressurePolicy::Disconnect);
    let Ok(mut subscription) = subscription else {
        let _ = runtime.shutdown();
        panic!("failed to subscribe to encoded MP4 packets");
    };
    assert!(handle.play().is_ok());

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut first_video = None;
    let mut first_audio = None;
    while Instant::now() < deadline && (first_video.is_none() || first_audio.is_none()) {
        match subscription.try_recv() {
            Ok(packet) => {
                let timestamp = packet.dts.or(packet.pts);
                assert!(
                    packet.pts.is_none_or(|pts| pts >= 0),
                    "negative pts packet: {packet:?}"
                );
                assert!(
                    packet.dts.is_none_or(|dts| dts >= 0),
                    "negative dts packet: {packet:?}"
                );
                match packet.track {
                    MediaTrackKind::Video => first_video.get_or_insert(timestamp),
                    MediaTrackKind::Audio => first_audio.get_or_insert(timestamp),
                };
            }
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(TryRecvError::Disconnected) => break,
        }
    }
    assert!(runtime.shutdown().is_ok());

    assert_eq!(first_audio.flatten(), Some(0));
    assert_eq!(first_video.flatten(), Some(1_920));
}

#[test]
fn h265_b_frames_and_aac_priming_should_share_a_non_negative_epoch() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h265-aac.mp4"), false).is_ok());
    let subscription =
        handle.subscribe(MediaConsumerKind::Live, 128, BackpressurePolicy::Disconnect);
    let Ok(mut subscription) = subscription else {
        let _ = runtime.shutdown();
        panic!("failed to subscribe to H.265 fixture");
    };
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut packet_count = 0;
    while Instant::now() < deadline && packet_count < 20 {
        match subscription.try_recv() {
            Ok(packet) => {
                assert!(
                    packet.pts.is_none_or(|pts| pts >= 0),
                    "negative pts packet: {packet:?}"
                );
                assert!(
                    packet.dts.is_none_or(|dts| dts >= 0),
                    "negative dts packet: {packet:?}"
                );
                packet_count += 1;
            }
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(TryRecvError::Disconnected) => break,
        }
    }
    assert!(runtime.shutdown().is_ok());
    assert!(packet_count >= 10);
}

#[test]
fn late_subscriber_should_receive_real_mp4_track_descriptors() {
    for (name, video_format) in [
        ("h264-aac.mp4", CodecConfigurationFormat::H264Avcc),
        ("h265-aac.mp4", CodecConfigurationFormat::H265Hvcc),
    ] {
        let runtime = start_runtime();
        let handle = runtime.handle();
        assert!(handle.open_mp4(asset(name), false).is_ok());
        let subscription =
            handle.subscribe(MediaConsumerKind::Live, 128, BackpressurePolicy::Disconnect);
        let Ok(mut first) = subscription else {
            let _ = runtime.shutdown();
            panic!("failed to subscribe to {name}");
        };
        assert!(handle.play().is_ok());
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut tracks = Vec::new();
        while Instant::now() < deadline && tracks.len() < 2 {
            match first.try_recv() {
                Ok(packet) if !tracks.contains(&packet.track) => tracks.push(packet.track),
                Ok(_) | Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        let late = handle.subscribe(MediaConsumerKind::Live, 8, BackpressurePolicy::Disconnect);
        let Ok(late) = late else {
            let _ = runtime.shutdown();
            panic!("late subscription failed for {name}");
        };
        let video = late
            .descriptors
            .iter()
            .find(|descriptor| descriptor.track == MediaTrackKind::Video);
        let audio = late
            .descriptors
            .iter()
            .find(|descriptor| descriptor.track == MediaTrackKind::Audio);
        assert_eq!(
            video.and_then(|value| value.configuration_format),
            Some(video_format)
        );
        let video_configuration = video.and_then(|value| value.configuration.as_deref());
        assert!(video_configuration.is_some_and(|value| match video_format {
            CodecConfigurationFormat::H264Avcc => avcc_contains_sps_and_pps(value),
            CodecConfigurationFormat::H265Hvcc => hvcc_contains_vps_sps_and_pps(value),
            _ => false,
        }));
        assert!(video.and_then(|value| value.width).is_some());
        assert!(video.and_then(|value| value.height).is_some());
        assert!(video.and_then(|value| value.frame_rate).is_some());
        assert_eq!(
            audio.and_then(|value| value.configuration_format),
            Some(CodecConfigurationFormat::AacAsc)
        );
        assert!(
            audio
                .and_then(|value| value.configuration.as_ref())
                .is_some_and(|value| value.len() >= 2)
        );
        assert_eq!(audio.and_then(|value| value.sample_rate), Some(48_000));
        assert_eq!(audio.and_then(|value| value.channels), Some(1));
        assert!(runtime.shutdown().is_ok());
    }
}

#[test]
fn preview_detach_should_keep_live_consumer_running() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), true).is_ok());
    let subscription = handle.subscribe(MediaConsumerKind::Live, 8, BackpressurePolicy::Disconnect);
    assert!(subscription.is_ok());
    let Ok(mut live) = subscription else {
        let _ = runtime.shutdown();
        return;
    };
    assert!(handle.attach_preview().is_ok());
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && live.try_recv().is_err() {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(handle.detach_preview().is_ok());
    assert_eq!(handle.status().source_status, MediaSourceStatus::Playing);
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn seek_and_eof_should_update_runtime_state() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-aac.mp4"), false).is_ok());
    let seek = handle.seek(0.3);
    assert!(seek.is_ok());
    if let Ok(status) = seek {
        assert!(status.position_seconds >= 0.2);
    }
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && handle.status().source_status != MediaSourceStatus::Stopped {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(handle.status().source_status, MediaSourceStatus::Stopped);
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn looping_output_timestamps_should_remain_monotonic() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), true).is_ok());
    let subscription =
        handle.subscribe(MediaConsumerKind::Live, 64, BackpressurePolicy::Disconnect);
    assert!(subscription.is_ok());
    let Ok(mut subscription) = subscription else {
        let _ = runtime.shutdown();
        return;
    };
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut timestamps = Vec::new();
    while Instant::now() < deadline && timestamps.len() < 12 {
        match subscription.try_recv() {
            Ok(packet) if packet.track == MediaTrackKind::Video => {
                if let Some(pts) = packet.pts {
                    timestamps.push(pts);
                }
            }
            Ok(_) | Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(TryRecvError::Disconnected) => break,
        }
    }
    assert!(runtime.shutdown().is_ok());
    assert!(
        timestamps.len() >= 8,
        "insufficient packets: {timestamps:?}"
    );
    assert!(timestamps.windows(2).all(|window| window[0] <= window[1]));
    assert!(timestamps.last() > timestamps.first());
}

#[test]
fn third_loop_pause_resume_should_not_wait_for_the_accumulated_session_time() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), true).is_ok());
    let subscription =
        handle.subscribe(MediaConsumerKind::Live, 512, BackpressurePolicy::Disconnect);
    assert!(subscription.is_ok());
    let Ok(mut subscription) = subscription else {
        let _ = runtime.shutdown();
        return;
    };
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut latest = 0;
    while Instant::now() < deadline && latest < 180_000 {
        match subscription.try_recv() {
            Ok(packet) => latest = latest.max(packet.dts.or(packet.pts).unwrap_or_default()),
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(TryRecvError::Disconnected) => break,
        }
    }
    assert!(latest >= 180_000, "did not reach the third loop: {latest}");
    assert!(handle.attach_preview().is_ok());
    assert!(handle.unsubscribe(subscription.id).is_ok());
    assert!(handle.pause().is_ok());
    thread::sleep(Duration::from_millis(150));
    let decoded_before = handle.status().decoded_frames;
    let resumed_at = Instant::now();
    assert!(handle.play().is_ok());
    while Instant::now().duration_since(resumed_at) < Duration::from_millis(600)
        && handle.status().decoded_frames == decoded_before
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(handle.status().decoded_frames > decoded_before);
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn active_live_consumer_should_reject_source_mutations() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), true).is_ok());
    let subscription = handle.subscribe(MediaConsumerKind::Live, 8, BackpressurePolicy::Disconnect);
    assert!(subscription.is_ok());

    assert!(handle.pause().is_err());
    assert!(handle.seek(0.1).is_err());
    assert!(handle.set_playback_rate(2.0).is_err());
    assert!(handle.stop().is_err());
    assert!(handle.reset().is_err());
    assert!(handle.close().is_err());
    assert!(handle.open_mp4(asset("h265-noaudio.mp4"), true).is_err());
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn playback_rate_changes_should_keep_audio_preview_decoding() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-aac.mp4"), true).is_ok());
    assert!(handle.attach_preview().is_ok());
    assert!(handle.play().is_ok());

    for rate in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
        assert!(handle.pause().is_ok());
        assert_eq!(
            handle
                .status()
                .audio_sink
                .as_ref()
                .map_or(0, |sink| sink.queued_samples),
            0,
            "pause must flush queued PCM"
        );
        let before = handle.status().metrics.audio_frames_decoded;
        assert!(handle.set_playback_rate(rate).is_ok());
        assert_eq!(
            handle
                .status()
                .audio_sink
                .as_ref()
                .map_or(0, |sink| sink.queued_samples),
            0,
            "rate change must not retain old-generation PCM"
        );
        assert!(handle.play().is_ok());
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && handle.status().metrics.audio_frames_decoded == before {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            handle.status().metrics.audio_frames_decoded > before,
            "audio preview stalled after changing rate to {rate}"
        );
    }
    assert!(handle.pause().is_ok());
    assert!(handle.seek(0.1).is_ok());
    assert_eq!(
        handle
            .status()
            .audio_sink
            .as_ref()
            .map_or(0, |sink| sink.queued_samples),
        0,
        "seek must flush queued PCM"
    );
    assert!(handle.play().is_ok());
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn non_aac_audio_should_not_block_mp4_video_preview() {
    let runtime = start_runtime();
    let handle = runtime.handle();
    let opened = handle.open_mp4(asset("h264-unsupported-audio.mp4"), false);
    assert!(opened.is_ok(), "open failed: {opened:?}");
    assert!(handle.attach_preview().is_ok());
    assert!(handle.play().is_ok());
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && handle.status().decoded_frames == 0 {
        thread::sleep(Duration::from_millis(5));
    }
    let status = handle.status();
    assert!(status.decoded_frames > 0);
    assert_eq!(
        status.audio.as_ref().map(|audio| audio.codec),
        Some(AudioCodec::Mp3)
    );
    assert!(runtime.shutdown().is_ok());
}
