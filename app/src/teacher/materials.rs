//! Playback of pre-recorded audio materials (mp3/wav) to the whole class or a
//! subset of students — decodes the file once with `symphonia`, then feeds the
//! result through the exact same resample/Opus-encode/UDP-send pipeline the live
//! mic broadcast uses (`common/src/audio.rs`, `teacher::mic::run_mic_broadcast`),
//! so a receiving student can't tell the difference from a live broadcast.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use lingua_common::{
    encode_audio_packet, new_encoder, Resampler, FRAME_SAMPLES, OPUS_SAMPLE_RATE,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tokio::net::UdpSocket;

use super::state::AppState;

/// Decodes an mp3/wav file to mono i16 PCM at its own native sample rate (channel
/// downmix is a plain average — fine for spoken-word classroom material, not meant
/// for music mastering). The format is auto-detected from content plus the file
/// extension as a hint; whatever `symphonia`'s probe recognizes is accepted.
pub fn decode_to_mono_pcm(path: &Path) -> Result<(Vec<i16>, u32)> {
    let file = std::fs::File::open(path).context("opening audio file")?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .context("unrecognized audio format (only mp3/wav are supported)")?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .context("no playable audio track in file")?
        .clone();
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .context("audio track has no sample rate")?;
    let channels = track
        .codec_params
        .channels
        .context("audio track has no channel layout")?
        .count()
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported audio codec")?;

    let mut mono = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let mut sample_buf = SampleBuffer::<i16>::new(audio_buf.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(audio_buf);
                for frame in sample_buf.samples().chunks(channels) {
                    let avg = frame.iter().map(|&s| s as i32).sum::<i32>() / channels as i32;
                    mono.push(avg as i16);
                }
            }
            // A single bad packet shouldn't sink an otherwise-playable file.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::ensure!(!mono.is_empty(), "decoded audio file produced no samples");
    Ok((mono, sample_rate))
}

/// Resamples the whole (already-decoded) clip to 16kHz, Opus-encodes it, and sends
/// it to `targets` at the same 20ms-per-frame cadence a live mic stream would —
/// unlike a live capture there's no natural real-time pacing for a file that's
/// entirely in memory already, so a `tokio::time::interval` ticker stands in for
/// the hardware callback that normally paces `run_mic_broadcast`. Updates
/// `state.playing.elapsed_ms` as it goes; stops early (without clearing `playing`
/// itself — the caller already did, e.g. via the "Stop" button) if `playing` goes
/// `None` out from under it.
pub async fn run_playback(
    state: AppState,
    samples: Vec<i16>,
    native_rate: u32,
    targets: Vec<SocketAddr>,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let mut resampler = Resampler::new(native_rate, OPUS_SAMPLE_RATE);
    let encoder = new_encoder()?;
    let mut seq: u32 = 0;

    let resampled = resampler.push(&samples);
    let frame_count = resampled.len() / FRAME_SAMPLES;
    let frame_duration = Duration::from_millis((FRAME_SAMPLES as u64 * 1000) / OPUS_SAMPLE_RATE as u64);
    let mut ticker = tokio::time::interval(frame_duration);

    for i in 0..frame_count {
        ticker.tick().await;

        let frame = &resampled[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let mut opus_buf = [0u8; 4000];
        let n = match encoder.encode(frame, &mut opus_buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let packet = encode_audio_packet(seq, &opus_buf[..n]);
        seq = seq.wrapping_add(1);
        for addr in &targets {
            let _ = socket.send_to(&packet, *addr).await;
        }

        let elapsed_ms = (i as u64 + 1) * frame_duration.as_millis() as u64;
        let mut guard = state.lock().unwrap();
        match guard.playing.as_mut() {
            Some(playing) => playing.elapsed_ms = elapsed_ms,
            // Stopped externally (e.g. the "Stop" button already cleared `playing`).
            None => return Ok(()),
        }
    }

    state.lock().unwrap().playing = None;
    Ok(())
}
