//! Minimal MPEG-PS muxing for GB28181 video playback.

use super::{EncodedMediaCodec, EncodedMediaPacket, VideoCodec};

const VIDEO_STREAM_ID: u8 = 0xe0;
const PROGRAM_MUX_RATE: u32 = 2_257_150;

/// Creates one PS payload containing a pack header, PSM and a video PES.
pub fn mux_video_packet(packet: &EncodedMediaPacket, pts_90khz: u64) -> Option<Vec<u8>> {
    let codec = match packet.codec {
        EncodedMediaCodec::Video(codec) => codec,
        EncodedMediaCodec::Audio(_) => return None,
    };
    let configuration = packet
        .codec_configuration
        .as_deref()
        .filter(|bytes| bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1]));
    let payload_len = packet
        .data
        .len()
        .saturating_add(configuration.map_or(0, <[u8]>::len));
    let mut output = Vec::with_capacity(payload_len + 128);
    let scr_90khz = packet
        .dts
        .or(packet.pts)
        .unwrap_or_else(|| pts_90khz.cast_signed())
        .max(0)
        .cast_unsigned();
    output.extend_from_slice(&pack_header(scr_90khz));
    output.extend_from_slice(&program_stream_map(codec));
    let mut video_payload = Vec::with_capacity(payload_len);
    if packet.is_keyframe
        && let Some(configuration) = configuration
    {
        video_payload.extend_from_slice(configuration);
    }
    video_payload.extend_from_slice(&packet.data);
    output.extend_from_slice(&pes(VIDEO_STREAM_ID, &video_payload, pts_90khz));
    Some(output)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "PS 字段按规范截取固定宽度位段"
)]
fn pack_header(scr_90khz: u64) -> [u8; 14] {
    let scr = scr_90khz & ((1 << 33) - 1);
    let mut result = [0_u8; 14];
    result[..4].copy_from_slice(&[0, 0, 1, 0xba]);
    result[4] = 0x44 | (((scr >> 30) as u8 & 0x07) << 3) | (((scr >> 28) as u8) & 0x03);
    result[5] = (scr >> 20) as u8;
    // This byte spans both 15-bit SCR groups: SCR[19..15], marker,
    // SCR[14..13].
    result[6] = (((scr >> 12) as u8) & 0xf8) | 0x04 | (((scr >> 13) as u8) & 0x03);
    result[7] = (scr >> 5) as u8;
    // SCR[4..0] occupies bits 7..3; bit 2 is the marker before the
    // nine-bit SCR extension (zero here).
    result[8] = (((scr as u8) << 3) & 0xf8) | 0x04;
    // SCR extension is zero. Its trailing marker bit remains mandatory.
    result[9] = 0x01;
    // Encode a 22-bit program_mux_rate followed by the two required marker
    // bits, reserved bits and zero stuffing length.
    result[10] = (PROGRAM_MUX_RATE >> 14) as u8;
    result[11] = (PROGRAM_MUX_RATE >> 6) as u8;
    result[12] = ((PROGRAM_MUX_RATE << 2) as u8 & 0xfc) | 0x03;
    result[13] = 0xf8;
    result
}

fn program_stream_map(codec: VideoCodec) -> Vec<u8> {
    let stream_type = match codec {
        VideoCodec::H264 => 0x1b,
        VideoCodec::H265 => 0x24,
    };
    let mut map = vec![
        0,
        0,
        1,
        0xbc,
        0,
        14,
        0xe0,
        0xff,
        0,
        0,
        0,
        4,
        stream_type,
        0xe0,
        0,
        0,
    ];
    let crc = crc32_mpeg2(&map[6..]);
    map.extend_from_slice(&crc.to_be_bytes());
    map
}

fn pes(stream_id: u8, payload: &[u8], pts_90khz: u64) -> Vec<u8> {
    let header_data = encode_pts(pts_90khz);
    let pes_length = 3 + header_data.len() + payload.len();
    let mut result = Vec::with_capacity(9 + payload.len());
    result.extend_from_slice(&[0, 0, 1, stream_id]);
    let encoded_length = if pes_length > usize::from(u16::MAX) {
        0
    } else {
        u16::try_from(pes_length).unwrap_or(0)
    };
    result.extend_from_slice(&encoded_length.to_be_bytes());
    result.extend_from_slice(&[0x80, 0x80, 5]);
    result.extend_from_slice(&header_data);
    result.extend_from_slice(payload);
    result
}

fn crc32_mpeg2(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "PTS 按规范截取固定宽度位段"
)]
const fn encode_pts(value: u64) -> [u8; 5] {
    let value = value & ((1 << 33) - 1);
    [
        0x21 | (((value >> 30) as u8 & 0x07) << 1),
        (value >> 22) as u8,
        (((value >> 15) as u8) << 1) | 1,
        (value >> 7) as u8,
        ((value as u8) << 1) | 1,
    ]
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{EncodedMediaCodec, EncodedMediaPacket, VideoCodec, mux_video_packet};
    use crate::media::{MediaTimeBase, MediaTrackKind};

    fn packet(codec: VideoCodec) -> EncodedMediaPacket {
        EncodedMediaPacket {
            track: MediaTrackKind::Video,
            codec: EncodedMediaCodec::Video(codec),
            data: Bytes::from_static(b"frame"),
            pts: Some(0),
            dts: Some(0),
            duration: 3_600,
            time_base: MediaTimeBase::MPEG_CLOCK,
            is_keyframe: true,
            codec_configuration: None,
        }
    }

    #[test]
    fn mux_should_emit_h264_ps_and_pes_start_codes() {
        let Some(output) = mux_video_packet(&packet(VideoCodec::H264), 90_000) else {
            return;
        };
        assert_eq!(&output[..4], &[0, 0, 1, 0xba]);
        assert!(output.windows(4).any(|window| window == [0, 0, 1, 0xbc]));
        assert!(output.windows(4).any(|window| window == [0, 0, 1, 0xe0]));
    }

    #[test]
    fn mux_should_ignore_audio_packets() {
        let mut packet = packet(VideoCodec::H264);
        packet.track = MediaTrackKind::Audio;
        packet.codec = EncodedMediaCodec::Audio(crate::media::AudioCodec::Aac);
        assert!(mux_video_packet(&packet, 0).is_none());
    }

    #[test]
    fn pack_header_should_set_mpeg_scr_marker_bits() {
        let mut packet = packet(VideoCodec::H264);
        packet.pts = None;
        packet.dts = None;
        let Some(output) = mux_video_packet(&packet, 0x12345) else {
            return;
        };

        assert_eq!(&output[..4], &[0, 0, 1, 0xba]);
        assert_eq!(
            &output[4..14],
            &[0x44, 0x00, 0x15, 0x1a, 0x2c, 0x01, 0x89, 0xc3, 0xfb, 0xf8]
        );
    }
}
