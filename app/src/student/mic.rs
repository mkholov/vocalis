use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;
use tracing::warn;

/// Owns the live cpal input stream; dropping it stops the microphone.
pub struct MicCapture {
    _stream: cpal::Stream,
}

/// Opens the default microphone and streams mono i16 chunks to `tx` as they arrive.
/// Returns the capture handle (keep it alive) and the device's native sample rate.
pub fn start_mic_capture(tx: mpsc::UnboundedSender<Vec<i16>>) -> Result<(MicCapture, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default microphone found")?;
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
