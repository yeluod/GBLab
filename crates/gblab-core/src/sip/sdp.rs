//! Restricted SDP support for GB28181 real-time playback.

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
}

/// Parsed GB28181 video offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdpOffer {
    /// Address where the platform expects RTP packets.
    pub remote_address: IpAddr,
    /// RTP port where the platform expects packets.
    pub remote_port: u16,
    /// Negotiated payload type, normally 96 for PS.
    pub payload_type: u8,
    /// Optional SSRC requested by the platform (`y=` line).
    pub ssrc: Option<u32>,
    /// Whether the offer advertises PS/90000.
    pub ps: bool,
}

impl SdpOffer {
    /// Parses the constrained SDP offer used by GB28181 playback.
    ///
    /// # Errors
    ///
    /// Returns [`SdpError`] when the offer is malformed or does not advertise PS video.
    pub fn parse(input: &str) -> Result<Self, SdpError> {
        let mut session_address = None;
        let mut media_address = None;
        let mut remote_port = None;
        let mut payload_type = None;
        let mut ps = false;
        let mut ssrc = None;
        let mut in_video = false;

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if let Some(value) = line.strip_prefix("c=IN IP4 ") {
                let address = IpAddr::from_str(value.trim())
                    .map_err(|_| SdpError::Invalid(format!("地址 {value}")))?;
                if in_video {
                    media_address = Some(address);
                } else {
                    session_address = Some(address);
                }
            } else if let Some(value) = line.strip_prefix("m=video ") {
                let mut fields = value.split_whitespace();
                let port = fields
                    .next()
                    .ok_or(SdpError::Missing("m=video 端口"))?
                    .parse::<u16>()
                    .map_err(|_| SdpError::Invalid(value.to_owned()))?;
                let profile = fields.next().ok_or(SdpError::Missing("m=video profile"))?;
                let pt = fields
                    .next()
                    .ok_or(SdpError::Missing("m=video payload type"))?
                    .parse::<u8>()
                    .map_err(|_| SdpError::Invalid(value.to_owned()))?;
                if profile != "RTP/AVP" {
                    return Err(SdpError::Invalid(format!("不支持 profile {profile}")));
                }
                if port == 0 {
                    return Err(SdpError::Invalid("RTP 端口不能为 0".to_owned()));
                }
                remote_port = Some(port);
                payload_type = Some(pt);
                in_video = true;
            } else if in_video {
                if let Some(value) = line.strip_prefix("a=rtpmap:") {
                    let (_, codec) = value
                        .split_once(' ')
                        .ok_or_else(|| SdpError::Invalid(value.to_owned()))?;
                    ps = codec.eq_ignore_ascii_case("ps/90000");
                } else if let Some(value) = line.strip_prefix("y=") {
                    ssrc = Some(
                        value
                            .trim()
                            .parse::<u32>()
                            .map_err(|_| SdpError::Invalid(value.to_owned()))?,
                    );
                }
            }
        }

        let remote_address = media_address
            .or(session_address)
            .ok_or(SdpError::Missing("c="))?;
        let remote_port = remote_port.ok_or(SdpError::UnsupportedVideo)?;
        let payload_type = payload_type.ok_or(SdpError::UnsupportedVideo)?;
        if !ps {
            return Err(SdpError::UnsupportedVideo);
        }
        Ok(Self {
            remote_address,
            remote_port,
            payload_type,
            ssrc,
            ps,
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
    /// Payload type.
    pub payload_type: u8,
    /// SSRC used by the simulator.
    pub ssrc: u32,
}

impl fmt::Display for SdpAnswer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v=0\r\no=GBLab 1 1 IN IP4 {}\r\ns=Play\r\nc=IN IP4 {}\r\nt=0 0\r\nm=video {} RTP/AVP {}\r\na=sendonly\r\na=rtpmap:{} PS/90000\r\ny={}\r\n",
            self.address, self.address, self.port, self.payload_type, self.payload_type, self.ssrc
        )
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{SdpAnswer, SdpOffer};

    #[test]
    fn offer_should_parse_video_endpoint_and_ssrc() {
        let Ok(offer) = SdpOffer::parse(
            "v=0\r\nc=IN IP4 192.168.10.91\r\nm=video 6000 RTP/AVP 96\r\na=rtpmap:96 PS/90000\r\ny=100000001\r\n",
        ) else {
            return;
        };
        assert_eq!(
            offer.remote_address,
            IpAddr::V4(Ipv4Addr::new(192, 168, 10, 91))
        );
        assert_eq!(offer.remote_port, 6000);
        assert_eq!(offer.payload_type, 96);
        assert_eq!(offer.ssrc, Some(100_000_001));
    }

    #[test]
    fn offer_should_reject_non_ps_video() {
        let result = SdpOffer::parse(
            "c=IN IP4 127.0.0.1\r\nm=video 6000 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\n",
        );
        assert!(result.is_err());
    }

    #[test]
    fn answer_should_contain_sendonly_ps_description() {
        let answer = SdpAnswer {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 5000,
            payload_type: 96,
            ssrc: 1,
        }
        .to_string();
        assert!(answer.contains("m=video 5000 RTP/AVP 96"));
        assert!(answer.contains("a=sendonly"));
        assert!(answer.contains("a=rtpmap:96 PS/90000"));
    }
}
