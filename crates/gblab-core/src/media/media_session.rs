//! One cancellable real-time media session.

use std::{net::SocketAddr, sync::Arc};

use tokio::{
    net::UdpSocket,
    sync::watch,
    task::JoinHandle,
    time::{Duration, timeout},
};
use tokio_util::sync::CancellationToken;

use super::{GlobalMediaHandle, MediaSubscription, RtpPacketizer, mux_video_packet};

/// Runtime statistics for one RTP session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MediaSessionStats {
    /// Number of RTP packets sent.
    pub packets_sent: u64,
    /// Number of bytes sent.
    pub bytes_sent: u64,
    /// Number of send errors.
    pub send_errors: u64,
}

/// Handle for a spawned media sender.
pub struct MediaSession {
    /// Remote RTP endpoint.
    pub remote: SocketAddr,
    /// Local RTP endpoint.
    pub local: SocketAddr,
    cancellation: CancellationToken,
    task: Option<JoinHandle<MediaSessionStats>>,
    media: GlobalMediaHandle,
    subscription_id: u64,
    activation: watch::Sender<bool>,
}

impl MediaSession {
    /// Starts reading from a bounded Live subscription and sending PS/RTP.
    #[expect(
        clippy::too_many_arguments,
        reason = "媒体会话启动参数对应一次协商结果"
    )]
    pub async fn start(
        mut subscription: MediaSubscription,
        remote: SocketAddr,
        payload_type: u8,
        ssrc: u32,
        mtu: usize,
        cancellation: CancellationToken,
        media: GlobalMediaHandle,
    ) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let local = socket.local_addr()?;
        let subscription_id = subscription.id;
        let (activation_tx, activation_rx) = watch::channel(false);
        let sender_socket = Arc::clone(&socket);
        let task_cancellation = cancellation.clone();
        let media_for_task = media.clone();
        let task = tokio::spawn(async move {
            let mut packetizer = RtpPacketizer::new(0, 0, ssrc, payload_type, mtu);
            let mut stats = MediaSessionStats::default();
            let mut waiting_for_keyframe = true;
            let mut activation = activation_rx;
            if !*activation.borrow() {
                let activated = tokio::select! {
                    () = task_cancellation.cancelled() => false,
                    result = timeout(Duration::from_secs(8), async {
                        loop {
                            tokio::select! {
                                () = task_cancellation.cancelled() => return false,
                                result = activation.changed() => {
                                    if result.is_err() {
                                        return false;
                                    }
                                    if *activation.borrow() {
                                        return true;
                                    }
                                }
                                packet = subscription.recv() => {
                                    // Drain pre-ACK packets so a slow SIP transaction
                                    // cannot fill the bounded Live queue and disconnect
                                    // the session before media is negotiated.
                                    if packet.is_none() {
                                        return false;
                                    }
                                }
                            }
                        }
                    }) => result.is_ok_and(|activated| activated),
                };
                if !activated {
                    let _ = media_for_task.unsubscribe(subscription_id);
                    return stats;
                }
            }
            while let Some(packet) = tokio::select! {
                () = task_cancellation.cancelled() => None,
                packet = subscription.recv() => packet,
            } {
                // A newly negotiated ZLM session cannot decode an inter-frame
                // until it has received an IDR and its parameter sets.  The
                // subscription may contain packets queued before SIP ACK, so
                // discard those inter-frames and begin the RTP stream at the
                // first random-access point.
                if !accept_media_packet(&mut waiting_for_keyframe, packet.is_keyframe) {
                    continue;
                }
                // RTP and MPEG-PS SCR follow decode order.  Using PTS here makes
                // B-frame sources publish timestamps that move backwards (for
                // example 0, 0.4, 0.2, 0.1), which causes downstream FLV muxers
                // to buffer and render with visible stalls.  The PS muxer
                // receives both PTS and DTS so downstream FLV remuxers retain
                // the source decode order.
                let decode_timestamp = packet.dts.or(packet.pts).unwrap_or_default();
                let decode_timestamp = decode_timestamp.max(0).cast_unsigned();
                let presentation_timestamp =
                    packet.pts.unwrap_or_else(|| decode_timestamp.cast_signed());
                let Some(ps) =
                    mux_video_packet(&packet, presentation_timestamp.max(0).cast_unsigned())
                else {
                    continue;
                };
                packetizer.set_timestamp(u32::try_from(decode_timestamp).unwrap_or(u32::MAX));
                for rtp in packetizer.packetize(&ps) {
                    match sender_socket.send_to(&rtp, remote).await {
                        Ok(bytes) => {
                            stats.packets_sent = stats.packets_sent.saturating_add(1);
                            stats.bytes_sent = stats.bytes_sent.saturating_add(bytes as u64);
                        }
                        Err(_) => stats.send_errors = stats.send_errors.saturating_add(1),
                    }
                }
                packetizer.advance_timestamp(u32::try_from(packet.duration.max(0)).unwrap_or(0));
            }
            let _ = media_for_task.unsubscribe(subscription_id);
            stats
        });
        Ok(Self {
            remote,
            local,
            cancellation,
            task: Some(task),
            media,
            subscription_id,
            activation: activation_tx,
        })
    }

    /// Allows RTP sending after the SIP ACK has been received.
    pub fn activate(&self) {
        let _ = self.activation.send(true);
    }

    /// Requests cancellation and waits for the sender task to finish.
    pub async fn stop(mut self) -> MediaSessionStats {
        self.cancellation.cancel();
        let stats = match self.task.take() {
            Some(task) => task.await.unwrap_or_default(),
            None => MediaSessionStats::default(),
        };
        let _ = self.media.unsubscribe(self.subscription_id);
        stats
    }
}

const fn accept_media_packet(waiting_for_keyframe: &mut bool, is_keyframe: bool) -> bool {
    if *waiting_for_keyframe && !is_keyframe {
        return false;
    }
    *waiting_for_keyframe = false;
    true
}

#[cfg(test)]
mod tests {
    use super::accept_media_packet;

    #[test]
    fn media_session_should_discard_interframes_before_first_keyframe() {
        let mut waiting_for_keyframe = true;
        assert!(!accept_media_packet(&mut waiting_for_keyframe, false));
        assert!(waiting_for_keyframe);
        assert!(accept_media_packet(&mut waiting_for_keyframe, true));
        assert!(!waiting_for_keyframe);
        assert!(accept_media_packet(&mut waiting_for_keyframe, false));
    }
}
