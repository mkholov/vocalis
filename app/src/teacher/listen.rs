use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lingua_common::{
    new_decoder, split_audio_packet, Resampler, SequenceTracker, OPUS_SAMPLE_RATE,
    TEACHER_LISTEN_PORT,
};
use tokio::net::UdpSocket;

pub type ListenQueue = Arc<Mutex<VecDeque<i16>>>;

/// Current level (RMS, fixed-point *1000) of whichever student is being listened to.
pub static LISTEN_LEVEL_MILLIS: AtomicI32 = AtomicI32::new(0);

/// Playback volume for listen-in/intercom audio, as a percentage (100 = unity gain,
/// the default — matches the previous, unscaled behavior). A single knob rather
/// than a per-student setting: it's a property of "how loud is this speaker fed
/// into my headphones right now", not something worth remembering per student.
/// Applied as a plain multiply in `pull_queued`, right before samples reach the
/// output device — nothing upstream (decoding, the level meter, the queue itself)
/// is touched.
pub static LISTEN_GAIN_PERCENT: AtomicI32 = AtomicI32::new(100);

static OUTPUT_STARTED: AtomicBool = AtomicBool::new(false);

pub fn new_listen_queue() -> ListenQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// Queried once at startup so the receiver can resample straight to the speaker's
/// native rate as audio arrives, instead of re-resampling from scratch (with a phase
/// reset — an audible click) on every single output callback.
pub fn default_output_sample_rate() -> u32 {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .map(|c| c.sample_rate().0)
        .unwrap_or(OPUS_SAMPLE_RATE)
}

fn ensure_output_started(queue: ListenQueue) {
    if OUTPUT_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        if let Err(e) = run_output_stream(queue) {
            tracing::warn!("listen-in output stream failed: {e:#}");
        }
    });
}

fn pull_queued(queue: &ListenQueue, count: usize) -> Vec<i16> {
    let gain = LISTEN_GAIN_PERCENT.load(Ordering::Relaxed) as f32 / 100.0;
    let mut q = queue.lock().unwrap();
    (0..count)
        .map(|_| {
            let s = q.pop_front().unwrap_or(0);
            (s as f32 * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16
        })
        .collect()
}

fn run_output_stream(queue: ListenQueue) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default output device")?;
    // Match the device's own mix format instead of forcing mono/16kHz — most hardware
    // rejects that combination outright (see the same fix in student::audio). The
    // queue already holds samples resampled to this exact rate.
    let config = device
        .default_output_config()
        .context("no default output config")?;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let err_fn = |err| tracing::warn!("listen-in output error: {err}");

    let stream = match sample_format {
        cpal::SampleFormat::I16 => device.build_output_stream(
            &stream_config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                let native = pull_queued(&queue, data.len() / channels);
                for (frame, &s) in data.chunks_mut(channels).zip(native.iter()) {
                    frame.fill(s);
                }
            },
            err_fn,
            None,
        )?,
        _ => device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let native = pull_queued(&queue, data.len() / channels);
                for (frame, &s) in data.chunks_mut(channels).zip(native.iter()) {
                    frame.fill(s as f32 / i16::MAX as f32);
                }
            },
            err_fn,
            None,
        )?,
    };
    stream.play()?;
    std::thread::park();
    Ok(())
}

/// Receives whichever student is currently uploading its mic for real-time listen-in,
/// conceals dropped packets, resamples to the speaker's native rate, and plays it
/// back. Only one student is ever asked to upload at a time, so a single
/// decoder/tracker/resampler is enough.
pub async fn run_listen_receiver(queue: ListenQueue, output_rate: u32) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", TEACHER_LISTEN_PORT)).await?;
    let mut decoder = new_decoder()?;
    let mut tracker = SequenceTracker::new();
    let mut resampler = Resampler::new(OPUS_SAMPLE_RATE, output_rate);
    let mut buf = [0u8; 4096];
    loop {
        let (len, _from) = socket.recv_from(&mut buf).await?;
        let Some((seq, payload)) = split_audio_packet(&buf[..len]) else {
            continue;
        };
        let samples = tracker.decode(&mut decoder, seq, payload);
        if samples.is_empty() {
            continue;
        }
        ensure_output_started(queue.clone());
        LISTEN_LEVEL_MILLIS.store(rms_millis(&samples), Ordering::Relaxed);
        let resampled = resampler.push(&samples);
        let mut q = queue.lock().unwrap();
        q.extend(resampled);
        let max_len = output_rate as usize / 2;
        while q.len() > max_len {
            q.pop_front();
        }
    }
}

fn rms_millis(samples: &[i16]) -> i32 {
    if samples.is_empty() {
        return 0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    ((rms / i16::MAX as f64) * 1000.0) as i32
}
