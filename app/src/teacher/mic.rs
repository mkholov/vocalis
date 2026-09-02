use std::net::SocketAddr;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use lingua_common::{
    crypto, encode_audio_packet, new_encoder, Resampler, SessionKey, StudentId, FRAME_SAMPLES,
    OPUS_SAMPLE_RATE, SCREEN_AUDIO_PORT,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::warn;

use super::state::AppState;

/// Current mic input level (RMS, fixed-point *1000) for the class-wide broadcast,
/// read by the GUI for the top-bar VU meter.
pub static MIC_LEVEL_MILLIS: AtomicI32 = AtomicI32::new(0);
/// Same, but for the private intercom mic — kept as a separate atomic (rather than
/// reusing `MIC_LEVEL_MILLIS`) so the two VU meters don't stomp on each other when
/// a class broadcast and a private intercom are both live at once.
pub static INTERCOM_MIC_LEVEL_MILLIS: AtomicI32 = AtomicI32::new(0);

/// Owns the live cpal input stream. Must stay alive (and on the thread that created it)
/// for as long as the microphone should be capturing; dropping it stops the stream.
pub struct MicCapture {
    _stream: cpal::Stream,
}

/// Opens `device_name` (or the system default, if `None` or not found — see
/// `audio_devices::resolve_input_device`) and starts pushing mono i16 chunks
/// to `tx` as they arrive, reporting RMS level into `level` as it goes (pass
/// `&MIC_LEVEL_MILLIS` or `&INTERCOM_MIC_LEVEL_MILLIS` depending on which
/// pipeline is capturing — the two can run concurrently, each with its own
/// independent `cpal` input stream). Returns the capture handle (keep it
/// alive) and the device's native sample rate.
pub fn start_mic_capture(
    tx: mpsc::UnboundedSender<Vec<i16>>,
    level: &'static AtomicI32,
    device_name: Option<&str>,
) -> Result<(MicCapture, u32)> {
    let device = crate::audio_devices::resolve_input_device(device_name)?;
    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let err_fn = |err| warn!("audio input stream error: {err}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        let avg = frame.iter().sum::<f32>() / channels as f32;
                        (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                    })
                    .collect();
                level.store(rms_millis(&mono), Ordering::Relaxed);
                let _ = tx.send(mono);
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let mono: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                        (sum / channels as i32) as i16
                    })
                    .collect();
                level.store(rms_millis(&mono), Ordering::Relaxed);
                let _ = tx.send(mono);
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("unsupported input sample format: {other:?}"),
    };
    stream.play()?;
    Ok((MicCapture { _stream: stream }, sample_rate))
}

fn rms_millis(samples: &[i16]) -> i32 {
    if samples.is_empty() {
        return 0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    ((rms / i16::MAX as f64) * 1000.0) as i32
}

/// Resamples to 16kHz, Opus-encodes, and fans each frame out over UDP to every
/// currently connected student.
pub async fn run_mic_broadcast(
    state: AppState,
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    native_rate: u32,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let mut resampler = Resampler::new(native_rate, OPUS_SAMPLE_RATE);
    let encoder = new_encoder()?;
    let mut seq: u32 = 0;
    let mut pending: Vec<i16> = Vec::new();

    while let Some(chunk) = rx.recv().await {
        pending.extend(resampler.push(&chunk));
        while pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = pending.drain(..FRAME_SAMPLES).collect();
            let mut opus_buf = [0u8; 4000];
            let n = match encoder.encode(&frame, &mut opus_buf) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let packet = encode_audio_packet(seq, &opus_buf[..n]);
            seq = seq.wrapping_add(1);
            // Not one shared packet fanned out identically — each student gets
            // their own ciphertext, encrypted under their own session key (see
            // `state::SharedState::student_addrs_with_keys`'s doc comment).
            let addrs_with_keys = state.lock().unwrap().student_addrs_with_keys();
            for (addr, key) in addrs_with_keys {
                let encrypted = crypto::encrypt(&key, &packet);
                let _ = socket.send_to(&encrypted, addr).await;
            }
        }
    }
    Ok(())
}

/// Resamples to 16kHz, Opus-encodes, and sends each frame to exactly one student —
/// the outbound leg of the private teacher<->student intercom. Structurally the
/// same pipeline as `run_mic_broadcast`, just addressed to a single fixed target
/// instead of fanned out to the whole class, and driven by its own independent mic
/// capture so it doesn't interfere with a concurrent class-wide broadcast.
pub async fn run_intercom_send(
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    native_rate: u32,
    target: SocketAddr,
    key: SessionKey,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let mut resampler = Resampler::new(native_rate, OPUS_SAMPLE_RATE);
    let encoder = new_encoder()?;
    let mut seq: u32 = 0;
    let mut pending: Vec<i16> = Vec::new();

    while let Some(chunk) = rx.recv().await {
        pending.extend(resampler.push(&chunk));
        while pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = pending.drain(..FRAME_SAMPLES).collect();
            let mut opus_buf = [0u8; 4000];
            let n = match encoder.encode(&frame, &mut opus_buf) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let packet = encode_audio_packet(seq, &opus_buf[..n]);
            seq = seq.wrapping_add(1);
            let encrypted = crypto::encrypt(&key, &packet);
            let _ = socket.send_to(&encrypted, target).await;
        }
    }
    Ok(())
}

/// Resamples to 16kHz, Opus-encodes, and fans each frame out (UDP, per-
/// recipient encrypted) to `targets` on `SCREEN_AUDIO_PORT` — the teacher's
/// *system* audio (see `system_audio`), streamed alongside a teacher-sourced
/// screen demo. Structurally the same pipeline as `run_mic_broadcast`, just a
/// separate port/stream so it never gets mixed with a concurrent mic
/// broadcast: the teacher can talk over the video without the two merging
/// into one. `targets` is fixed for the task's lifetime, exactly like the
/// video demo's own target list — started and stopped together with it.
pub async fn run_screen_audio_broadcast(
    state: AppState,
    mut rx: mpsc::UnboundedReceiver<Vec<i16>>,
    native_rate: u32,
    targets: Vec<StudentId>,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let mut resampler = Resampler::new(native_rate, OPUS_SAMPLE_RATE);
    let encoder = new_encoder()?;
    let mut seq: u32 = 0;
    let mut pending: Vec<i16> = Vec::new();

    while let Some(chunk) = rx.recv().await {
        pending.extend(resampler.push(&chunk));
        while pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = pending.drain(..FRAME_SAMPLES).collect();
            let mut opus_buf = [0u8; 4000];
            let n = match encoder.encode(&frame, &mut opus_buf) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let packet = encode_audio_packet(seq, &opus_buf[..n]);
            seq = seq.wrapping_add(1);
            let addrs_with_keys = state.lock().unwrap().addrs_with_keys(&targets, SCREEN_AUDIO_PORT);
            for (addr, key) in addrs_with_keys {
                let encrypted = crypto::encrypt(&key, &packet);
                let _ = socket.send_to(&encrypted, addr).await;
            }
        }
    }
    Ok(())
}
