//! Encoded and raw media data exchanged between media pipeline stages.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Rational frame rate preserving common NTSC rates such as 30000/1001.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRate {
    /// Numerator.
    pub numerator: u32,
    /// Denominator.
    pub denominator: u32,
}

impl FrameRate {
    /// Creates a validated rational frame rate.
    #[must_use]
    pub const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            None
        } else {
            Some(Self {
                numerator,
                denominator,
            })
        }
    }

    /// Approximates a finite floating-point rate with the closest denominator up to 1001.
    #[must_use]
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() || value <= 0.0 {
            return None;
        }
        let mut best = None;
        let mut best_error = f64::INFINITY;
        for denominator in 1..=1_001_u32 {
            let numerator = (value * f64::from(denominator)).round();
            if !(1.0..=f64::from(u32::MAX)).contains(&numerator) {
                continue;
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "positive rounded value was range-checked against u32"
            )]
            let numerator = numerator as u32;
            let error = (f64::from(numerator) / f64::from(denominator) - value).abs();
            if error < best_error {
                best_error = error;
                best = Some(Self {
                    numerator,
                    denominator,
                });
            }
        }
        best
    }

    /// Returns the floating-point representation for UI/probing only.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

/// Rational media time base used by integer timestamps.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTimeBase {
    /// Number of seconds represented by one timestamp unit.
    pub numerator: i32,
    /// Timestamp units per numerator seconds.
    pub denominator: i32,
}

impl MediaTimeBase {
    /// The common 90 kHz clock used by the normalized encoded stream.
    pub const MPEG_CLOCK: Self = Self {
        numerator: 1,
        denominator: 90_000,
    };

    /// Creates a validated time base.
    #[must_use]
    pub const fn new(numerator: i32, denominator: i32) -> Option<Self> {
        if numerator > 0 && denominator > 0 {
            Some(Self {
                numerator,
                denominator,
            })
        } else {
            None
        }
    }

    /// Converts an integer timestamp to seconds for presentation only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "UI presentation uses floating point seconds"
    )]
    pub fn seconds(self, timestamp: i64) -> f64 {
        timestamp as f64 * f64::from(self.numerator) / f64::from(self.denominator)
    }

    /// Rescales an integer timestamp without routing through floating point.
    #[must_use]
    pub fn rescale(self, timestamp: i64, target: Self) -> i64 {
        let numerator = i128::from(timestamp)
            .saturating_mul(i128::from(self.numerator))
            .saturating_mul(i128::from(target.denominator));
        let denominator = i128::from(self.denominator).saturating_mul(i128::from(target.numerator));
        let value = if denominator == 0 {
            0
        } else {
            numerator / denominator
        };
        i64::try_from(value).unwrap_or_else(|_| {
            if value.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        })
    }
}

/// Logical encoded track identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaTrackKind {
    /// Encoded video track.
    Video,
    /// Encoded audio track.
    Audio,
}

/// Final encoded video codec.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoCodec {
    /// H.264/AVC.
    H264,
    /// H.265/HEVC.
    H265,
}

/// Final encoded audio codec.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioCodec {
    /// AAC.
    Aac,
    /// G.711 A-law.
    G711a,
    /// G.711 mu-law.
    G711u,
    /// A detected codec which is not currently available to output consumers.
    Other,
}

/// Codec carried by an encoded media packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "track", content = "codec")]
pub enum EncodedMediaCodec {
    /// Encoded video.
    Video(VideoCodec),
    /// Encoded audio.
    Audio(AudioCodec),
}

/// Explicit byte semantics for codec initialization data.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodecConfigurationFormat {
    /// H.264 AVCC extradata from a container.
    H264Avcc,
    /// H.264 Annex-B parameter sets.
    H264AnnexBParameterSets,
    /// H.265 HVCC extradata from a container.
    H265Hvcc,
    /// H.265 Annex-B VPS/SPS/PPS.
    H265AnnexBParameterSets,
    /// AAC `AudioSpecificConfig`.
    AacAsc,
}

/// Describes one encoded track for late subscribers and future muxers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedStreamDescriptor {
    /// Source generation, incremented only when tracks may change.
    pub source_generation: u64,
    /// Timeline generation, incremented on seek/loop without discarding track structure.
    pub timeline_generation: u64,
    /// Logical track.
    pub track: MediaTrackKind,
    /// Encoded codec.
    pub codec: EncodedMediaCodec,
    /// Video width when the track is video.
    pub width: Option<u32>,
    /// Video height when the track is video.
    pub height: Option<u32>,
    /// Frame rate when known.
    pub frame_rate: Option<FrameRate>,
    /// Audio sample rate when the track is audio.
    pub sample_rate: Option<u32>,
    /// Audio channels when the track is audio.
    pub channels: Option<u32>,
    /// Encoded audio bitrate when known.
    pub bitrate: Option<u64>,
    /// Integer timestamp time base.
    pub time_base: MediaTimeBase,
    /// Codec initialization bytes with explicit semantics.
    pub configuration: Option<Vec<u8>>,
    /// Configuration byte format.
    pub configuration_format: Option<CodecConfigurationFormat>,
}

/// Packet ready for recorder, live-session and preview consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedMediaPacket {
    /// Logical track kind, independent from container stream indices.
    pub track: MediaTrackKind,
    /// Codec carried by this packet.
    pub codec: EncodedMediaCodec,
    /// Actual encoded packet bytes.
    pub data: Bytes,
    /// Presentation timestamp in `time_base` units.
    pub pts: Option<i64>,
    /// Decode timestamp in `time_base` units.
    pub dts: Option<i64>,
    /// Packet duration in `time_base` units.
    pub duration: i64,
    /// Integer timestamp time base.
    pub time_base: MediaTimeBase,
    /// Whether this video packet is a random-access point.
    pub is_keyframe: bool,
    /// Optional codec initialization data required by a downstream muxer.
    pub codec_configuration: Option<Bytes>,
}

impl EncodedMediaPacket {
    /// Presentation position in seconds, intended only for UI/status projection.
    #[must_use]
    pub fn position_seconds(&self) -> f64 {
        self.pts
            .or(self.dts)
            .map_or(0.0, |timestamp| self.time_base.seconds(timestamp))
    }
}

/// Raw decoded video frame used between capture/decode and encode/preview stages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawVideoFrame {
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Backend pixel-format name.
    pub pixel_format: String,
    /// Tightly packed frame bytes where applicable.
    pub data: Bytes,
    /// Presentation timestamp.
    pub pts: Option<i64>,
    /// Timestamp time base.
    pub time_base: MediaTimeBase,
}

/// Raw decoded audio frame used before resampling and encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawAudioFrame {
    /// Interleaved or planar bytes as described by `sample_format`.
    pub data: Bytes,
    /// `FFmpeg` sample-format name.
    pub sample_format: String,
    /// Sample rate.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u32,
    /// Presentation timestamp.
    pub pts: Option<i64>,
    /// Timestamp time base.
    pub time_base: MediaTimeBase,
}

#[cfg(test)]
mod tests {
    use super::{MediaTimeBase, VideoCodec};

    #[test]
    fn timestamp_rescale_should_preserve_integer_media_time() {
        let source = MediaTimeBase::new(1, 1_000).unwrap_or(MediaTimeBase::MPEG_CLOCK);

        assert_eq!(source.rescale(1_500, MediaTimeBase::MPEG_CLOCK), 135_000);
        assert_eq!(VideoCodec::H264, VideoCodec::H264);
    }
}
