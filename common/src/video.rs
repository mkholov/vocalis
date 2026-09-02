//! Wire format for the screen-demo H.264 video stream: purely the UDP
//! packetization/reassembly, mirroring `audio.rs`'s
//! `encode_audio_packet`/`split_audio_packet`/`SequenceTracker` shape. The
//! actual codec (openh264 encode/decode, screen capture) lives in the `app`
//! crate, since it needs platform capture/GUI glue this crate doesn't have —
//! this module only ever moves opaque bytes around, which is also exactly
//! what lets the teacher relay a presenting student's video on to the class
//! without decoding it (see `teacher::screen::run_screen_relay_receiver`).
//!
//! One encoded video frame is usually much bigger than a single UDP
//! datagram, unlike an Opus audio frame, so it's split across several
//! packets. Each carries a header of `(frame_seq, packet_index, is_last)`;
//! [`FrameReassembler`] collects a frame's packets back into one bitstream
//! once every index up to the one marked `is_last` has arrived, in whatever
//! order they showed up in.
//!
//! A frame that never completes (a packet was dropped) is discarded outright
//! rather than concealed the way [`crate::SequenceTracker`] conceals a lost
//! Opus frame — there's no equivalent of packet-loss concealment for a video
//! slice. Instead the encoder is configured with periodic keyframes (see
//! `app::video`) so the picture self-heals within a second or two, exactly
//! like any lossy video call tolerates an occasional glitch.

pub const VIDEO_HEADER_LEN: usize = 4 + 2 + 1;
/// Comfortably under a LAN's typical 1500-byte MTU once UDP/IP headers and
/// the AEAD nonce+tag (`crypto::NONCE_LEN + crypto::TAG_LEN`) are added on top.
pub const VIDEO_MAX_PAYLOAD: usize = 1200;

/// Splits one encoded frame's bitstream into `VIDEO_MAX_PAYLOAD`-sized
/// packets, each prefixed with `[frame_seq: u32 BE][packet_index: u16
/// BE][is_last: u8]`.
pub fn encode_video_packets(frame_seq: u32, bitstream: &[u8]) -> Vec<Vec<u8>> {
    let chunks: Vec<&[u8]> = if bitstream.is_empty() {
        vec![&[]]
    } else {
        bitstream.chunks(VIDEO_MAX_PAYLOAD).collect()
    };
    let last_index = chunks.len() - 1;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut buf = Vec::with_capacity(VIDEO_HEADER_LEN + chunk.len());
            buf.extend_from_slice(&frame_seq.to_be_bytes());
            buf.extend_from_slice(&(i as u16).to_be_bytes());
            buf.push(u8::from(i == last_index));
            buf.extend_from_slice(chunk);
            buf
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct VideoPacketHeader {
    pub frame_seq: u32,
    pub packet_index: u16,
    pub is_last: bool,
}

/// Splits a raw (already-decrypted) video packet into its header and payload.
pub fn split_video_packet(bytes: &[u8]) -> Option<(VideoPacketHeader, &[u8])> {
    if bytes.len() < VIDEO_HEADER_LEN {
        return None;
    }
    let frame_seq = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
    let packet_index = u16::from_be_bytes(bytes[4..6].try_into().ok()?);
    let is_last = bytes[6] != 0;
    Some((VideoPacketHeader { frame_seq, packet_index, is_last }, &bytes[VIDEO_HEADER_LEN..]))
}

/// True if `a` is a later frame sequence number than `b`, tolerating u32
/// wraparound — the same "is this newer" reasoning `SequenceTracker` applies
/// to audio via `wrapping_sub`, needed here so one late, stale packet from an
/// already-abandoned frame can't reset progress on the frame actually in
/// flight.
fn is_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < u32::MAX / 2
}

/// Reassembles UDP video packets back into complete per-frame H.264
/// bitstreams. See the module doc comment for the loss-handling rationale.
#[derive(Default)]
pub struct FrameReassembler {
    current_seq: Option<u32>,
    packets: Vec<Option<Vec<u8>>>,
    received: usize,
    /// Total packet count for the in-progress frame, known once the packet
    /// marked `is_last` has arrived (it may not be the last one to *arrive*).
    total: Option<usize>,
}

impl FrameReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one received (already decrypted) packet. Returns the complete,
    /// concatenated bitstream once every packet of its frame has arrived.
    pub fn push(&mut self, header: VideoPacketHeader, payload: &[u8]) -> Option<Vec<u8>> {
        match self.current_seq {
            Some(seq) if seq == header.frame_seq => {}
            Some(seq) if !is_newer(header.frame_seq, seq) => {
                // A straggler from a frame we already gave up on (or are
                // still waiting on something newer for) — not worth
                // resetting progress over.
                return None;
            }
            _ => {
                // A newer frame started — whatever was in progress for the
                // old one is abandoned; the next keyframe resyncs the picture.
                self.current_seq = Some(header.frame_seq);
                self.packets.clear();
                self.received = 0;
                self.total = None;
            }
        }

        let index = header.packet_index as usize;
        if index >= self.packets.len() {
            self.packets.resize(index + 1, None);
        }
        if self.packets[index].is_none() {
            self.packets[index] = Some(payload.to_vec());
            self.received += 1;
        }
        if header.is_last {
            self.total = Some(index + 1);
        }

        if self.total == Some(self.received) && self.total == Some(self.packets.len()) {
            let mut out = Vec::new();
            for p in self.packets.drain(..) {
                out.extend(p?);
            }
            self.current_seq = None;
            self.received = 0;
            self.total = None;
            Some(out)
        } else {
            None
        }
    }
}
