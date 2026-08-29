//! Monotonic encoded-stream timeline normalization.

use super::{EncodedMediaPacket, MediaTimeBase};

/// Normalizes source timestamps into one continuous 90 kHz session timeline.
#[derive(Debug, Default)]
pub struct MediaClock {
    loop_offset: i64,
    iteration_origin: Option<i64>,
    last_end: i64,
}

impl MediaClock {
    /// Creates an empty media clock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            loop_offset: 0,
            iteration_origin: None,
            last_end: 0,
        }
    }

    /// Starts a new source loop while retaining a monotonic output timeline.
    pub const fn begin_loop(&mut self) {
        self.loop_offset = self.last_end;
        self.iteration_origin = None;
    }

    /// Resets a local timeline, as required by independent preview seek sessions.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    /// Converts packet timestamps to the shared clock and keeps them monotonic across loops.
    pub fn normalize(&mut self, packet: &mut EncodedMediaPacket) {
        let source_time_base = packet.time_base;
        let mut pts = packet
            .pts
            .map(|value| source_time_base.rescale(value, MediaTimeBase::MPEG_CLOCK));
        let mut dts = packet
            .dts
            .map(|value| source_time_base.rescale(value, MediaTimeBase::MPEG_CLOCK));
        let duration = source_time_base
            .rescale(packet.duration.max(0), MediaTimeBase::MPEG_CLOCK)
            .max(0);
        let first_timestamp = pts.or(dts).unwrap_or(0);
        let origin = *self.iteration_origin.get_or_insert(first_timestamp);
        pts = pts.map(|value| {
            value
                .saturating_sub(origin)
                .saturating_add(self.loop_offset)
        });
        dts = dts.map(|value| {
            value
                .saturating_sub(origin)
                .saturating_add(self.loop_offset)
        });
        let packet_end = pts
            .or(dts)
            .unwrap_or(self.last_end)
            .saturating_add(duration.max(1));
        self.last_end = self.last_end.max(packet_end);
        packet.pts = pts;
        packet.dts = dts;
        packet.duration = duration;
        packet.time_base = MediaTimeBase::MPEG_CLOCK;
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::MediaClock;
    use crate::media::{
        EncodedMediaCodec, EncodedMediaPacket, MediaTimeBase, MediaTrackKind, VideoCodec,
    };

    fn packet(pts: i64) -> EncodedMediaPacket {
        EncodedMediaPacket {
            track: MediaTrackKind::Video,
            codec: EncodedMediaCodec::Video(VideoCodec::H264),
            data: Bytes::from_static(b"frame"),
            pts: Some(pts),
            dts: Some(pts),
            duration: 40,
            time_base: MediaTimeBase::new(1, 1_000).unwrap_or(MediaTimeBase::MPEG_CLOCK),
            is_keyframe: true,
            codec_configuration: None,
        }
    }

    #[test]
    fn loop_should_continue_after_previous_packet_end() {
        let mut clock = MediaClock::new();
        let mut first = packet(0);
        let mut last = packet(960);
        clock.normalize(&mut first);
        clock.normalize(&mut last);
        clock.begin_loop();
        let mut loop_first = packet(0);
        clock.normalize(&mut loop_first);

        assert_eq!(first.pts, Some(0));
        assert_eq!(last.pts, Some(86_400));
        assert_eq!(loop_first.pts, Some(90_000));
    }
}
