//! Bounded non-blocking fan-out for encoded media consumers.

use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::mpsc;

use super::{
    CodecConfigurationFormat, EncodedMediaCodec, EncodedMediaPacket, EncodedStreamDescriptor,
    MediaTrackKind,
};

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
    /// Latest codec descriptor snapshot available at subscription time.
    pub descriptor: Option<EncodedStreamDescriptor>,
    /// All currently known track descriptors at subscription time.
    pub descriptors: Vec<EncodedStreamDescriptor>,
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
    descriptors: BTreeMap<MediaTrackKind, EncodedStreamDescriptor>,
    source_generation: u64,
    timeline_generation: u64,
    probe: Option<super::Mp4ProbeResult>,
}

impl MediaStreamHub {
    /// Creates an empty stream hub.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            consumers: BTreeMap::new(),
            descriptors: BTreeMap::new(),
            source_generation: 0,
            timeline_generation: 0,
            probe: None,
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
        let descriptors = self.descriptors.values().cloned().collect::<Vec<_>>();
        let descriptor = descriptors.first().cloned();
        MediaSubscription {
            id,
            kind,
            descriptor,
            descriptors,
            receiver,
        }
    }

    /// Removes a consumer explicitly.
    pub fn unsubscribe(&mut self, id: u64) -> bool {
        self.consumers.remove(&id).is_some()
    }

    /// Broadcasts without waiting for any consumer.
    pub fn broadcast(&mut self, packet: &Arc<EncodedMediaPacket>) -> BroadcastReport {
        self.update_descriptor(packet);
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

    /// Returns the number of consumers for one downstream role.
    #[must_use]
    pub fn consumer_count_by_kind(&self, kind: MediaConsumerKind) -> usize {
        self.consumers
            .values()
            .filter(|consumer| consumer.kind == kind)
            .count()
    }

    /// Returns the latest descriptor for a logical track.
    #[must_use]
    pub fn descriptor(&self, track: MediaTrackKind) -> Option<&EncodedStreamDescriptor> {
        self.descriptors.get(&track)
    }

    /// Replaces the source track structure and removes stale descriptors.
    pub fn replace_source(&mut self, probe: super::Mp4ProbeResult) {
        self.source_generation = self.source_generation.saturating_add(1);
        self.timeline_generation = 0;
        self.descriptors.clear();
        self.probe = Some(probe);
    }

    /// Starts a new seek/loop timeline while retaining track descriptors.
    pub fn begin_timeline(&mut self) {
        self.timeline_generation = self.timeline_generation.saturating_add(1);
        for descriptor in self.descriptors.values_mut() {
            descriptor.timeline_generation = self.timeline_generation;
        }
    }

    fn update_descriptor(&mut self, packet: &EncodedMediaPacket) {
        let previous = self.descriptors.get(&packet.track);
        let configuration = packet
            .codec_configuration
            .clone()
            .map(|bytes| bytes.to_vec())
            .or_else(|| annex_b_parameter_sets(packet))
            .or_else(|| previous.and_then(|descriptor| descriptor.configuration.clone()));
        let configuration_format = configuration
            .as_deref()
            .and_then(|bytes| configuration_format(packet.codec, bytes));
        let (width, height, frame_rate, sample_rate, channels, bitrate) = match packet.track {
            MediaTrackKind::Video => {
                self.probe
                    .as_ref()
                    .map_or((None, None, None, None, None, None), |probe| {
                        (
                            Some(probe.video.width),
                            Some(probe.video.height),
                            super::FrameRate::from_f64(probe.video.frames_per_second),
                            None,
                            None,
                            probe.video.bitrate,
                        )
                    })
            }
            MediaTrackKind::Audio => self
                .probe
                .as_ref()
                .and_then(|probe| probe.audio.as_ref())
                .map_or((None, None, None, None, None, None), |audio| {
                    (
                        None,
                        None,
                        None,
                        Some(audio.sample_rate),
                        Some(audio.channels),
                        audio.bitrate,
                    )
                }),
        };
        self.descriptors.insert(
            packet.track,
            EncodedStreamDescriptor {
                source_generation: self.source_generation,
                timeline_generation: self.timeline_generation,
                track: packet.track,
                codec: packet.codec,
                width,
                height,
                frame_rate,
                sample_rate,
                channels,
                bitrate,
                time_base: packet.time_base,
                configuration,
                configuration_format,
            },
        );
    }
}

fn annex_b_parameter_sets(packet: &EncodedMediaPacket) -> Option<Vec<u8>> {
    if !packet.is_keyframe {
        return None;
    }
    let wanted = |nal: &[u8]| match packet.codec {
        EncodedMediaCodec::Video(super::VideoCodec::H264) => {
            nal.first().is_some_and(|byte| matches!(byte & 0x1f, 7 | 8))
        }
        EncodedMediaCodec::Video(super::VideoCodec::H265) => nal
            .first()
            .is_some_and(|byte| matches!((byte >> 1) & 0x3f, 32..=34)),
        _ => false,
    };
    let data = packet.data.as_ref();
    let starts = annex_b_starts(data);
    let mut configuration = Vec::new();
    for (index, &(offset, prefix_len)) in starts.iter().enumerate() {
        let nal_start = offset + prefix_len;
        let nal_end = starts.get(index + 1).map_or(data.len(), |next| next.0);
        if nal_start < nal_end && wanted(&data[nal_start..nal_end]) {
            configuration.extend_from_slice(&[0, 0, 0, 1]);
            configuration.extend_from_slice(&data[nal_start..nal_end]);
        }
    }
    (!configuration.is_empty()).then_some(configuration)
}

fn annex_b_starts(data: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        if data[index..].starts_with(&[0, 0, 1]) {
            starts.push((index, 3));
            index += 3;
        } else if data[index..].starts_with(&[0, 0, 0, 1]) {
            starts.push((index, 4));
            index += 4;
        } else {
            index += 1;
        }
    }
    starts
}

fn configuration_format(
    codec: EncodedMediaCodec,
    bytes: &[u8],
) -> Option<CodecConfigurationFormat> {
    match codec {
        EncodedMediaCodec::Video(super::VideoCodec::H264) => Some(if bytes.first() == Some(&1) {
            CodecConfigurationFormat::H264Avcc
        } else {
            CodecConfigurationFormat::H264AnnexBParameterSets
        }),
        EncodedMediaCodec::Video(super::VideoCodec::H265) => Some(if bytes.first() == Some(&1) {
            CodecConfigurationFormat::H265Hvcc
        } else {
            CodecConfigurationFormat::H265AnnexBParameterSets
        }),
        EncodedMediaCodec::Audio(super::AudioCodec::Aac) => Some(CodecConfigurationFormat::AacAsc),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;

    use super::{BackpressurePolicy, MediaConsumerKind, MediaStreamHub};
    use crate::media::{
        AudioCodec, EncodedMediaCodec, EncodedMediaPacket, MediaTimeBase, MediaTrackKind,
        Mp4ProbeResult, VideoCodec, VideoStreamInfo,
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

    #[test]
    fn late_subscriber_should_receive_latest_codec_descriptor_snapshot() {
        let mut hub = MediaStreamHub::new();
        let mut packet = packet();
        Arc::make_mut(&mut packet).codec_configuration = Some(Bytes::from_static(&[1, 2, 3]));
        let _ = hub.broadcast(&packet);

        let subscription =
            hub.subscribe(MediaConsumerKind::Live, 2, BackpressurePolicy::Disconnect);
        assert_eq!(subscription.descriptors.len(), 1);
        assert_eq!(
            subscription
                .descriptor
                .as_ref()
                .and_then(|d| d.configuration_format),
            Some(crate::media::CodecConfigurationFormat::H264Avcc)
        );
    }

    #[test]
    fn normal_packets_should_not_erase_cached_configuration() {
        let mut hub = MediaStreamHub::new();
        let mut configured = packet();
        Arc::make_mut(&mut configured).codec_configuration = Some(Bytes::from_static(&[1, 2, 3]));
        let _ = hub.broadcast(&configured);
        for _ in 0..8 {
            let _ = hub.broadcast(&packet());
        }

        assert_eq!(
            hub.descriptor(MediaTrackKind::Video)
                .and_then(|descriptor| descriptor.configuration.as_deref()),
            Some([1, 2, 3].as_slice())
        );
    }

    #[test]
    fn h264_and_h265_keyframes_should_supply_annex_b_parameter_sets() {
        let mut hub = MediaStreamHub::new();
        let mut h264 = packet();
        Arc::make_mut(&mut h264).data =
            Bytes::from_static(&[0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 2, 0, 0, 1, 0x65, 3]);
        let _ = hub.broadcast(&h264);
        let h264_configuration = hub
            .descriptor(MediaTrackKind::Video)
            .and_then(|descriptor| descriptor.configuration.as_deref());
        assert_eq!(
            h264_configuration,
            Some([0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2].as_slice())
        );

        let mut h265 = packet();
        let h265 = Arc::make_mut(&mut h265);
        h265.codec = EncodedMediaCodec::Video(VideoCodec::H265);
        h265.data = Bytes::from_static(&[
            0, 0, 1, 0x40, 1, 0, 0, 1, 0x42, 2, 0, 0, 1, 0x44, 3, 0, 0, 1, 0x26, 4,
        ]);
        let _ = hub.broadcast(&h265.clone().into());
        assert!(
            hub.descriptor(MediaTrackKind::Video)
                .and_then(|descriptor| descriptor.configuration.as_ref())
                .is_some_and(|configuration| configuration.len() == 18)
        );
    }

    #[test]
    fn source_replacement_should_remove_stale_track_descriptors() {
        let mut hub = MediaStreamHub::new();
        let mut video = packet();
        Arc::make_mut(&mut video).codec_configuration = Some(Bytes::from_static(&[1, 2]));
        let _ = hub.broadcast(&video);
        let mut audio = (*packet()).clone();
        audio.track = MediaTrackKind::Audio;
        audio.codec = EncodedMediaCodec::Audio(AudioCodec::Aac);
        audio.codec_configuration = Some(Bytes::from_static(&[0x12, 0x10]));
        let _ = hub.broadcast(&Arc::new(audio));

        hub.replace_source(Mp4ProbeResult {
            file_path: "camera".to_owned(),
            video: VideoStreamInfo {
                codec: VideoCodec::H265,
                width: 1920,
                height: 1080,
                frames_per_second: 25.0,
                bitrate: Some(4_000_000),
                duration_seconds: None,
            },
            audio: None,
            duration_seconds: None,
            bitrate: None,
        });

        assert!(hub.descriptor(MediaTrackKind::Video).is_none());
        assert!(hub.descriptor(MediaTrackKind::Audio).is_none());
        let mut replacement_packet = (*packet()).clone();
        replacement_packet.codec = EncodedMediaCodec::Video(VideoCodec::H265);
        let _ = hub.broadcast(&Arc::new(replacement_packet));
        let Some(descriptor) = hub.descriptor(MediaTrackKind::Video) else {
            return;
        };
        assert_eq!(descriptor.width, Some(1920));
        assert_eq!(descriptor.height, Some(1080));
        assert_eq!(descriptor.bitrate, Some(4_000_000));
    }
}
