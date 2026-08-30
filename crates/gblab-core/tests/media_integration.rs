use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gblab_core::media::{
    AudioCodec, BackpressurePolicy, EncodedMediaCodec, GlobalMediaRuntime, MediaConsumerKind,
    MediaSourceStatus, MediaTrackKind, VideoCodec, probe_mp4,
};
use tokio::sync::mpsc::error::TryRecvError;

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("assets")
        .join(name)
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
            Some(AudioCodec::Other),
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
        let runtime = GlobalMediaRuntime::start();
        let handle = runtime.handle();
        let opened = handle.open_mp4(asset(name), false);
        assert!(opened.is_ok(), "open failed for {name}: {opened:?}");
        let subscription = handle.subscribe(
            MediaConsumerKind::Recorder,
            32,
            BackpressurePolicy::Disconnect,
        );
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
fn preview_detach_should_keep_live_consumer_running() {
    let runtime = GlobalMediaRuntime::start();
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
    let runtime = GlobalMediaRuntime::start();
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
    let runtime = GlobalMediaRuntime::start();
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
    let runtime = GlobalMediaRuntime::start();
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
    let runtime = GlobalMediaRuntime::start();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), true).is_ok());
    let subscription = handle.subscribe(MediaConsumerKind::Live, 8, BackpressurePolicy::Disconnect);
    assert!(subscription.is_ok());

    assert!(handle.pause().is_err());
    assert!(handle.seek(0.1).is_err());
    assert!(handle.set_playback_rate(2.0).is_err());
    assert!(handle.close().is_err());
    assert!(handle.open_mp4(asset("h265-noaudio.mp4"), true).is_err());
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn unsupported_audio_should_not_block_mp4_video_preview() {
    let runtime = GlobalMediaRuntime::start();
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
        Some(AudioCodec::Other)
    );
    assert!(runtime.shutdown().is_ok());
}
