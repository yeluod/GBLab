//! RTP packetization for MPEG-PS over UDP.

use std::num::Wrapping;

/// RTP packetizer using a fixed payload type and SSRC for one dialog.
#[derive(Debug)]
pub struct RtpPacketizer {
    sequence: Wrapping<u16>,
    timestamp: u32,
    ssrc: u32,
    payload_type: u8,
    mtu: usize,
}

impl RtpPacketizer {
    /// Creates a packetizer. MTU is clamped to leave room for the RTP header.
    #[must_use]
    pub fn new(sequence: u16, timestamp: u32, ssrc: u32, payload_type: u8, mtu: usize) -> Self {
        Self {
            sequence: Wrapping(sequence),
            timestamp,
            ssrc,
            payload_type: payload_type & 0x7f,
            mtu: mtu.max(13),
        }
    }

    /// Packetizes one PS access unit and marks its final fragment.
    pub fn packetize(&mut self, payload: &[u8]) -> Vec<Vec<u8>> {
        let chunk_size = self.mtu - 12;
        if payload.is_empty() {
            return Vec::new();
        }
        let chunks = payload.chunks(chunk_size).collect::<Vec<_>>();
        let mut packets = Vec::with_capacity(chunks.len());
        for (index, chunk) in chunks.iter().enumerate() {
            let marker = index + 1 == chunks.len();
            let mut packet = Vec::with_capacity(chunk.len() + 12);
            packet.extend_from_slice(&[0x80, self.payload_type | if marker { 0x80 } else { 0 }]);
            packet.extend_from_slice(&self.sequence.0.to_be_bytes());
            packet.extend_from_slice(&self.timestamp.to_be_bytes());
            packet.extend_from_slice(&self.ssrc.to_be_bytes());
            packet.extend_from_slice(chunk);
            packets.push(packet);
            self.sequence += Wrapping(1);
        }
        packets
    }

    /// Advances the RTP timestamp by a 90 kHz duration.
    pub const fn advance_timestamp(&mut self, duration_90khz: u32) {
        self.timestamp = self.timestamp.wrapping_add(duration_90khz);
    }

    /// Sets the RTP timestamp for the next access unit.
    pub const fn set_timestamp(&mut self, timestamp: u32) {
        self.timestamp = timestamp;
    }

    /// Returns the current RTP timestamp.
    #[must_use]
    pub const fn timestamp(&self) -> u32 {
        self.timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::RtpPacketizer;

    #[test]
    fn packetizer_should_split_payload_and_set_marker_only_on_last_packet() {
        let mut packetizer = RtpPacketizer::new(65_535, 10, 7, 96, 20);
        let packets = packetizer.packetize(&[1; 25]);
        assert_eq!(packets.len(), 4);
        assert_eq!(u16::from_be_bytes([packets[0][2], packets[0][3]]), 65_535);
        assert_eq!(packets[0][1] & 0x80, 0);
        assert_eq!(packets[3][1] & 0x80, 0x80);
        assert_eq!(u16::from_be_bytes([packets[1][2], packets[1][3]]), 0);
    }

    #[test]
    fn packetizer_should_wrap_timestamp() {
        let mut packetizer = RtpPacketizer::new(1, u32::MAX, 2, 96, 1400);
        packetizer.advance_timestamp(2);
        assert_eq!(packetizer.timestamp(), 1);
    }
}
