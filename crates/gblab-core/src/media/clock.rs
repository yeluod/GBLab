//! Monotonic encoded-stream timeline normalization.

use super::{EncodedMediaPacket, MediaTimeBase};

/// Normalizes source timestamps into one continuous 90 kHz session timeline.
#[derive(Debug, Default)]
pub struct MediaClock {
    loop_offset: i64,
    source_epoch: Option<i64>,
    iteration_origin: Option<i64>,
    last_end: i64,
    seek_generation: u64,
}

impl MediaClock {
    /// Creates an empty media clock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            loop_offset: 0,
            source_epoch: None,
            iteration_origin: None,
            last_end: 0,
            seek_generation: 0,
        }
    }

    /// Starts a new source loop while retaining a monotonic output timeline.
    pub const fn begin_loop(&mut self) {
        self.loop_offset = self.last_end;
        self.iteration_origin = self.source_epoch;
    }

    /// Resets a local timeline, as required by independent preview seek sessions.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    /// Starts an independent seek generation while retaining the monotonic output offset.
    pub const fn begin_seek(&mut self) {
        self.seek_generation = self.seek_generation.saturating_add(1);
        self.iteration_origin = self.source_epoch;
    }

    /// Configures the earliest selected stream timestamp as the source epoch.
    ///
    /// MP4 tracks may start at different timestamps.  Supplying the earliest
    /// stream start time before packet delivery preserves their relative offset
    /// even when a negative audio timestamp arrives after the first video packet.
    pub const fn set_source_epoch(&mut self, source_epoch: Option<i64>) {
        self.source_epoch = source_epoch;
        self.iteration_origin = source_epoch;
    }

    /// Returns the generation of the current source timeline.
    #[must_use]
    pub const fn seek_generation(&self) -> u64 {
        self.seek_generation
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
        // Use the earliest valid decode/presentation timestamp as the iteration
        // origin.  Taking PTS first can leave a negative DTS in the normalized
        // session timeline and break downstream PS/RTP timestamp contracts.
        let first_timestamp = match (pts, dts) {
            (Some(pts), Some(dts)) => pts.min(dts),
            (Some(timestamp), None) | (None, Some(timestamp)) => timestamp,
            (None, None) => 0,
        };
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

    /// Converts a normalized MPEG-clock timestamp to seconds for wall-clock pacing.
    #[must_use]
    pub fn timestamp_seconds(timestamp: i64) -> f64 {
        MediaTimeBase::MPEG_CLOCK.seconds(timestamp)
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

    #[test]
    fn normalize_should_keep_negative_pts_and_dts_ordered() {
        let mut clock = MediaClock::new();
        let mut packet = EncodedMediaPacket {
            track: MediaTrackKind::Video,
            codec: EncodedMediaCodec::Video(VideoCodec::H264),
            data: Bytes::from_static(b"frame"),
            pts: Some(-40),
            dts: Some(-80),
            duration: 40,
            time_base: MediaTimeBase::new(1, 1_000).unwrap_or(MediaTimeBase::MPEG_CLOCK),
            is_keyframe: false,
            codec_configuration: None,
        };
        clock.normalize(&mut packet);

        assert_eq!(packet.pts, Some(3_600));
        assert_eq!(packet.dts, Some(0));
        assert!(packet.dts < packet.pts);
    }

    #[test]
    fn configured_source_epoch_should_preserve_later_negative_track_offset() {
        let mut clock = MediaClock::new();
        clock.set_source_epoch(Some(-7_200));
        let mut video = packet(0);
        let mut audio = packet(-80);

        clock.normalize(&mut video);
        clock.normalize(&mut audio);

        assert_eq!(video.pts, Some(7_200));
        assert_eq!(audio.pts, Some(0));
    }

    #[test]
    fn seek_should_increment_generation_without_resetting_loop_offset() {
        let mut clock = MediaClock::new();
        let mut packet = packet(0);
        clock.normalize(&mut packet);
        clock.begin_seek();
        assert_eq!(clock.seek_generation(), 1);
    }
}
