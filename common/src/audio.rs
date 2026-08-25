//! Audio pipeline shared by every voice stream in the app (mic broadcast, group/peer
//! audio, individual student listen-in): mono PCM, resampled to a fixed rate and
//! compressed with Opus before it ever touches the network.
//!
//! Wire format for one UDP packet: `[seq: u32 BE][opus payload bytes]`.

use anyhow::{Context, Result};
pub use audiopus::coder::{Decoder, Encoder};
use audiopus::{Application, Channels, SampleRate};

/// Every stream is resampled to this rate before encoding — one of the few rates
/// Opus itself accepts, and low enough to keep bandwidth and CPU use trivial on a
/// classroom LAN.
pub const OPUS_SAMPLE_RATE: u32 = 16_000;
/// 20ms frames: a standard Opus frame size and a reasonable latency/overhead tradeoff.
pub const FRAME_SAMPLES: usize = 320;
/// Cap on how many consecutive missing frames we'll ask Opus to conceal for. Beyond
/// this it's a real disconnect, not a dropped packet, so don't synthesize audio for it.
const MAX_CONCEALED_FRAMES: u32 = 10;

pub const AUDIO_HEADER_LEN: usize = 4;

pub fn new_encoder() -> Result<Encoder> {
    Encoder::new(SampleRate::Hz16000, Channels::Mono, Application::Voip)
        .context("creating Opus encoder")
}

pub fn new_decoder() -> Result<Decoder> {
    Decoder::new(SampleRate::Hz16000, Channels::Mono).context("creating Opus decoder")
}

pub fn encode_audio_packet(seq: u32, opus_payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(AUDIO_HEADER_LEN + opus_payload.len());
    buf.extend_from_slice(&seq.to_be_bytes());
    buf.extend_from_slice(opus_payload);
    buf
}

/// Splits a raw UDP datagram into its sequence number and Opus payload.
pub fn split_audio_packet(bytes: &[u8]) -> Option<(u32, &[u8])> {
    if bytes.len() < AUDIO_HEADER_LEN {
        return None;
    }
    let seq = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
    Some((seq, &bytes[AUDIO_HEADER_LEN..]))
}

/// Decodes one Opus payload into exactly [`FRAME_SAMPLES`] i16 samples.
pub fn decode_frame(decoder: &mut Decoder, payload: &[u8]) -> Result<Vec<i16>> {
    let mut out = vec![0i16; FRAME_SAMPLES];
    let n = decoder
        .decode(Some(payload), &mut out, false)
        .context("decoding Opus frame")?;
    out.truncate(n);
    Ok(out)
}

/// Asks Opus to synthesize one lost frame from decoder state (packet loss concealment)
/// instead of leaving a hole that would otherwise surface as a click/gap.
fn conceal_frame(decoder: &mut Decoder) -> Result<Vec<i16>> {
    let mut out = vec![0i16; FRAME_SAMPLES];
    let n = decoder
        .decode(None::<&[u8]>, &mut out, false)
        .context("concealing lost Opus frame")?;
    out.truncate(n);
    Ok(out)
}

/// Tracks the expected next sequence number for one incoming audio stream so gaps
/// (dropped UDP packets) can be filled in with Opus packet-loss concealment instead
/// of silence, which is what actually produces an audible click/gap.
#[derive(Default)]
pub struct SequenceTracker {
    expected: Option<u32>,
}

impl SequenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call for every packet as it arrives, in arrival order. Returns concealed frames
    /// for any gap detected before this packet, followed by this packet's own decoded
    /// frame — i.e. everything that should be appended to the playback queue, in order.
    pub fn decode(&mut self, decoder: &mut Decoder, seq: u32, payload: &[u8]) -> Vec<i16> {
        let mut out = Vec::new();
        if let Some(expected) = self.expected {
            let missing = seq.wrapping_sub(expected);
            if missing > 0 && missing <= MAX_CONCEALED_FRAMES {
                for _ in 0..missing {
                    if let Ok(samples) = conceal_frame(decoder) {
                        out.extend(samples);
                    }
                }
            }
        }
        self.expected = Some(seq.wrapping_add(1));
        if let Ok(samples) = decode_frame(decoder, payload) {
            out.extend(samples);
        }
        out
    }
}

/// Simple stateful resampler for mono i16 PCM: linear interpolation plus a one-pole
/// low-pass filter that kicks in when downsampling, to tame the aliasing hiss/noise
/// naive decimation would otherwise dump straight into the audible band. Not
/// audiophile-grade, but a real fix for the crackle a raw linear resample produces
/// when e.g. a 48kHz mic is dropped straight to 16kHz.
pub struct Resampler {
    from_rate: u32,
    to_rate: u32,
    pos: f64,
    buffer: Vec<i16>,
    lpf_state: f64,
    /// One-pole filter coefficient; 1.0 means "no filtering" (upsampling / equal rates
    /// don't introduce aliasing, so there's nothing to remove).
    lpf_alpha: f64,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        let lpf_alpha = if to_rate < from_rate {
            let cutoff = (to_rate as f64 / 2.0) * 0.9;
            let dt = 1.0 / from_rate as f64;
            let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
            dt / (rc + dt)
        } else {
            1.0
        };
        Self {
            from_rate,
            to_rate,
            pos: 0.0,
            buffer: Vec::new(),
            lpf_state: 0.0,
            lpf_alpha,
        }
    }

    /// Feeds in more native-rate samples and returns as many resampled samples as
    /// can now be produced. Leftover input (and filter/phase state) is retained
    /// across calls, so a stream of chunks resamples continuously with no clicks
    /// at the chunk boundaries.
    pub fn push(&mut self, input: &[i16]) -> Vec<i16> {
        if self.from_rate == self.to_rate {
            return input.to_vec();
        }
        for &s in input {
            self.lpf_state += self.lpf_alpha * (s as f64 - self.lpf_state);
            self.buffer.push(self.lpf_state as i16);
        }
        let ratio = self.from_rate as f64 / self.to_rate as f64;
        let mut out = Vec::new();
        loop {
            let idx = self.pos as usize;
            if idx + 1 >= self.buffer.len() {
                break;
            }
            let frac = self.pos - idx as f64;
            let a = self.buffer[idx] as f64;
            let b = self.buffer[idx + 1] as f64;
            out.push((a + (b - a) * frac) as i16);
            self.pos += ratio;
        }
        let consumed = (self.pos as usize).min(self.buffer.len().saturating_sub(1));
        self.buffer.drain(0..consumed);
        self.pos -= consumed as f64;
        out
    }
}
