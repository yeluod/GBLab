//! Dedicated owner thread for the single global media source.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::audio_preview::AudioPreviewSink;
use super::{
    BackpressurePolicy, CameraCaptureSettings, CameraMediaSource, MediaClock, MediaConsumerKind,
    MediaError, MediaResult, MediaRuntimeStatus, MediaSourceKind, MediaSourceStatus,
    MediaStreamHub, MediaSubscription, MediaVideoFrame, Mp4MediaSource,
    types::{MediaSource, MediaSourceSession},
};
use gblab_ffmpeg_device::InterruptReason;

const COMMAND_CAPACITY: usize = 32;
const PREVIEW_CAPACITY: usize = 2;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

type StatusReply = mpsc::SyncSender<MediaResult<MediaRuntimeStatus>>;

enum MediaCommand {
    OpenMp4 {
        path: PathBuf,
        looping: bool,
        reply: StatusReply,
    },
    OpenCamera {
        settings: CameraCaptureSettings,
        reply: StatusReply,
    },
    Play(StatusReply),
    Pause(StatusReply),
    Stop(StatusReply),
    Close(StatusReply),
    Reset(StatusReply),
    Seek {
        position_seconds: f64,
        reply: StatusReply,
    },
    SetPlaybackRate {
        rate: f64,
        reply: StatusReply,
    },
    SetAudioControl {
        muted: bool,
        volume: f64,
        reply: StatusReply,
    },
    SetAudioMonitoring {
        enabled: bool,
        reply: StatusReply,
    },
    StepFrame(mpsc::SyncSender<MediaResult<Option<MediaVideoFrame>>>),
    AttachPreview(StatusReply),
    DetachPreview(StatusReply),
    Subscribe {
        kind: MediaConsumerKind,
        capacity: usize,
        policy: BackpressurePolicy,
        reply: mpsc::SyncSender<MediaResult<MediaSubscription>>,
    },
    Unsubscribe(u64),
    Shutdown(mpsc::SyncSender<()>),
}

/// Cloneable control handle which never exposes `FFmpeg` contexts.
#[derive(Clone)]
pub struct GlobalMediaHandle {
    commands: mpsc::SyncSender<MediaCommand>,
    status: Arc<RwLock<MediaRuntimeStatus>>,
    preview: Arc<Mutex<mpsc::Receiver<MediaVideoFrame>>>,
    interrupt: Arc<AtomicU8>,
}

impl GlobalMediaHandle {
    /// Opens the single MP4 source on the owner worker.
    pub fn open_mp4(&self, path: PathBuf, looping: bool) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt
            .store(InterruptReason::Reconfigure as u8, Ordering::Release);
        let result = self.request_status(|reply| MediaCommand::OpenMp4 {
            path,
            looping,
            reply,
        });
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt
                .store(InterruptReason::None as u8, Ordering::Release);
        }
        result
    }

    /// Opens the single camera source on the owner worker.
    pub fn open_camera(&self, settings: CameraCaptureSettings) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt
            .store(InterruptReason::Reconfigure as u8, Ordering::Release);
        let result = self.request_status(|reply| MediaCommand::OpenCamera { settings, reply });
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt
                .store(InterruptReason::None as u8, Ordering::Release);
        }
        result
    }

    /// Starts or resumes the source.
    pub fn play(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::Play)
    }

    /// Attaches the UI preview consumer without implicitly replacing source state.
    pub fn attach_preview(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::AttachPreview)
    }

    /// Detaches the UI preview consumer; source shutdown is demand-driven.
    pub fn detach_preview(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(
            InterruptReason::PreviewDetach,
            MediaCommand::DetachPreview,
        )
    }

    /// Pauses source production.
    pub fn pause(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(InterruptReason::Pause, MediaCommand::Pause)
    }

    /// Stops production and releases live capture devices.
    pub fn stop(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(InterruptReason::Stop, MediaCommand::Stop)
    }

    /// Closes the current source and releases every owned codec/input context.
    pub fn close(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(InterruptReason::Close, MediaCommand::Close)
    }

    /// Resets an MP4 source to its beginning.
    pub fn reset(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::Reset)
    }

    /// Seeks the independent local source timeline.
    pub fn seek(&self, position_seconds: f64) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(|reply| MediaCommand::Seek {
            position_seconds,
            reply,
        })
    }

    /// Changes local preview/source playback pacing.
    pub fn set_playback_rate(&self, rate: f64) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(|reply| MediaCommand::SetPlaybackRate { rate, reply })
    }

    /// Updates audio presentation controls without coupling them to the encoded stream.
    pub fn set_audio_control(&self, muted: bool, volume: f64) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(|reply| MediaCommand::SetAudioControl {
            muted,
            volume,
            reply,
        })
    }

    /// Enables or disables local microphone monitoring for a camera source.
    pub fn set_audio_monitoring(&self, enabled: bool) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(|reply| MediaCommand::SetAudioMonitoring { enabled, reply })
    }

    /// Reads one frame while paused.
    pub fn step_frame(&self) -> MediaResult<Option<MediaVideoFrame>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(MediaCommand::StepFrame(reply))?;
        receiver
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| MediaError::RuntimeUnavailable("媒体 worker 单帧响应超时".to_owned()))?
    }

    /// Returns the latest runtime snapshot without waiting for source I/O.
    #[must_use]
    pub fn status(&self) -> MediaRuntimeStatus {
        self.status.read().map_or_else(
            |_| MediaRuntimeStatus::unconfigured(),
            |status| status.clone(),
        )
    }

    /// Takes the next bounded preview frame if available.
    pub fn try_preview_frame(&self) -> MediaResult<Option<MediaVideoFrame>> {
        let preview = self
            .preview
            .lock()
            .map_err(|_| MediaError::RuntimeUnavailable("预览队列不可用".to_owned()))?;
        match preview.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(MediaError::RuntimeUnavailable(
                "媒体 worker 已停止".to_owned(),
            )),
        }
    }

    /// Adds a bounded encoded-stream consumer.
    pub fn subscribe(
        &self,
        kind: MediaConsumerKind,
        capacity: usize,
        policy: BackpressurePolicy,
    ) -> MediaResult<MediaSubscription> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(MediaCommand::Subscribe {
            kind,
            capacity,
            policy,
            reply,
        })?;
        receiver
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| MediaError::RuntimeUnavailable("媒体订阅响应超时".to_owned()))?
    }

    /// Removes an encoded-stream consumer.
    pub fn unsubscribe(&self, id: u64) -> MediaResult<()> {
        self.send(MediaCommand::Unsubscribe(id))
    }

    fn request_status(
        &self,
        build: impl FnOnce(StatusReply) -> MediaCommand,
    ) -> MediaResult<MediaRuntimeStatus> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(build(reply))?;
        match receiver.recv_timeout(COMMAND_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Cancel blocking capture/open work before reporting timeout. The worker checks
                // this interrupt flag in the FFmpeg input callback and will not keep a stale
                // device read alive after the caller has abandoned the request.
                self.interrupt
                    .store(InterruptReason::Timeout as u8, Ordering::Release);
                Err(MediaError::CommandTimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(MediaError::RuntimeUnavailable(
                "媒体 worker 已停止".to_owned(),
            )),
        }
    }

    fn request_interrupting_status(
        &self,
        reason: InterruptReason,
        build: impl FnOnce(StatusReply) -> MediaCommand,
    ) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt.store(reason as u8, Ordering::Release);
        let result = self.request_status(build);
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt
                .store(InterruptReason::None as u8, Ordering::Release);
        }
        result
    }

    fn send(&self, command: MediaCommand) -> MediaResult<()> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    MediaError::RuntimeUnavailable("媒体命令队列已满".to_owned())
                }
                mpsc::TrySendError::Disconnected(_) => {
                    MediaError::RuntimeUnavailable("媒体 worker 已停止".to_owned())
                }
            })
    }
}

/// Process-wide media runtime owning the dedicated source thread.
pub struct GlobalMediaRuntime {
    handle: GlobalMediaHandle,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl GlobalMediaRuntime {
    /// Starts the dedicated media owner thread.
    #[must_use]
    pub fn start() -> Self {
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (preview_sender, preview_receiver) = mpsc::sync_channel(PREVIEW_CAPACITY);
        let status = Arc::new(RwLock::new(MediaRuntimeStatus::unconfigured()));
        let interrupt = Arc::new(AtomicU8::new(InterruptReason::None as u8));
        let worker_status = Arc::clone(&status);
        let worker_interrupt = Arc::clone(&interrupt);
        let worker = thread::Builder::new()
            .name("gblab-media-source".to_owned())
            .spawn(move || {
                MediaWorker::new(receiver, preview_sender, worker_status, worker_interrupt).run();
            })
            .ok();
        Self {
            handle: GlobalMediaHandle {
                commands,
                status,
                preview: Arc::new(Mutex::new(preview_receiver)),
                interrupt,
            },
            worker: Mutex::new(worker),
        }
    }

    /// Returns a cloneable command handle.
    #[must_use]
    pub fn handle(&self) -> GlobalMediaHandle {
        self.handle.clone()
    }

    /// Stops and joins the owner thread. Calling this more than once is harmless.
    pub fn shutdown(&self) -> MediaResult<()> {
        self.handle
            .interrupt
            .store(InterruptReason::Shutdown as u8, Ordering::Release);
        let (reply, receiver) = mpsc::sync_channel(1);
        if self
            .handle
            .commands
            .send(MediaCommand::Shutdown(reply))
            .is_ok()
        {
            receiver
                .recv_timeout(COMMAND_TIMEOUT)
                .map_err(|_| MediaError::RuntimeUnavailable("媒体 worker 停止超时".to_owned()))?;
        }
        let worker = self
            .worker
            .lock()
            .map_err(|_| MediaError::RuntimeUnavailable("媒体线程句柄不可用".to_owned()))?
            .take();
        if let Some(worker) = worker {
            worker
                .join()
                .map_err(|_| MediaError::RuntimeUnavailable("媒体 worker 异常退出".to_owned()))?;
        }
        Ok(())
    }
}

impl Drop for GlobalMediaRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl Default for GlobalMediaRuntime {
    fn default() -> Self {
        Self::start()
    }
}

struct MediaWorker {
    commands: mpsc::Receiver<MediaCommand>,
    preview: mpsc::SyncSender<MediaVideoFrame>,
    shared_status: Arc<RwLock<MediaRuntimeStatus>>,
    status: MediaRuntimeStatus,
    session: Option<MediaSourceSession>,
    hub: MediaStreamHub,
    clock: MediaClock,
    next_read_at: Instant,
    pacing_anchor: Option<Instant>,
    last_media_timestamp: Option<i64>,
    interrupt: Arc<AtomicU8>,
    preview_attached: bool,
    audio_sink: Option<AudioPreviewSink>,
}

impl MediaWorker {
    fn new(
        commands: mpsc::Receiver<MediaCommand>,
        preview: mpsc::SyncSender<MediaVideoFrame>,
        shared_status: Arc<RwLock<MediaRuntimeStatus>>,
        interrupt: Arc<AtomicU8>,
    ) -> Self {
        Self {
            commands,
            preview,
            shared_status,
            status: MediaRuntimeStatus::unconfigured(),
            session: None,
            hub: MediaStreamHub::new(),
            clock: MediaClock::new(),
            next_read_at: Instant::now(),
            pacing_anchor: None,
            last_media_timestamp: None,
            interrupt,
            preview_attached: false,
            audio_sink: None,
        }
    }

    fn run(mut self) {
        loop {
            if self.status.source_status != MediaSourceStatus::Playing {
                match self.commands.recv() {
                    Ok(command) => {
                        if self.handle_command(command) {
                            continue;
                        }
                        break;
                    }
                    Err(_) => break,
                }
            }

            let now = Instant::now();
            if now < self.next_read_at {
                match self
                    .commands
                    .recv_timeout(self.next_read_at.duration_since(now))
                {
                    Ok(command) => {
                        if self.handle_command(command) {
                            continue;
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            } else {
                while let Ok(command) = self.commands.try_recv() {
                    if !self.handle_command(command) {
                        return;
                    }
                }
            }

            if self.status.source_status != MediaSourceStatus::Playing {
                continue;
            }
            if let Err(error) = self.produce_once() {
                let interrupt = InterruptReason::from_u8(
                    self.interrupt
                        .swap(InterruptReason::None as u8, Ordering::AcqRel),
                );
                if interrupt != InterruptReason::None {
                    // An interrupt is the expected wake-up path for pause, stop,
                    // close, detach and reconfigure.  The queued command owns
                    // the final state transition; do not expose AVERROR_EXIT as
                    // a user-visible source failure.
                    continue;
                }
                self.status.source_status = MediaSourceStatus::Stopped;
                self.status.last_error = Some(format!("FatalSource: {error}"));
                self.publish_status();
            }
        }
        self.session = None;
    }

    fn produce_once(&mut self) -> MediaResult<()> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let mut output = session.read_source_output()?;
        let retry_after = output.retry_after;
        if output.looped {
            self.clock.begin_loop();
            self.hub.begin_timeline();
            if let Some(sink) = self.audio_sink.as_ref() {
                sink.clear();
            }
        }
        if output.end_of_stream {
            self.status.source_status = MediaSourceStatus::Stopped;
        }
        if let Some(mut packet) = output.packet.take() {
            self.clock.normalize(&mut packet);
            self.status.position_seconds = packet.position_seconds();
            self.schedule_next_read(packet.dts.or(packet.pts));
            let packet = Arc::new(packet);
            let report = self.hub.broadcast(&packet);
            if report.disconnected > 0 {
                let _ = self.reconcile_runtime_demand();
            }
        }
        for frame in output.preview_frames {
            self.status.position_seconds = frame.position_seconds;
            self.status.decoded_frames = self.status.decoded_frames.saturating_add(1);
            let _ = self.preview.try_send(frame);
        }
        self.status.metrics.merge(output.metrics);
        if let Some(error) = output.branch_errors.into_iter().last() {
            self.status.last_pipeline_error = Some(error);
        }
        if self.preview_attached
            && (self.status.source_kind == Some(MediaSourceKind::Mp4)
                || self.status.audio_monitoring)
            && let Some(sink) = self.audio_sink.as_ref()
        {
            for frame in output.audio_frames {
                sink.push(frame.samples);
            }
        }
        if let Some(delay) = retry_after {
            self.next_read_at = Instant::now() + delay;
        }
        self.publish_status();
        Ok(())
    }

    fn schedule_next_read(&mut self, timestamp: Option<i64>) {
        let Some(timestamp) = timestamp else {
            self.next_read_at = Instant::now();
            return;
        };
        if self
            .session
            .as_ref()
            .is_some_and(MediaSourceSession::is_live_capture)
        {
            // Live input pacing is owned by av_read_frame/device backend. Sleeping here would
            // halve the effective camera rate and make the driver queue grow under load.
            self.last_media_timestamp = Some(timestamp);
            self.next_read_at = Instant::now();
            return;
        }
        let anchor = *self.pacing_anchor.get_or_insert_with(Instant::now);
        let previous = self.last_media_timestamp.replace(timestamp);
        let rate = self.status.playback_rate.max(0.01);
        let media_seconds = MediaClock::timestamp_seconds(timestamp);
        let deadline = if previous.is_none() {
            anchor
        } else {
            pacing_deadline(anchor, media_seconds, rate)
        };
        self.next_read_at = deadline;
    }

    /// Returns true while the worker should keep running.
    #[expect(
        clippy::too_many_lines,
        reason = "The owner thread keeps serialized media commands in one explicit dispatcher"
    )]
    fn handle_command(&mut self, command: MediaCommand) -> bool {
        self.interrupt
            .store(InterruptReason::None as u8, Ordering::Release);
        match command {
            MediaCommand::OpenMp4 {
                path,
                looping,
                reply,
            } => {
                let result = self
                    .release_current_source()
                    .and_then(|()| Mp4MediaSource::new(path).open(looping))
                    .and_then(|session| self.replace_session(session, MediaSourceKind::Mp4));
                Self::reply_status(&reply, result);
            }
            MediaCommand::OpenCamera { settings, reply } => {
                let result = self
                    .release_current_source()
                    .and_then(|()| {
                        CameraMediaSource::new(settings, Arc::clone(&self.interrupt)).open(false)
                    })
                    .and_then(|session| self.replace_session(session, MediaSourceKind::Camera));
                Self::reply_status(&reply, result);
            }
            MediaCommand::Play(reply) => {
                let result = self.with_session(MediaSourceSession::play);
                if result.is_ok() {
                    self.status.source_status = MediaSourceStatus::Playing;
                    self.status.last_error = None;
                    self.next_read_at = Instant::now();
                    // Re-anchor against the current media position.  Resetting
                    // the anchor to `now` makes a resumed file wait from its
                    // original timestamp (for example ~10 seconds after a
                    // pause at 10s).
                    let position = self
                        .last_media_timestamp
                        .map(MediaClock::timestamp_seconds)
                        .unwrap_or_default()
                        .max(0.0);
                    self.pacing_anchor = Instant::now().checked_sub(Duration::from_secs_f64(
                        position / self.status.playback_rate.max(0.01),
                    ));
                    if let Some(sink) = self.audio_sink.as_ref()
                        && (self.status.source_kind == Some(MediaSourceKind::Mp4)
                            || self.status.audio_monitoring)
                    {
                        let _ = sink.resume();
                    }
                }
                Self::reply_status(&reply, result.map(|()| self.status.clone()));
            }
            MediaCommand::AttachPreview(reply) => {
                self.preview_attached = true;
                if let Some(session) = self.session.as_mut() {
                    session.set_preview_enabled(true);
                }
                if self.status.audio.is_some() && self.audio_sink.is_none() {
                    match AudioPreviewSink::open() {
                        Ok(sink) => {
                            let configure_result = self
                                .session
                                .as_mut()
                                .ok_or(MediaError::NoSourceOpen)
                                .and_then(|session| session.set_audio_output_format(sink.format()));
                            if let Err(error) = configure_result {
                                self.status.last_pipeline_error =
                                    Some(format!("AudioDecode: {error}"));
                            }
                            sink.set_control(self.status.muted, self.status.volume);
                            if self.status.source_kind == Some(MediaSourceKind::Camera)
                                && !self.status.audio_monitoring
                            {
                                let _ = sink.pause();
                            }
                            self.audio_sink = Some(sink);
                        }
                        Err(error) => {
                            self.status.last_pipeline_error = Some(format!("AudioSink: {error}"));
                        }
                    }
                }
                Self::reply_status(&reply, Ok(self.status.clone()));
            }
            MediaCommand::DetachPreview(reply) => {
                self.preview_attached = false;
                if let Some(sink) = self.audio_sink.take() {
                    sink.clear();
                }
                let result = self.reconcile_runtime_demand();
                Self::reply_status(&reply, result);
            }
            MediaCommand::Pause(reply) => {
                let result = if self.hub.consumer_count() > 0 {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能暂停全局源".to_owned(),
                    ))
                } else {
                    self.with_session(MediaSourceSession::pause)
                };
                if result.is_ok() {
                    self.status.source_status = MediaSourceStatus::Paused;
                    if let Some(sink) = self.audio_sink.as_ref() {
                        let _ = sink.pause();
                    }
                }
                Self::reply_status(&reply, result.map(|()| self.status.clone()));
            }
            MediaCommand::Stop(reply) => {
                let result = self.stop_source();
                Self::reply_status(&reply, result);
            }
            MediaCommand::Close(reply) => {
                let result = self.close_source();
                Self::reply_status(&reply, result);
            }
            MediaCommand::Reset(reply) => {
                let result = self.reset_source();
                Self::reply_status(&reply, result);
            }
            MediaCommand::Seek {
                position_seconds,
                reply,
            } => {
                let result = if self.hub.consumer_count() > 0 {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能跳转全局源".to_owned(),
                    ))
                } else {
                    self.seek_source(position_seconds)
                };
                Self::reply_status(&reply, result);
            }
            MediaCommand::SetPlaybackRate { rate, reply } => {
                let result = if self.hub.consumer_count() > 0 {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能修改全局播放倍速".to_owned(),
                    ))
                } else if rate.is_finite() && (0.25..=4.0).contains(&rate) {
                    self.status.playback_rate = rate;
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| !session.is_live_capture())
                    {
                        let current = self
                            .last_media_timestamp
                            .map(MediaClock::timestamp_seconds)
                            .unwrap_or_default()
                            .max(0.0)
                            / rate;
                        self.pacing_anchor =
                            Instant::now().checked_sub(Duration::from_secs_f64(current));
                    }
                    Ok(self.status.clone())
                } else {
                    Err(MediaError::Playback(
                        "播放倍速必须介于 0.25 和 4.0".to_owned(),
                    ))
                };
                Self::reply_status(&reply, result);
            }
            MediaCommand::SetAudioControl {
                muted,
                volume,
                reply,
            } => {
                let result = if volume.is_finite() && (0.0..=1.0).contains(&volume) {
                    self.status.muted = muted;
                    self.status.volume = volume;
                    if let Some(sink) = self.audio_sink.as_ref() {
                        sink.set_control(muted, volume);
                    }
                    Ok(self.status.clone())
                } else {
                    Err(MediaError::Playback("音量必须介于 0.0 和 1.0".to_owned()))
                };
                Self::reply_status(&reply, result);
            }
            MediaCommand::SetAudioMonitoring { enabled, reply } => {
                let result = if self.status.source_kind != Some(MediaSourceKind::Camera) {
                    Err(MediaError::UnsupportedSource(
                        "音频监听仅适用于摄像头".to_owned(),
                    ))
                } else if self.status.audio.is_none() {
                    Err(MediaError::UnsupportedSource(
                        "当前摄像头未启用麦克风".to_owned(),
                    ))
                } else if let Some(sink) = self.audio_sink.as_ref() {
                    self.status.audio_monitoring = enabled;
                    if enabled { sink.resume() } else { sink.pause() }.map(|()| self.status.clone())
                } else {
                    self.status.audio_monitoring = enabled;
                    Ok(self.status.clone())
                };
                Self::reply_status(&reply, result);
            }
            MediaCommand::StepFrame(reply) => {
                let result = self.step_frame();
                let _ = reply.try_send(result);
            }
            MediaCommand::Subscribe {
                kind,
                capacity,
                policy,
                reply,
            } => {
                let subscription = self.hub.subscribe(kind, capacity, policy);
                let id = subscription.id;
                match self.reconcile_runtime_demand() {
                    Ok(_) => {
                        let _ = reply.try_send(Ok(subscription));
                    }
                    Err(error) => {
                        let _ = self.hub.unsubscribe(id);
                        let _ = reply.try_send(Err(error));
                    }
                }
            }
            MediaCommand::Unsubscribe(id) => {
                let _ = self.hub.unsubscribe(id);
                let _ = self.reconcile_runtime_demand();
            }
            MediaCommand::Shutdown(reply) => {
                let _ = self.finalize_current_session();
                self.session = None;
                let _ = reply.try_send(());
                return false;
            }
        }
        self.publish_status();
        true
    }

    fn replace_session(
        &mut self,
        session: MediaSourceSession,
        source_kind: MediaSourceKind,
    ) -> MediaResult<MediaRuntimeStatus> {
        self.finalize_current_session()?;
        let probe = session.probe().clone();
        self.session = Some(session);
        self.hub.replace_source(probe.clone());
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.reset_or_continue_clock();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status = MediaRuntimeStatus::ready(
            source_kind,
            probe.video,
            probe.audio,
            probe.duration_seconds,
        );
        self.status.active_live_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Live))
                .unwrap_or(u64::MAX);
        self.status.active_recorder_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Recorder))
                .unwrap_or(u64::MAX);
        if let Some(session) = self.session.as_mut() {
            session.set_preview_enabled(self.preview_attached);
            session.set_encoded_enabled(self.hub.consumer_count() > 0)?;
        }
        Ok(self.status.clone())
    }

    fn release_current_source(&mut self) -> MediaResult<()> {
        self.finalize_current_session()?;
        self.session = None;
        self.audio_sink = None;
        self.clock.reset();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status = MediaRuntimeStatus::unconfigured();
        Ok(())
    }

    fn stop_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let source_kind = self.status.source_kind;
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.stop()?;
        if source_kind == Some(MediaSourceKind::Camera) {
            self.finalize_current_session()?;
            self.session = None;
        }
        self.reset_or_continue_clock();
        self.status.source_status = MediaSourceStatus::Stopped;
        self.status.position_seconds = 0.0;
        Ok(self.status.clone())
    }

    fn close_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        self.finalize_current_session()?;
        self.session = None;
        self.audio_sink = None;
        self.clock.reset();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status = MediaRuntimeStatus::unconfigured();
        Ok(self.status.clone())
    }

    fn finalize_current_session(&mut self) -> MediaResult<()> {
        let packets = match self.session.as_mut() {
            Some(session) => session.finish_encoded_packets()?,
            None => return Ok(()),
        };
        for mut packet in packets {
            self.clock.normalize(&mut packet);
            let packet = Arc::new(packet);
            let _ = self.hub.broadcast(&packet);
        }
        Ok(())
    }

    fn reset_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.reset()?;
        self.reset_or_continue_clock();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status.source_status = MediaSourceStatus::Ready;
        self.status.position_seconds = 0.0;
        Ok(self.status.clone())
    }

    fn seek_source(&mut self, position_seconds: f64) -> MediaResult<MediaRuntimeStatus> {
        if let Some(duration) = self.status.duration_seconds
            && position_seconds > duration
        {
            return Err(MediaError::Playback("跳转位置超过媒体总时长".to_owned()));
        }
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let frame = session.seek_frame(position_seconds)?;
        self.hub.begin_timeline();
        self.clock.begin_seek();
        self.reset_or_continue_clock();
        self.pacing_anchor = Instant::now().checked_sub(Duration::from_secs_f64(
            position_seconds.max(0.0) / self.status.playback_rate.max(0.01),
        ));
        self.last_media_timestamp = None;
        self.status.position_seconds = frame
            .as_ref()
            .map_or(position_seconds, |frame| frame.position_seconds);
        if let Some(frame) = frame {
            let _ = self.preview.try_send(frame);
        }
        Ok(self.status.clone())
    }

    fn step_frame(&mut self) -> MediaResult<Option<MediaVideoFrame>> {
        if self.status.source_status == MediaSourceStatus::Playing {
            return Err(MediaError::Playback(
                "单帧步进仅允许在暂停或就绪状态使用".to_owned(),
            ));
        }
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let frame = session.step_frame()?;
        if let Some(frame) = &frame {
            self.status.position_seconds = frame.position_seconds;
            self.status.decoded_frames = self.status.decoded_frames.saturating_add(1);
        }
        Ok(frame)
    }

    fn reset_or_continue_clock(&mut self) {
        if self.hub.has_consumer(MediaConsumerKind::Live)
            || self.hub.has_consumer(MediaConsumerKind::Recorder)
        {
            self.clock.begin_loop();
        } else {
            self.clock.reset();
        }
        self.clock.set_source_epoch(
            self.session
                .as_ref()
                .and_then(MediaSourceSession::timestamp_origin),
        );
    }

    fn reconcile_runtime_demand(&mut self) -> MediaResult<MediaRuntimeStatus> {
        let has_encoded_consumer = self.hub.consumer_count() > 0;
        self.status.active_live_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Live))
                .unwrap_or(u64::MAX);
        self.status.active_recorder_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Recorder))
                .unwrap_or(u64::MAX);
        let has_demand = self.preview_attached || has_encoded_consumer;
        if has_demand && self.session.is_none() {
            return Err(MediaError::NoSourceOpen);
        }
        if let Some(session) = self.session.as_mut() {
            session.set_preview_enabled(self.preview_attached);
            session.set_encoded_enabled(has_encoded_consumer)?;
        }
        if !has_demand && self.session.is_some() {
            return self.stop_source();
        }
        Ok(self.status.clone())
    }

    fn with_session(&mut self, operation: impl FnOnce(&mut MediaSourceSession)) -> MediaResult<()> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        operation(session);
        Ok(())
    }

    fn reply_status(reply: &StatusReply, result: MediaResult<MediaRuntimeStatus>) {
        let _ = reply.try_send(result);
    }

    fn publish_status(&self) {
        if let Ok(mut status) = self.shared_status.write() {
            status.clone_from(&self.status);
        }
    }
}

fn pacing_deadline(anchor: Instant, media_seconds: f64, rate: f64) -> Instant {
    let elapsed = Duration::from_secs_f64((media_seconds / rate.max(0.01)).max(0.0));
    anchor.checked_add(elapsed).unwrap_or_else(Instant::now)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use super::{GlobalMediaRuntime, pacing_deadline};
    use crate::media::{MediaError, MediaSourceStatus};

    #[test]
    fn runtime_should_start_and_shutdown_without_a_source() {
        let runtime = GlobalMediaRuntime::start();

        assert_eq!(
            runtime.handle().status().source_status,
            MediaSourceStatus::Unconfigured
        );
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn commands_should_preserve_no_source_error() {
        let runtime = GlobalMediaRuntime::start();

        assert!(matches!(
            runtime.handle().play(),
            Err(MediaError::NoSourceOpen)
        ));
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn resume_anchor_should_not_reintroduce_the_already_played_gap() {
        let now = Instant::now();
        let anchor = now.checked_sub(Duration::from_secs(10)).unwrap_or(now);
        let deadline = pacing_deadline(anchor, 10.04, 1.0);

        assert!(deadline.duration_since(now) < Duration::from_millis(200));
    }

    #[test]
    fn resume_anchor_should_scale_with_playback_rate() {
        let now = Instant::now();
        let anchor = now.checked_sub(Duration::from_secs(10)).unwrap_or(now);
        let deadline = pacing_deadline(anchor, 10.5, 2.0);

        assert!(deadline.duration_since(now) < Duration::from_millis(400));
    }

    #[test]
    fn lifecycle_interrupts_should_not_publish_a_source_error() {
        let runtime = GlobalMediaRuntime::start();
        let handle = runtime.handle();
        let asset = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
            .join("h264-noaudio.mp4");
        assert!(handle.open_mp4(asset, true).is_ok());
        assert!(handle.attach_preview().is_ok());
        assert!(handle.play().is_ok());
        assert!(handle.pause().is_ok());
        assert!(handle.detach_preview().is_ok());
        assert!(handle.status().last_error.is_none());
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn mp4_worker_should_support_full_control_and_source_replacement_lifecycle() {
        let runtime = GlobalMediaRuntime::start();
        let handle = runtime.handle();
        let asset = |name: &str| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("assets")
                .join(name)
        };

        assert!(handle.open_mp4(asset("h264-aac.mp4"), false).is_ok());
        assert!(handle.play().is_ok());
        assert!(handle.pause().is_ok());
        assert!(handle.seek(0.2).is_ok());
        assert!(handle.reset().is_ok());
        assert!(handle.open_mp4(asset("h265-noaudio.mp4"), true).is_ok());
        assert_eq!(
            handle.status().video.map(|video| video.codec),
            Some(crate::media::VideoCodec::H265)
        );
        assert!(handle.close().is_ok());
        assert_eq!(
            handle.status().source_status,
            MediaSourceStatus::Unconfigured
        );
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn mp4_aac_preview_should_decode_pcm_without_an_encoded_consumer() {
        let runtime = GlobalMediaRuntime::start();
        let handle = runtime.handle();
        let asset = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
            .join("h264-aac.mp4");
        assert!(handle.open_mp4(asset, true).is_ok());
        assert!(handle.attach_preview().is_ok());
        assert!(handle.play().is_ok());
        std::thread::sleep(Duration::from_millis(350));
        assert!(handle.set_audio_control(true, 0.25).is_ok());
        assert!(handle.pause().is_ok());
        assert!(handle.seek(0.1).is_ok());
        assert!(handle.play().is_ok());
        assert!(handle.set_audio_control(false, 0.75).is_ok());
        std::thread::sleep(Duration::from_millis(150));

        let status = handle.status();
        assert!(status.metrics.audio_packets_captured > 0);
        assert!(status.metrics.audio_frames_decoded > 0);
        assert_eq!(status.active_live_consumers, 0);
        assert!(!status.muted);
        assert!((status.volume - 0.75).abs() < f64::EPSILON);
        assert!(status.last_pipeline_error.is_none());
        assert!(runtime.shutdown().is_ok());
    }
}
