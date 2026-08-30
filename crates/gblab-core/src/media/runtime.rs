//! Dedicated owner thread for the single global media source.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::audio_preview::AudioPreviewSink;
use super::{
    BackpressurePolicy, MediaClock, MediaConsumerKind, MediaError, MediaResult, MediaRuntimeStatus,
    MediaSourceKind, MediaSourceStatus, MediaStreamHub, MediaSubscription, MediaVideoFrame,
    Mp4MediaSource,
    types::{AudioSinkInfo, AudioSinkStatus, MediaSource, MediaSourceSession},
};
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
}

impl GlobalMediaHandle {
    /// Opens the single MP4 source on the owner worker.
    pub fn open_mp4(&self, path: PathBuf, looping: bool) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(|reply| MediaCommand::OpenMp4 {
            path,
            looping,
            reply,
        })
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
        self.request_status(MediaCommand::DetachPreview)
    }

    /// Pauses source production.
    pub fn pause(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::Pause)
    }

    /// Stops production and resets the active MP4 session to its beginning.
    pub fn stop(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::Stop)
    }

    /// Closes the current source and releases every owned codec/input context.
    pub fn close(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_status(MediaCommand::Close)
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
            Err(mpsc::RecvTimeoutError::Timeout) => Err(MediaError::CommandTimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(MediaError::RuntimeUnavailable(
                "媒体 worker 已停止".to_owned(),
            )),
        }
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
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::RuntimeUnavailable`] when the operating system
    /// cannot create the owner thread.
    pub fn start() -> MediaResult<Self> {
        Self::start_with_spawner(|job| {
            thread::Builder::new()
                .name("gblab-media-source".to_owned())
                .spawn(job)
        })
    }

    fn start_with_spawner<F>(spawner: F) -> MediaResult<Self>
    where
        F: FnOnce(Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<thread::JoinHandle<()>>,
    {
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (preview_sender, preview_receiver) = mpsc::sync_channel(PREVIEW_CAPACITY);
        let status = Arc::new(RwLock::new(MediaRuntimeStatus::unconfigured()));
        let worker_status = Arc::clone(&status);
        let worker = spawner(Box::new(move || {
            MediaWorker::new(receiver, preview_sender, worker_status).run();
        }))
        .map_err(|error| {
            MediaError::RuntimeUnavailable(format!("创建媒体 worker 失败: {error}"))
        })?;
        Ok(Self {
            handle: GlobalMediaHandle {
                commands,
                status,
                preview: Arc::new(Mutex::new(preview_receiver)),
            },
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Returns a cloneable command handle.
    #[must_use]
    pub fn handle(&self) -> GlobalMediaHandle {
        self.handle.clone()
    }

    /// Stops and joins the owner thread. Calling this more than once is harmless.
    pub fn shutdown(&self) -> MediaResult<()> {
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
    preview_attached: bool,
    audio_sink: Option<AudioPreviewSink>,
    last_audio_activity: Option<Instant>,
}

impl MediaWorker {
    fn new(
        commands: mpsc::Receiver<MediaCommand>,
        preview: mpsc::SyncSender<MediaVideoFrame>,
        shared_status: Arc<RwLock<MediaRuntimeStatus>>,
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
            preview_attached: false,
            audio_sink: None,
            last_audio_activity: None,
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
        }
        let pacing_timestamp = output
            .pacing_timestamp
            .map(|timestamp| self.clock.normalize_timestamp(timestamp));
        if output.end_of_stream {
            self.status.source_status = MediaSourceStatus::Stopped;
        }
        if let Some(mut packet) = output.packet.take() {
            self.clock.normalize(&mut packet);
            self.status.position_seconds = packet.position_seconds();
            self.schedule_next_read(packet.dts.or(packet.pts).or(pacing_timestamp));
            let packet = Arc::new(packet);
            let report = self.hub.broadcast(&packet);
            if report.disconnected > 0 {
                let _ = self.reconcile_runtime_demand();
            }
        } else if pacing_timestamp.is_some() {
            self.schedule_next_read(pacing_timestamp);
        }
        for frame in output.preview_frames {
            self.status.position_seconds = frame.position_seconds;
            self.status.decoded_frames = self.status.decoded_frames.saturating_add(1);
            let _ = self.preview.try_send(frame);
        }
        self.status.metrics.merge(output.metrics);
        if output.metrics.audio_frames_decoded > 0 {
            self.last_audio_activity = Some(Instant::now());
        } else if self
            .last_audio_activity
            .is_some_and(|last| last.elapsed() >= Duration::from_millis(500))
        {
            self.status.metrics.audio_rms = 0.0;
            self.status.metrics.audio_peak = 0.0;
        }
        let branch_errors = output.branch_errors;
        if let Some(error) = branch_errors.iter().next_back().cloned() {
            self.status.last_pipeline_error = Some(error);
        } else if self
            .status
            .last_pipeline_error
            .as_deref()
            .is_some_and(|error| pipeline_error_recovered(error, output.metrics))
        {
            self.status.last_pipeline_error = None;
        }
        if self.preview_attached
            && let Some(sink) = self.audio_sink.as_ref()
        {
            for frame in output.audio_frames {
                sink.push(&frame.samples);
            }
        }
        self.refresh_audio_sink_status();
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
        match command {
            MediaCommand::OpenMp4 {
                path,
                looping,
                reply,
            } => {
                let result = self
                    .ensure_source_replacement_allowed()
                    .and_then(|()| Mp4MediaSource::new(path).open(looping))
                    .map(|session| self.replace_session(session));
                self.reply_status(&reply, result);
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
                    self.sync_audio_sink_playback(true);
                }
                self.reply_status(&reply, result.map(|()| self.status.clone()));
            }
            MediaCommand::AttachPreview(reply) => {
                self.preview_attached = true;
                if let Some(session) = self.session.as_mut() {
                    session.set_preview_enabled(true);
                }
                self.ensure_audio_sink();
                self.reply_status(&reply, Ok(self.status.clone()));
            }
            MediaCommand::DetachPreview(reply) => {
                self.preview_attached = false;
                if let Some(sink) = self.audio_sink.take() {
                    sink.clear();
                }
                self.status.audio_sink = None;
                let result = self.reconcile_runtime_demand();
                self.reply_status(&reply, result);
            }
            MediaCommand::Pause(reply) => {
                let result = if self.has_stream_consumers() {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能暂停全局源".to_owned(),
                    ))
                } else {
                    self.with_session(MediaSourceSession::pause)
                };
                if result.is_ok() {
                    self.status.source_status = MediaSourceStatus::Paused;
                    self.sync_audio_sink_playback(false);
                    self.status.metrics.audio_rms = 0.0;
                    self.status.metrics.audio_peak = 0.0;
                    self.last_audio_activity = None;
                }
                self.reply_status(&reply, result.map(|()| self.status.clone()));
            }
            MediaCommand::Stop(reply) => {
                let result = self.stop_source();
                self.reply_status(&reply, result);
            }
            MediaCommand::Close(reply) => {
                let result = self.close_source();
                self.reply_status(&reply, result);
            }
            MediaCommand::Reset(reply) => {
                let result = self.reset_source();
                self.reply_status(&reply, result);
            }
            MediaCommand::Seek {
                position_seconds,
                reply,
            } => {
                let result = if self.has_stream_consumers() {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能跳转全局源".to_owned(),
                    ))
                } else {
                    self.seek_source(position_seconds)
                };
                self.reply_status(&reply, result);
            }
            MediaCommand::SetPlaybackRate { rate, reply } => {
                let result = if self.has_stream_consumers() {
                    Err(MediaError::Playback(
                        "存在 Live/Recorder 消费者时不能修改全局播放倍速".to_owned(),
                    ))
                } else if rate.is_finite() && (0.25..=4.0).contains(&rate) {
                    let audio_error = match self
                        .session
                        .as_mut()
                        .ok_or(MediaError::NoSourceOpen)
                        .map(|session| session.set_playback_rate(rate))
                    {
                        Ok(error) => error,
                        Err(error) => {
                            self.reply_status(&reply, Err(error));
                            return true;
                        }
                    };
                    self.status.playback_rate = rate;
                    if let Some(error) = audio_error {
                        self.status.last_pipeline_error = Some(error);
                    } else if self
                        .status
                        .last_pipeline_error
                        .as_deref()
                        .is_some_and(|error| error.starts_with("AudioTempo:"))
                    {
                        self.status.last_pipeline_error = None;
                    }
                    if let Some(sink) = self.audio_sink.as_ref() {
                        sink.clear();
                    }
                    let current = self
                        .last_media_timestamp
                        .map(MediaClock::timestamp_seconds)
                        .unwrap_or_default()
                        .max(0.0)
                        / rate;
                    self.pacing_anchor =
                        Instant::now().checked_sub(Duration::from_secs_f64(current));
                    Ok(self.status.clone())
                } else {
                    Err(MediaError::Playback(
                        "播放倍速必须介于 0.25 和 4.0".to_owned(),
                    ))
                };
                self.reply_status(&reply, result);
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
                self.reply_status(&reply, result);
            }
            MediaCommand::StepFrame(reply) => {
                let result = self.step_frame();
                self.publish_status();
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
                        self.publish_status();
                        let _ = reply.try_send(Ok(subscription));
                    }
                    Err(error) => {
                        let _ = self.hub.unsubscribe(id);
                        self.publish_status();
                        let _ = reply.try_send(Err(error));
                    }
                }
            }
            MediaCommand::Unsubscribe(id) => {
                let _ = self.hub.unsubscribe(id);
                let _ = self.reconcile_runtime_demand();
            }
            MediaCommand::Shutdown(reply) => {
                self.session = None;
                let _ = reply.try_send(());
                return false;
            }
        }
        self.publish_status();
        true
    }

    fn replace_session(&mut self, session: MediaSourceSession) -> MediaRuntimeStatus {
        if let Some(sink) = self.audio_sink.take() {
            sink.clear();
        }
        self.last_audio_activity = None;
        let probe = session.probe().clone();
        self.session = Some(session);
        self.hub.replace_source(probe.clone());
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.reset_or_continue_clock();
        self.status = MediaRuntimeStatus::ready(
            MediaSourceKind::Mp4,
            probe.video,
            probe.audio,
            probe.duration_seconds,
        );
        self.status.last_pipeline_error = self
            .session
            .as_ref()
            .and_then(MediaSourceSession::initial_pipeline_error);
        self.status.active_live_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Live))
                .unwrap_or(u64::MAX);
        self.status.active_recorder_consumers =
            u64::try_from(self.hub.consumer_count_by_kind(MediaConsumerKind::Recorder))
                .unwrap_or(u64::MAX);
        if let Some(session) = self.session.as_mut() {
            session.set_preview_enabled(self.preview_attached);
        }
        if self.preview_attached {
            self.ensure_audio_sink();
        }
        self.status.clone()
    }

    fn ensure_source_replacement_allowed(&self) -> MediaResult<()> {
        if self.hub.has_consumer(MediaConsumerKind::Live)
            || self.hub.has_consumer(MediaConsumerKind::Recorder)
        {
            return Err(MediaError::Playback(
                "存在 Live/Recorder 消费者时不能替换或关闭全局源".to_owned(),
            ));
        }
        Ok(())
    }

    fn stop_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        if self.has_stream_consumers() {
            return Err(MediaError::Playback(
                "存在 Live/Recorder 消费者时不能停止全局源".to_owned(),
            ));
        }
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.stop()?;
        if let Some(sink) = self.audio_sink.as_ref() {
            sink.clear();
        }
        self.reset_or_continue_clock();
        self.status.source_status = MediaSourceStatus::Stopped;
        self.status.position_seconds = 0.0;
        self.status.metrics.audio_rms = 0.0;
        self.status.metrics.audio_peak = 0.0;
        self.last_audio_activity = None;
        self.sync_audio_sink_playback(false);
        Ok(self.status.clone())
    }

    fn close_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        self.ensure_source_replacement_allowed()?;
        self.session = None;
        self.audio_sink = None;
        self.last_audio_activity = None;
        self.clock.reset();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status = MediaRuntimeStatus::unconfigured();
        Ok(self.status.clone())
    }

    fn reset_source(&mut self) -> MediaResult<MediaRuntimeStatus> {
        if self.has_stream_consumers() {
            return Err(MediaError::Playback(
                "存在 Live/Recorder 消费者时不能重置全局源".to_owned(),
            ));
        }
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        session.reset()?;
        if let Some(sink) = self.audio_sink.as_ref() {
            sink.clear();
        }
        self.reset_or_continue_clock();
        self.pacing_anchor = None;
        self.last_media_timestamp = None;
        self.status.source_status = MediaSourceStatus::Ready;
        self.status.position_seconds = 0.0;
        self.status.metrics.audio_rms = 0.0;
        self.status.metrics.audio_peak = 0.0;
        self.last_audio_activity = None;
        self.sync_audio_sink_playback(false);
        Ok(self.status.clone())
    }

    fn seek_source(&mut self, position_seconds: f64) -> MediaResult<MediaRuntimeStatus> {
        if let Some(duration) = self.status.duration_seconds
            && position_seconds > duration
        {
            return Err(MediaError::Playback("跳转位置超过媒体总时长".to_owned()));
        }
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let was_playing = self.status.source_status == MediaSourceStatus::Playing;
        let frame = session.seek_frame(position_seconds)?;
        if let Some(sink) = self.audio_sink.as_ref() {
            sink.clear();
        }
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
        self.status.metrics.audio_rms = 0.0;
        self.status.metrics.audio_peak = 0.0;
        self.last_audio_activity = None;
        self.sync_audio_sink_playback(was_playing);
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
        let has_encoded_consumer = self.has_stream_consumers();
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
        }
        if !has_demand && self.session.is_some() {
            return self.stop_source();
        }
        Ok(self.status.clone())
    }

    fn has_stream_consumers(&self) -> bool {
        self.hub.has_consumer(MediaConsumerKind::Live)
            || self.hub.has_consumer(MediaConsumerKind::Recorder)
    }

    fn refresh_audio_sink_status(&mut self) {
        let Some(sink) = self.audio_sink.as_ref() else {
            // Keep an unavailable diagnostic produced by sink setup failure.  There is
            // no native sink to poll in this state, so replacing it with `None` would
            // erase the actionable error on the next media tick.
            return;
        };
        let Some(diagnostics) = sink.diagnostics() else {
            return;
        };
        let sink_error = diagnostics.last_error.clone();
        self.status.audio_sink = Some(diagnostics);
        if let Some(error) = sink_error {
            self.status.last_pipeline_error = Some(format!("AudioSink: {error}"));
        } else if self
            .status
            .last_pipeline_error
            .as_deref()
            .is_some_and(|error| error.starts_with("AudioSink:"))
        {
            self.status.last_pipeline_error = None;
        }
    }

    /// Lazily creates the local sink for the currently attached preview.
    ///
    /// Sink setup is deliberately non-fatal for the source: a missing speaker or an
    /// unsupported output format must leave video preview and encoded consumers usable.
    fn ensure_audio_sink(&mut self) {
        let audio_preview_available = self
            .session
            .as_ref()
            .is_some_and(MediaSourceSession::audio_preview_available);
        if self.status.audio.is_none() || !audio_preview_available {
            if self.status.audio.is_some() && !audio_preview_available {
                let error = self
                    .session
                    .as_ref()
                    .and_then(MediaSourceSession::initial_pipeline_error)
                    .unwrap_or_else(|| "音频预览不可用".to_owned());
                self.status.audio_sink = Some(unavailable_audio_sink(error));
            }
            return;
        }
        if self.audio_sink.is_some() {
            return;
        }
        match AudioPreviewSink::open() {
            Ok(sink) => {
                let configure_result = self
                    .session
                    .as_mut()
                    .ok_or(MediaError::NoSourceOpen)
                    .and_then(|session| session.set_audio_output_format(sink.format()));
                match configure_result {
                    Ok(()) => {
                        sink.set_control(self.status.muted, self.status.volume);
                        self.audio_sink = Some(sink);
                        self.sync_audio_sink_playback(
                            self.status.source_status == MediaSourceStatus::Playing,
                        );
                    }
                    Err(error) => {
                        self.status.last_pipeline_error = Some(format!("AudioDecode: {error}"));
                        self.status.audio_sink = Some(unavailable_audio_sink(error.to_string()));
                    }
                }
            }
            Err(error) => {
                self.status.last_pipeline_error = Some(format!("AudioSink: {error}"));
                self.status.audio_sink = Some(unavailable_audio_sink(error.to_string()));
            }
        }
    }

    fn sync_audio_sink_playback(&mut self, should_play: bool) {
        let result = self.audio_sink.as_ref().map(|sink| {
            if should_play {
                sink.resume()
            } else {
                sink.pause()
            }
        });
        if let Some(Err(error)) = result {
            self.status.last_pipeline_error = Some(format!("AudioSink: {error}"));
        }
        self.refresh_audio_sink_status();
    }

    fn with_session(&mut self, operation: impl FnOnce(&mut MediaSourceSession)) -> MediaResult<()> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        operation(session);
        Ok(())
    }

    fn reply_status(&self, reply: &StatusReply, result: MediaResult<MediaRuntimeStatus>) {
        // Publish before acknowledging the command so a subsequent status read
        // observes the same state that was returned to the caller.
        self.publish_status();
        let _ = reply.try_send(result);
    }

    fn publish_status(&self) {
        if let Ok(mut status) = self.shared_status.write() {
            status.clone_from(&self.status);
        }
    }
}

const fn unavailable_audio_sink(error: String) -> AudioSinkInfo {
    AudioSinkInfo {
        status: AudioSinkStatus::Unavailable,
        queued_samples: 0,
        played_samples: 0,
        underruns: 0,
        dropped_samples: 0,
        last_error: Some(error),
    }
}

fn pipeline_error_recovered(error: &str, metrics: super::MediaRuntimeMetrics) -> bool {
    (error.starts_with("VideoPreview:") && metrics.video_preview_frames > 0)
        || ((error.starts_with("AudioDecode:")
            || error.starts_with("AudioPreview:")
            || error.starts_with("AudioDrain:")
            || error.starts_with("AudioTempo:"))
            && metrics.audio_frames_decoded > 0)
}

fn pacing_deadline(anchor: Instant, media_seconds: f64, rate: f64) -> Instant {
    let elapsed = Duration::from_secs_f64((media_seconds / rate.max(0.01)).max(0.0));
    anchor.checked_add(elapsed).unwrap_or_else(Instant::now)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::panic,
        reason = "the runtime test helper reports worker-start failures with full context"
    )]
    use std::{
        io,
        path::PathBuf,
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };

    use super::{
        GlobalMediaRuntime, MediaWorker, pacing_deadline, pipeline_error_recovered,
        unavailable_audio_sink,
    };
    use crate::media::MediaRuntimeMetrics;
    use crate::media::{AudioSinkStatus, MediaError, MediaSourceStatus};

    fn start_runtime() -> GlobalMediaRuntime {
        match GlobalMediaRuntime::start() {
            Ok(runtime) => runtime,
            Err(error) => panic!("failed to start media runtime: {error}"),
        }
    }

    #[test]
    fn runtime_should_start_and_shutdown_without_a_source() {
        let runtime = start_runtime();

        assert_eq!(
            runtime.handle().status().source_status,
            MediaSourceStatus::Unconfigured
        );
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn pipeline_error_should_clear_only_after_the_same_branch_recovers() {
        let mut metrics = MediaRuntimeMetrics::default();
        assert!(!pipeline_error_recovered("AudioDecode: failed", metrics));
        metrics.audio_frames_decoded = 1;
        assert!(pipeline_error_recovered("AudioDecode: failed", metrics));
    }

    #[test]
    fn refresh_should_preserve_unavailable_sink_diagnostics_without_a_native_sink() {
        let (_command_sender, command_receiver) = mpsc::sync_channel(1);
        let (preview_sender, _preview_receiver) = mpsc::sync_channel(1);
        let shared_status = Arc::new(std::sync::RwLock::new(
            crate::media::MediaRuntimeStatus::unconfigured(),
        ));
        let mut worker = MediaWorker::new(command_receiver, preview_sender, shared_status);
        worker.status.audio_sink = Some(unavailable_audio_sink("speaker unavailable".to_owned()));
        worker.status.last_pipeline_error = Some("AudioSink: speaker unavailable".to_owned());

        worker.refresh_audio_sink_status();

        assert_eq!(
            worker
                .status
                .audio_sink
                .as_ref()
                .and_then(|sink| sink.last_error.as_deref()),
            Some("speaker unavailable")
        );
        assert_eq!(
            worker.status.last_pipeline_error.as_deref(),
            Some("AudioSink: speaker unavailable")
        );
    }

    #[test]
    fn commands_should_preserve_no_source_error() {
        let runtime = start_runtime();

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
    fn lifecycle_commands_should_not_publish_a_source_error() {
        let runtime = start_runtime();
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
        let runtime = start_runtime();
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
        let runtime = start_runtime();
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
        assert!(status.metrics.audio_packets_read > 0);
        assert!(status.metrics.audio_frames_decoded > 0);
        assert_eq!(status.active_live_consumers, 0);
        assert!(!status.muted);
        assert!((status.volume - 0.75).abs() < f64::EPSILON);
        // Audio output is optional on headless/Linux test runners. A sink
        // failure must remain observable without turning into a source error.
        assert!(status.last_error.is_none());
        if let Some(sink) = status.audio_sink
            && sink.status == AudioSinkStatus::Playing
        {
            assert!(sink.played_samples > 0);
        }
        assert!(runtime.shutdown().is_ok());
    }

    #[test]
    fn worker_spawn_failure_should_be_reported() {
        let result = GlobalMediaRuntime::start_with_spawner(|_| {
            Err(io::Error::other("injected worker spawn failure"))
        });

        assert!(matches!(
            result,
            Err(MediaError::RuntimeUnavailable(message))
                if message.contains("injected worker spawn failure")
        ));
    }
}
