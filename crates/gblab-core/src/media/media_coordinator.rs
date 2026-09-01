//! Bounded coordinator for active GB28181 media sessions.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::sync::Mutex;

use super::{
    BackpressurePolicy, GlobalMediaHandle, MediaConsumerKind, MediaError, MediaResult, MediaSession,
};

/// Configuration for the real-time media coordinator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaCoordinatorConfig {
    /// Maximum concurrent dialogs.
    pub max_sessions: usize,
    /// RTP payload type.
    pub payload_type: u8,
    /// Maximum UDP packet size including the RTP header.
    pub mtu: usize,
}

impl Default for MediaCoordinatorConfig {
    fn default() -> Self {
        Self {
            max_sessions: 64,
            payload_type: 96,
            mtu: 1_400,
        }
    }
}

/// Owns active media sessions and their Live subscriptions.
#[derive(Clone)]
pub struct MediaSessionCoordinator {
    media: GlobalMediaHandle,
    config: MediaCoordinatorConfig,
    sessions: Arc<Mutex<HashMap<String, MediaSession>>>,
}

impl MediaSessionCoordinator {
    /// Creates an empty coordinator over the global media source.
    #[must_use]
    pub fn new(media: GlobalMediaHandle, config: MediaCoordinatorConfig) -> Self {
        Self {
            media,
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Starts one RTP session for a dialog.
    pub async fn start(
        &self,
        dialog_id: impl Into<String>,
        remote: SocketAddr,
        ssrc: u32,
    ) -> MediaResult<SocketAddr> {
        self.start_with_payload_type(dialog_id, remote, self.config.payload_type, ssrc)
            .await
    }

    /// Starts one RTP session using the payload type selected during SDP negotiation.
    pub async fn start_with_payload_type(
        &self,
        dialog_id: impl Into<String>,
        remote: SocketAddr,
        payload_type: u8,
        ssrc: u32,
    ) -> MediaResult<SocketAddr> {
        if matches!(
            self.media.status().source_status,
            super::MediaSourceStatus::Stopped
        ) {
            // A non-looping MP4 remains at EOF after the previous dialog.  All
            // sessions are stale at that point, so release their subscriptions
            // before rewinding the single global source for a new INVITE.
            self.stop_all().await;
            self.media.reset()?;
        }
        let dialog_id = dialog_id.into();
        {
            let sessions = self.sessions.lock().await;
            if sessions.len() >= self.config.max_sessions {
                return Err(MediaError::RuntimeUnavailable(
                    "媒体会话已达到并发上限".to_owned(),
                ));
            }
            if sessions.contains_key(&dialog_id) {
                return Err(MediaError::RuntimeUnavailable("媒体会话已存在".to_owned()));
            }
        }
        let subscription =
            self.media
                .subscribe(MediaConsumerKind::Live, 128, BackpressurePolicy::Disconnect)?;
        let subscription_id = subscription.id;
        let session = match MediaSession::start(
            subscription,
            remote,
            payload_type,
            ssrc,
            self.config.mtu,
            tokio_util::sync::CancellationToken::new(),
            self.media.clone(),
        )
        .await
        {
            Ok(session) => session,
            Err(error) => {
                let _ = self.media.unsubscribe(subscription_id);
                return Err(MediaError::RuntimeUnavailable(format!(
                    "绑定 RTP socket 失败: {error}"
                )));
            }
        };
        let local = session.local;
        self.sessions.lock().await.insert(dialog_id, session);
        Ok(local)
    }

    /// Returns whether a configured MP4 source can provide encoded packets.
    #[must_use]
    pub fn source_available(&self) -> bool {
        let status = self.media.status();
        !matches!(status.source_status, super::MediaSourceStatus::Unconfigured)
            && status.video.is_some()
            && status.last_error.is_none()
    }

    /// Stops a session and releases its Live subscription.
    pub async fn stop(&self, dialog_id: &str) -> bool {
        let session = self.sessions.lock().await.remove(dialog_id);
        if let Some(session) = session {
            let _ = session.stop().await;
            true
        } else {
            false
        }
    }

    /// Releases the ACK gate for a negotiated dialog.
    pub async fn activate(&self, dialog_id: &str) -> bool {
        let activated = self
            .sessions
            .lock()
            .await
            .get(dialog_id)
            .is_some_and(|session| {
                session.activate();
                true
            });
        if !activated {
            return false;
        }
        if !matches!(
            self.media.status().source_status,
            super::MediaSourceStatus::Playing
        ) && self.media.play().is_err()
        {
            let _ = self.stop(dialog_id).await;
            return false;
        }
        true
    }

    /// Stops all active sessions during runtime shutdown.
    pub async fn stop_all(&self) {
        let sessions = self
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>();
        for session in sessions {
            let _ = session.stop().await;
        }
    }

    /// Returns the number of active dialogs.
    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Returns whether no dialogs are active.
    pub async fn is_empty(&self) -> bool {
        self.sessions.lock().await.is_empty()
    }
}
