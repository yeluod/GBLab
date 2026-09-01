//! Restricted, tolerant SDP support for GB28181 real-time playback.

use std::{fmt, net::IpAddr, str::FromStr};

use thiserror::Error;

/// SDP parsing or generation failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SdpError {
    /// A required SDP field is missing.
    #[error("SDP 缺少必要字段: {0}")]
    Missing(&'static str),
    /// A field contains an invalid value.
    #[error("SDP 字段无效: {0}")]
    Invalid(String),
    /// The offer does not describe a supported video stream.
    #[error("SDP 不包含可用的视频媒体描述")]
    UnsupportedVideo,
    /// The offer uses a media direction that cannot be answered by the simulator.
    #[error("SDP 媒体方向不受支持: {0}")]
    UnsupportedDirection(String),
    /// The offer uses a transport profile that cannot be answered by the simulator.
    #[error("SDP 传输协议不受支持: {0}")]
    UnsupportedTransport(String),
}

/// Video codecs that can be advertised by a GB28181 SDP offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoCodec {
    /// MPEG program stream carried in RTP, the codec used by this simulator.
    Ps,
    /// H.264 elementary stream.
    H264,
    /// H.265 elementary stream.
    H265,
    /// MPEG-4 video.
    Mpeg4,
    /// An otherwise valid but unsupported codec.
    Other,
}

/// One `a=rtpmap` entry from a video media description.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodecDescription {
    /// RTP payload type.
    pub payload_type: u8,
    /// Advertised codec.
    pub codec: VideoCodec,
    /// RTP clock rate.
    pub clock_rate: u32,
}

/// Parsed GB28181 video offer after local capability negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdpOffer {
    /// Address where the platform expects RTP packets.
    pub remote_address: IpAddr,
    /// RTP port where the platform expects packets.
    pub remote_port: u16,
    /// Payload type selected for the negotiated PS stream.
    pub payload_type: u8,
    /// All payload types listed by the platform in the video m-line.
    pub offered_payload_types: Vec<u8>,
    /// Parsed codec mappings advertised by the platform.
    pub codecs: Vec<CodecDescription>,
    /// Optional SSRC requested by the platform (`y=` line).
    pub ssrc: Option<u32>,
    /// Whether the selected codec is PS/90000.
    pub ps: bool,
}

impl SdpOffer {
    /// Parses an SDP offer and selects PS independently of codec ordering.
    ///
    /// The parser intentionally supports the constrained GB28181 subset used
    /// for playback. Unknown media attributes are ignored, while malformed
    /// values in fields that affect negotiation are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`SdpError`] when required connection, media, direction, or
    /// codec negotiation fields are malformed or unsupported.
    #[expect(
        clippy::too_many_lines,
        reason = "SDP parsing keeps media-level context and negotiation in one boundary"
    )]
    pub fn parse(input: &str) -> Result<Self, SdpError> {
        let mut session_address = None;
        let mut media_address = None;
        let mut remote_port = None;
        let mut offered_payload_types = Vec::new();
        let mut codecs = Vec::new();
        let mut ssrc = None;
        let mut in_video = false;
        let mut media_section_seen = false;

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(value) = line.strip_prefix("c=") {
                let mut fields = value.split_whitespace();
                let network = fields.next().unwrap_or_default();
                let address_type = fields.next().unwrap_or_default();
                let address = fields.next().ok_or(SdpError::Missing("c="))?;
                if !network.eq_ignore_ascii_case("IN") || !address_type.eq_ignore_ascii_case("IP4")
                {
                    if in_video {
                        return Err(SdpError::UnsupportedTransport(value.to_owned()));
                    }
                    continue;
                }
                let address = IpAddr::from_str(address)
                    .map_err(|_| SdpError::Invalid(format!("地址 {address}")))?;
                if in_video {
                    media_address = Some(address);
                } else if !media_section_seen {
                    session_address = Some(address);
                }
                continue;
            }

            if let Some(value) = line.strip_prefix("m=") {
                media_section_seen = true;
                let mut fields = value.split_whitespace();
                let media_type = fields.next().unwrap_or_default();
                if media_type.eq_ignore_ascii_case("video") {
                    let port = fields
                        .next()
                        .ok_or(SdpError::Missing("m=video 端口"))?
                        .split('/')
                        .next()
                        .ok_or(SdpError::Missing("m=video 端口"))?
                        .parse::<u16>()
                        .map_err(|_| SdpError::Invalid(value.to_owned()))?;
                    let profile = fields.next().ok_or(SdpError::Missing("m=video profile"))?;
                    if !profile.eq_ignore_ascii_case("RTP/AVP") {
                        return Err(SdpError::UnsupportedTransport(profile.to_owned()));
                    }
                    offered_payload_types = fields
                        .map(|payload_type| {
                            payload_type
                                .parse::<u8>()
                                .map_err(|_| SdpError::Invalid(value.to_owned()))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if port == 0 || offered_payload_types.is_empty() {
                        return Err(SdpError::UnsupportedVideo);
                    }
                    remote_port = Some(port);
                    in_video = true;
                } else {
                    in_video = false;
                }
                continue;
            }

            if !in_video {
                continue;
            }

            if let Some(value) = line.strip_prefix("a=rtpmap:") {
                let mut fields = value.split_whitespace();
                let payload_type = fields
                    .next()
                    .ok_or_else(|| SdpError::Invalid(value.to_owned()))?;
                let codec_value = fields
                    .next()
                    .ok_or_else(|| SdpError::Invalid(value.to_owned()))?;
                let payload_type = payload_type
                    .parse::<u8>()
                    .map_err(|_| SdpError::Invalid(value.to_owned()))?;
                let mut codec_fields = codec_value.split('/');
                let codec_name = codec_fields.next().unwrap_or_default();
                let clock_rate = codec_fields
                    .next()
                    .ok_or_else(|| SdpError::Invalid(value.to_owned()))?
                    .parse::<u32>()
                    .map_err(|_| SdpError::Invalid(value.to_owned()))?;
                let codec = if codec_name.eq_ignore_ascii_case("ps") {
                    VideoCodec::Ps
                } else if codec_name.eq_ignore_ascii_case("h264") {
                    VideoCodec::H264
                } else if codec_name.eq_ignore_ascii_case("h265")
                    || codec_name.eq_ignore_ascii_case("hevc")
                {
                    VideoCodec::H265
                } else if codec_name.eq_ignore_ascii_case("mpeg4") {
                    VideoCodec::Mpeg4
                } else {
                    VideoCodec::Other
                };
                codecs.push(CodecDescription {
                    payload_type,
                    codec,
                    clock_rate,
                });
            } else if let Some(value) = line.strip_prefix("a=") {
                if value.eq_ignore_ascii_case("sendonly") || value.eq_ignore_ascii_case("inactive")
                {
                    return Err(SdpError::UnsupportedDirection(value.to_owned()));
                }
            } else if let Some(value) = line.strip_prefix("y=") {
                ssrc = Some(
                    value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| SdpError::Invalid(value.to_owned()))?,
                );
            }
        }

        let remote_address = media_address
            .or(session_address)
            .ok_or(SdpError::Missing("c="))?;
        let remote_port = remote_port.ok_or(SdpError::UnsupportedVideo)?;

        let selected_payload_type = codecs
            .iter()
            .find(|codec| {
                codec.codec == VideoCodec::Ps
                    && codec.clock_rate == 90_000
                    && offered_payload_types.contains(&codec.payload_type)
            })
            .map(|codec| codec.payload_type)
            // A large number of GB28181 devices omit rtpmap for the default
            // dynamic PS payload. Keep this fallback explicit and limited.
            .or_else(|| {
                offered_payload_types
                    .contains(&96)
                    .then_some(96)
                    .filter(|_| !codecs.iter().any(|codec| codec.payload_type == 96))
            })
            .ok_or(SdpError::UnsupportedVideo)?;

        Ok(Self {
            remote_address,
            remote_port,
            payload_type: selected_payload_type,
            offered_payload_types,
            codecs,
            ssrc,
            ps: true,
        })
    }
}

/// Generates the SDP answer sent in a 200 response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdpAnswer {
    /// Address advertised by the simulator.
    pub address: IpAddr,
    /// Local RTP port.
    pub port: u16,
    /// Payload type selected from the remote offer.
    pub payload_type: u8,
    /// SSRC used by the simulator.
    pub ssrc: u32,
}

impl fmt::Display for SdpAnswer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v=0\r\no=GBLab 1 1 IN IP4 {}\r\ns=Play\r\nc=IN IP4 {}\r\nt=0 0\r\nm=video {} RTP/AVP {}\r\na=sendonly\r\na=rtpmap:{} PS/90000\r\ny={:010}\r\n",
            self.address, self.address, self.port, self.payload_type, self.payload_type, self.ssrc
        )
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{SdpAnswer, SdpError, SdpOffer, VideoCodec};

    const WVP_OFFER: &str = "v=0\r\no=34020000002000000999 0 0 IN IP4 192.168.10.84\r\ns=Play\r\nc=IN IP4 192.168.10.84\r\nt=0 0\r\nm=video 50116 RTP/AVP 96 97 98 99\r\na=recvonly\r\na=rtpmap:96 PS/90000\r\na=rtpmap:98 H264/90000\r\na=rtpmap:97 MPEG4/90000\r\na=rtpmap:99 H265/90000\r\ny=0702008280\r\n";

    #[test]
    fn offer_should_select_ps_when_ps_is_first() -> Result<(), SdpError> {
        let offer = SdpOffer::parse(WVP_OFFER)?;
        assert_eq!(offer.payload_type, 96);
        assert_eq!(offer.codecs[0].codec, VideoCodec::Ps);
        assert_eq!(offer.remote_port, 50_116);
        assert_eq!(offer.ssrc, Some(702_008_280));
        Ok(())
    }

    #[test]
    fn offer_should_select_ps_when_ps_is_not_first() -> Result<(), SdpError> {
        let input = WVP_OFFER
            .replace("96 97 98 99", "98 97 99 96")
            .replace(
                "a=rtpmap:96 PS/90000",
                "a=rtpmap:98 H264/90000\r\na=rtpmap:96 PS/90000",
            )
            .replace(
                "\r\na=rtpmap:98 H264/90000\r\na=rtpmap:97",
                "\r\na=rtpmap:97",
            );
        let offer = SdpOffer::parse(&input)?;
        assert_eq!(offer.payload_type, 96);
        Ok(())
    }

    #[test]
    fn offer_should_allow_missing_default_ps_rtpmap_in_compatible_mode() -> Result<(), SdpError> {
        let offer = SdpOffer::parse(
            "c=IN IP4 192.168.10.84\r\nm=video 6000 RTP/AVP 96\r\na=recvonly\r\ny=1\r\n",
        )?;
        assert_eq!(offer.payload_type, 96);
        assert!(offer.codecs.is_empty());
        Ok(())
    }

    #[test]
    fn offer_should_tolerate_extra_spaces_and_other_codec_mappings() -> Result<(), SdpError> {
        let offer = SdpOffer::parse(
            "c=IN IP4 192.168.10.84\r\nm=video 6000 RTP/AVP 96 98\r\na=rtpmap:98   H264/90000\r\n",
        )?;
        assert_eq!(offer.payload_type, 96);
        Ok(())
    }

    #[test]
    fn offer_should_not_use_an_audio_connection_address_for_video() -> Result<(), SdpError> {
        let offer = SdpOffer::parse(
            "c=IN IP4 192.168.10.84\r\nm=audio 6002 RTP/AVP 0\r\nc=IN IP4 192.168.10.85\r\nm=video 6000 RTP/AVP 96\r\na=rtpmap:96 PS/90000\r\n",
        )?;
        assert_eq!(
            offer.remote_address,
            IpAddr::V4(Ipv4Addr::new(192, 168, 10, 84))
        );
        Ok(())
    }

    #[test]
    fn offer_should_reject_when_no_ps_is_offered() {
        let result = SdpOffer::parse(
            "c=IN IP4 127.0.0.1\r\nm=video 6000 RTP/AVP 98\r\na=rtpmap:98 H264/90000\r\n",
        );
        assert!(result.is_err());
    }

    #[test]
    fn offer_should_reject_sendonly_direction() {
        let result = SdpOffer::parse(
            "c=IN IP4 127.0.0.1\r\nm=video 6000 RTP/AVP 96\r\na=sendonly\r\na=rtpmap:96 PS/90000\r\n",
        );
        assert!(matches!(result, Err(SdpError::UnsupportedDirection(_))));
    }

    #[test]
    fn answer_should_contain_negotiated_payload_type() {
        let answer = SdpAnswer {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 5000,
            payload_type: 98,
            ssrc: 1,
        }
        .to_string();
        assert!(answer.contains("m=video 5000 RTP/AVP 98"));
        assert!(answer.contains("a=rtpmap:98 PS/90000"));
    }

    #[test]
    fn answer_should_preserve_ten_digit_ssrc_with_leading_zeroes() {
        let answer = SdpAnswer {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 5000,
            payload_type: 96,
            ssrc: 20_000_001,
        }
        .to_string();
        assert!(answer.contains("y=0020000001\r\n"));
    }
}
