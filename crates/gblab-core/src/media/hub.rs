//! Bounded non-blocking fan-out for encoded media consumers.

use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::mpsc;

use super::EncodedMediaPacket;

/// Downstream role consuming the single global encoded stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaConsumerKind {
    /// UI preview decoder.
    Preview,
    /// Future local recorder.
    Recorder,
    /// Future GB28181 live media session.
    Live,
}

/// Behavior when a bounded consumer queue is full.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackpressurePolicy {
    /// Drop the newest packet for this consumer and keep the consumer attached.
    DropNewest,
    /// Disconnect a consumer which cannot keep up.
    Disconnect,
}

/// A bounded encoded-stream subscription.
pub struct MediaSubscription {
    /// Stable subscription identifier.
    pub id: u64,
    /// Consumer role.
    pub kind: MediaConsumerKind,
    receiver: mpsc::Receiver<Arc<EncodedMediaPacket>>,
}

impl MediaSubscription {
    /// Receives the next packet asynchronously.
    pub async fn recv(&mut self) -> Option<Arc<EncodedMediaPacket>> {
        self.receiver.recv().await
    }

    /// Attempts to receive without waiting.
    pub fn try_recv(&mut self) -> Result<Arc<EncodedMediaPacket>, mpsc::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

#[derive(Debug)]
struct Consumer {
    kind: MediaConsumerKind,
    sender: mpsc::Sender<Arc<EncodedMediaPacket>>,
    policy: BackpressurePolicy,
    dropped_packets: u64,
}

/// Result of one non-blocking broadcast.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BroadcastReport {
    /// Consumers which accepted the packet.
    pub delivered: usize,
    /// Full consumer queues which dropped this packet.
    pub dropped: usize,
    /// Closed or overloaded consumers removed from the hub.
    pub disconnected: usize,
}

/// Owns bounded queues for every downstream encoded-stream consumer.
#[derive(Debug, Default)]
pub struct MediaStreamHub {
    next_id: u64,
    consumers: BTreeMap<u64, Consumer>,
}

impl MediaStreamHub {
    /// Creates an empty stream hub.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            consumers: BTreeMap::new(),
        }
    }

    /// Adds a bounded consumer.
    pub fn subscribe(
        &mut self,
        kind: MediaConsumerKind,
        capacity: usize,
        policy: BackpressurePolicy,
    ) -> MediaSubscription {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.consumers.insert(
            id,
            Consumer {
                kind,
                sender,
                policy,
                dropped_packets: 0,
            },
        );
        MediaSubscription { id, kind, receiver }
    }

    /// Removes a consumer explicitly.
    pub fn unsubscribe(&mut self, id: u64) -> bool {
        self.consumers.remove(&id).is_some()
    }

    /// Broadcasts without waiting for any consumer.
    pub fn broadcast(&mut self, packet: &Arc<EncodedMediaPacket>) -> BroadcastReport {
        let mut report = BroadcastReport::default();
        self.consumers.retain(|_, consumer| {
            match consumer.sender.try_send(Arc::clone(packet)) {
                Ok(()) => report.delivered = report.delivered.saturating_add(1),
                Err(mpsc::error::TrySendError::Full(_))
                    if consumer.policy == BackpressurePolicy::DropNewest =>
                {
                    consumer.dropped_packets = consumer.dropped_packets.saturating_add(1);
                    report.dropped = report.dropped.saturating_add(1);
                }
                Err(mpsc::error::TrySendError::Full(_) | mpsc::error::TrySendError::Closed(_)) => {
                    report.disconnected = report.disconnected.saturating_add(1);
                    return false;
                }
            }
            true
        });
        report
    }

    /// Current consumer count.
    #[must_use]
    pub fn consumer_count(&self) -> usize {
        self.consumers.len()
    }

    /// Reports whether at least one consumer of the requested role is attached.
    #[must_use]
    pub fn has_consumer(&self, kind: MediaConsumerKind) -> bool {
        self.consumers
            .values()
            .any(|consumer| consumer.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;

    use super::{BackpressurePolicy, MediaConsumerKind, MediaStreamHub};
    use crate::media::{
        EncodedMediaCodec, EncodedMediaPacket, MediaTimeBase, MediaTrackKind, VideoCodec,
    };

    fn packet() -> Arc<EncodedMediaPacket> {
        Arc::new(EncodedMediaPacket {
            track: MediaTrackKind::Video,
            codec: EncodedMediaCodec::Video(VideoCodec::H264),
            data: Bytes::from_static(b"packet"),
            pts: Some(0),
            dts: Some(0),
            duration: 3_600,
            time_base: MediaTimeBase::MPEG_CLOCK,
            is_keyframe: true,
            codec_configuration: None,
        })
    }

    #[test]
    fn slow_preview_should_not_block_other_consumers() {
        let mut hub = MediaStreamHub::new();
        let _preview = hub.subscribe(
            MediaConsumerKind::Preview,
            1,
            BackpressurePolicy::DropNewest,
        );
        let mut recorder = hub.subscribe(
            MediaConsumerKind::Recorder,
            4,
            BackpressurePolicy::Disconnect,
        );
        let first_packet = packet();
        let first = hub.broadcast(&first_packet);
        let second_packet = packet();
        let second = hub.broadcast(&second_packet);

        assert_eq!(first.delivered, 2);
        assert_eq!(second.dropped, 1);
        assert!(recorder.try_recv().is_ok());
        assert!(recorder.try_recv().is_ok());
    }

    #[test]
    fn disconnected_consumer_should_be_removed() {
        let mut hub = MediaStreamHub::new();
        let consumer = hub.subscribe(MediaConsumerKind::Live, 1, BackpressurePolicy::DropNewest);
        drop(consumer);

        let packet = packet();
        let report = hub.broadcast(&packet);

        assert_eq!(report.disconnected, 1);
        assert_eq!(hub.consumer_count(), 0);
    }

    #[test]
    fn overloaded_strict_consumer_should_disconnect_without_affecting_preview() {
        let mut hub = MediaStreamHub::new();
        let mut preview = hub.subscribe(
            MediaConsumerKind::Preview,
            4,
            BackpressurePolicy::DropNewest,
        );
        let _live = hub.subscribe(MediaConsumerKind::Live, 1, BackpressurePolicy::Disconnect);
        let first_packet = packet();
        let first = hub.broadcast(&first_packet);
        let second_packet = packet();
        let second = hub.broadcast(&second_packet);

        assert_eq!(first.delivered, 2);
        assert_eq!(second.disconnected, 1);
        assert_eq!(hub.consumer_count(), 1);
        assert!(preview.try_recv().is_ok());
        assert!(preview.try_recv().is_ok());
    }

    #[test]
    fn unsubscribe_should_remove_only_the_requested_consumer() {
        let mut hub = MediaStreamHub::new();
        let preview = hub.subscribe(
            MediaConsumerKind::Preview,
            1,
            BackpressurePolicy::DropNewest,
        );
        let recorder = hub.subscribe(
            MediaConsumerKind::Recorder,
            1,
            BackpressurePolicy::Disconnect,
        );

        assert!(hub.unsubscribe(preview.id));
        assert!(!hub.unsubscribe(preview.id));
        assert_eq!(hub.consumer_count(), 1);
        drop(recorder);
    }
}
