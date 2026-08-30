#![cfg(target_os = "macos")]
#![expect(
    clippy::panic,
    reason = "native acceptance failures retain explicit fixture and CoreAudio context"
)]

use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use gblab_core::media::{AudioSinkStatus, GlobalMediaRuntime};

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("assets")
        .join(name)
}

fn start_runtime() -> GlobalMediaRuntime {
    match GlobalMediaRuntime::start() {
        Ok(runtime) => runtime,
        Err(error) => panic!("failed to start native media runtime: {error}"),
    }
}

fn wait_until(deadline: Instant, condition: impl Fn() -> bool) -> bool {
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
#[ignore = "requires a macOS host with a real CoreAudio output device"]
fn mp4_native_preview_acceptance() {
    for fixture in ["h264-aac.mp4", "h265-aac.mp4", "h264-unsupported-audio.mp4"] {
        let runtime = start_runtime();
        let handle = runtime.handle();
        assert!(handle.open_mp4(asset(fixture), true).is_ok());
        assert!(handle.attach_preview().is_ok());
        assert!(handle.play().is_ok());
        assert!(
            wait_until(Instant::now() + Duration::from_secs(5), || {
                handle
                    .status()
                    .audio_sink
                    .is_some_and(|sink| sink.played_samples > 0)
            }),
            "CoreAudio did not consume PCM for {fixture}: {:?}",
            handle.status()
        );
        let status = handle.status();
        assert_eq!(
            status.audio_sink.as_ref().map(|sink| sink.status),
            Some(AudioSinkStatus::Playing),
            "CoreAudio sink did not start for {fixture}: {:?}",
            status.audio_sink
        );
        assert!(status.last_error.is_none());
        assert!(
            status
                .audio_sink
                .as_ref()
                .is_some_and(|sink| sink.last_error.is_none())
        );
        assert!(runtime.shutdown().is_ok());
    }

    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-noaudio.mp4"), false).is_ok());
    assert!(handle.attach_preview().is_ok());
    assert!(handle.play().is_ok());
    assert!(wait_until(Instant::now() + Duration::from_secs(2), || {
        handle.status().decoded_frames > 0
    }));
    assert!(handle.status().audio.is_none());
    assert!(handle.status().last_error.is_none());
    assert!(runtime.shutdown().is_ok());

    let runtime = start_runtime();
    let handle = runtime.handle();
    assert!(handle.open_mp4(asset("h264-aac.mp4"), true).is_ok());
    assert!(handle.attach_preview().is_ok());
    assert!(handle.play().is_ok());
    assert!(handle.set_audio_control(true, 0.2).is_ok());
    assert!(handle.set_audio_control(false, 0.75).is_ok());
    for rate in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
        assert!(handle.pause().is_ok());
        let decoded_before = handle.status().metrics.audio_frames_decoded;
        assert!(handle.set_playback_rate(rate).is_ok());
        assert!(handle.play().is_ok());
        assert!(
            wait_until(Instant::now() + Duration::from_secs(3), || {
                handle.status().metrics.audio_frames_decoded > decoded_before
            }),
            "audio preview stalled at {rate}x"
        );
    }
    let status = handle.status();
    assert!(!status.muted);
    assert!((status.volume - 0.75).abs() < f64::EPSILON);
    assert!(status.last_error.is_none());
    assert!(status.audio_sink.is_some_and(|sink| {
        sink.status == AudioSinkStatus::Playing
            && sink.played_samples > 0
            && sink.last_error.is_none()
    }));
    assert!(runtime.shutdown().is_ok());
}
