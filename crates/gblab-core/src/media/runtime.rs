//! Dedicated owner thread for the single global media source.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::{
    BackpressurePolicy, CameraCaptureSettings, CameraMediaSource, MediaClock, MediaConsumerKind,
    MediaError, MediaResult, MediaRuntimeStatus, MediaSourceKind, MediaSourceStatus,
    MediaStreamHub, MediaSubscription, MediaVideoFrame, Mp4MediaSource,
    types::{MediaSource, MediaSourceSession},
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
    interrupt: Arc<AtomicBool>,
}

impl GlobalMediaHandle {
    /// Opens the single MP4 source on the owner worker.
    pub fn open_mp4(&self, path: PathBuf, looping: bool) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt.store(true, Ordering::Release);
        let result = self.request_status(|reply| MediaCommand::OpenMp4 {
            path,
            looping,
            reply,
        });
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt.store(false, Ordering::Release);
        }
        result
    }

    /// Opens the single camera source on the owner worker.
    pub fn open_camera(&self, settings: CameraCaptureSettings) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt.store(true, Ordering::Release);
        let result = self.request_status(|reply| MediaCommand::OpenCamera { settings, reply });
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt.store(false, Ordering::Release);
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
        self.request_interrupting_status(MediaCommand::DetachPreview)
    }

    /// Pauses source production.
    pub fn pause(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(MediaCommand::Pause)
    }

    /// Stops production and releases live capture devices.
    pub fn stop(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(MediaCommand::Stop)
    }

    /// Closes the current source and releases every owned codec/input context.
    pub fn close(&self) -> MediaResult<MediaRuntimeStatus> {
        self.request_interrupting_status(MediaCommand::Close)
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
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Cancel blocking capture/open work before reporting timeout. The worker checks
                // this interrupt flag in the FFmpeg input callback and will not keep a stale
                // device read alive after the caller has abandoned the request.
                self.interrupt.store(true, Ordering::Release);
                Err(MediaError::CommandTimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(MediaError::RuntimeUnavailable(
                "媒体 worker 已停止".to_owned(),
            )),
        }
    }

    fn request_interrupting_status(
        &self,
        build: impl FnOnce(StatusReply) -> MediaCommand,
    ) -> MediaResult<MediaRuntimeStatus> {
        self.interrupt.store(true, Ordering::Release);
        let result = self.request_status(build);
        if matches!(result, Err(MediaError::RuntimeUnavailable(_))) {
            self.interrupt.store(false, Ordering::Release);
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
        let interrupt = Arc::new(AtomicBool::new(false));
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
        self.handle.interrupt.store(true, Ordering::Release);
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
    interrupt: Arc<AtomicBool>,
    preview_attached: bool,
}

impl MediaWorker {
    fn new(
        commands: mpsc::Receiver<MediaCommand>,
        preview: mpsc::SyncSender<MediaVideoFrame>,
        shared_status: Arc<RwLock<MediaRuntimeStatus>>,
        interrupt: Arc<AtomicBool>,
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
                self.status.last_error = Some(error.to_string());
                self.publish_status();
            }
        }
        self.session = None;
    }

    fn produce_once(&mut self) -> MediaResult<()> {
        let session = self.session.as_mut().ok_or(MediaError::NoSourceOpen)?;
        let mut output = session.read_source_output()?;
        if output.looped {
            self.clock.begin_loop();
        }
        if output.end_of_stream {
            self.status.source_status = MediaSourceStatus::Stopped;
        }
        if let Some(mut packet) = output.packet.take() {
            self.clock.normalize(&mut packet);
            self.status.position_seconds = packet.position_seconds();
            self.schedule_next_read(packet.pts.or(packet.dts));
            let packet = Arc::new(packet);
            let _ = self.hub.broadcast(&packet);
        }
        for frame in output.preview_frames {
            self.status.position_seconds = frame.position_seconds;
            self.status.decoded_frames = self.status.decoded_frames.saturating_add(1);
            let _ = self.preview.try_send(frame);
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
            let elapsed = Duration::from_secs_f64((media_seconds / rate).max(0.0));
            anchor.checked_add(elapsed).unwrap_or_else(Instant::now)
        };
        self.next_read_at = deadline;
    }

    /// Returns true while the worker should keep running.
    #[expect(
        clippy::too_many_lines,
        reason = "The owner thread keeps serialized media commands in one explicit dispatcher"
    )]
    fn handle_command(&mut self, command: MediaCommand) -> bool {
        self.interrupt.store(false, Ordering::Release);
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
                    self.pacing_anchor = Some(Instant::now());
                    self.last_media_timestamp = None;
                }
                Self::reply_status(&reply, result.map(|()| self.status.clone()));
            }
            MediaCommand::AttachPreview(reply) => {
                self.preview_attached = true;
                Self::reply_status(&reply, Ok(self.status.clone()));
            }
            MediaCommand::DetachPreview(reply) => {
                self.preview_attached = false;
                let result = if !self.preview_attached && self.hub.consumer_count() == 0 {
                    self.stop_source()
                } else {
                    Ok(self.status.clone())
                };
                Self::reply_status(&reply, result);
            }
            MediaCommand::Pause(reply) => {
                let result = self.with_session(MediaSourceSession::pause);
                if result.is_ok() {
                    self.status.source_status = MediaSourceStatus::Paused;
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
                let result = self.seek_source(position_seconds);
                Self::reply_status(&reply, result);
            }
            MediaCommand::SetPlaybackRate { rate, reply } => {
                let result = if rate.is_finite() && (0.25..=4.0).contains(&rate) {
                    self.status.playback_rate = rate;
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| !session.is_live_capture())
                    {
                        let current = self.status.position_seconds.max(0.0) / rate;
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
                    Ok(self.status.clone())
                } else {
                    Err(MediaError::Playback("音量必须介于 0.0 和 1.0".to_owned()))
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
                let _ = reply.try_send(Ok(subscription));
            }
            MediaCommand::Unsubscribe(id) => {
                let _ = self.hub.unsubscribe(id);
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
        Ok(self.status.clone())
    }

    fn release_current_source(&mut self) -> MediaResult<()> {
        self.finalize_current_session()?;
        self.session = None;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::GlobalMediaRuntime;
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
}
