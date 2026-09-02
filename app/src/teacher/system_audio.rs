//! Captures the teacher's *system* audio — whatever's actually playing on the
//! machine (a video's sound, a browser tab, etc.), not the microphone — for
//! streaming alongside a screen demo of the teacher's own screen. Feeds mono
//! i16 chunks to a channel in exactly the shape `mic::start_mic_capture`
//! already does, so `mic::run_screen_audio_broadcast` can pick them up with
//! the same resample -> Opus-encode -> encrypted-UDP-fan-out pipeline the mic
//! broadcast uses.
//!
//! Windows-only: WASAPI loopback capture (recording from an *output* device
//! instead of an input one) has no equivalent on this app's other target,
//! macOS, and this is a Windows-classroom-PC feature by nature anyway. The
//! `not(windows)` build below is a no-op stub that always returns an error,
//! so callers on macOS/Linux just skip starting this stream — the screen-demo
//! video keeps working fine without it, exactly as if the teacher's system
//! happened to be silent.

use anyhow::Result;
use tokio::sync::mpsc;

#[cfg(windows)]
mod backend {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use tokio::sync::mpsc;
    use wasapi::{Direction, SampleType, StreamMode, WasapiError};

    /// Owns the live WASAPI loopback capture thread. Dropping it signals the
    /// thread to stop and tear down the stream — mirrors `mic::MicCapture`'s
    /// "alive == capturing" contract, just backed by a thread instead of a
    /// `cpal::Stream` (the `wasapi` crate's capture loop is blocking, not
    /// callback-based, so it needs its own thread rather than a cpal-style
    /// audio callback).
    pub struct SystemAudioCapture {
        running: Arc<AtomicBool>,
        _thread: std::thread::JoinHandle<()>,
    }

    impl Drop for SystemAudioCapture {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
        }
    }

    /// Starts loopback-capturing the default playback device and pushing mono
    /// i16 chunks to `tx` as they arrive. Returns the capture handle (keep it
    /// alive) and the device's native sample rate, exactly like
    /// `mic::start_mic_capture`.
    ///
    /// All WASAPI/COM setup happens *inside* the spawned thread — these are
    /// COM objects tied to the apartment that created them, matching the
    /// `wasapi` crate's own examples, which never construct a `Device`/
    /// `AudioClient` outside the thread that will use it. The native rate is
    /// reported back over a one-shot channel once that setup finishes, so this
    /// function can still return it synchronously (briefly blocking, the same
    /// tradeoff `start_mic_capture`'s synchronous cpal setup already makes).
    pub fn start(tx: mpsc::UnboundedSender<Vec<i16>>) -> Result<(SystemAudioCapture, u32)> {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32>>();

        let thread = std::thread::Builder::new()
            .name("system-audio-capture".to_string())
            .spawn(move || run_capture_thread(thread_running, tx, ready_tx))
            .context("spawning system-audio-capture thread")?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(native_rate)) => Ok((SystemAudioCapture { running, _thread: thread }, native_rate)),
            Ok(Err(e)) => Err(e),
            Err(_) => anyhow::bail!("timed out waiting for system audio capture to start"),
        }
    }

    fn run_capture_thread(
        running: Arc<AtomicBool>,
        tx: mpsc::UnboundedSender<Vec<i16>>,
        ready_tx: std::sync::mpsc::Sender<Result<u32>>,
    ) {
        let setup = init_loopback();
        let (audio_client, native_rate, channels, bytes_per_sample, sample_type, h_event, capture_client) =
            match setup {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
        if ready_tx.send(Ok(native_rate)).is_err() {
            return; // caller gave up waiting already
        }
        if let Err(e) = audio_client.start_stream() {
            tracing::warn!("failed to start system audio loopback stream: {e:#}");
            return;
        }

        let bytes_per_frame = channels * bytes_per_sample;
        let mut byte_queue: VecDeque<u8> = VecDeque::new();
        while running.load(Ordering::Relaxed) {
            // Short timeout so the stop flag above gets checked promptly even
            // if the loopback device goes quiet (some WASAPI implementations
            // pause buffer events entirely rather than delivering silence when
            // nothing is playing) — a timeout here just means "nothing new
            // yet", not an error.
            match h_event.wait_for_event(200) {
                Ok(()) => {}
                Err(WasapiError::EventTimeout) => continue,
                Err(e) => {
                    tracing::warn!("system audio capture stopped: {e:#}");
                    break;
                }
            }
            if let Err(e) = capture_client.read_from_device_to_deque(&mut byte_queue) {
                tracing::warn!("system audio read failed: {e:#}");
                break;
            }
            let frames = byte_queue.len() / bytes_per_frame;
            if frames == 0 {
                continue;
            }
            let mono = downmix_to_mono_i16(&mut byte_queue, frames, channels, bytes_per_sample, sample_type);
            if tx.send(mono).is_err() {
                break;
            }
        }
        let _ = audio_client.stop_stream();
    }

    #[allow(clippy::type_complexity)]
    fn init_loopback() -> Result<(
        wasapi::AudioClient,
        u32,
        usize,
        usize,
        SampleType,
        wasapi::Handle,
        wasapi::AudioCaptureClient,
    )> {
        wasapi::initialize_mta().ok().context("initializing COM (MTA) for loopback capture")?;
        let enumerator = wasapi::DeviceEnumerator::new().context("creating WASAPI device enumerator")?;
        // The default *render* (playback/speaker) device, not a capture device —
        // requesting a Capture-direction stream from it below is what turns
        // this into a loopback recording instead of a `RenderToCaptureDevice`
        // error.
        let device = enumerator.get_default_device(&Direction::Render).context("no default playback device")?;
        let mut audio_client = device.get_iaudioclient().context("creating loopback audio client")?;
        // Request the device's own mix format exactly (rather than asking WASAPI
        // to auto-convert to something else) — this is guaranteed to be
        // accepted in shared mode, and downmixing/resampling to the mono
        // 16kHz Opus wants happens here ourselves anyway, the same as
        // `mic::start_mic_capture` does with cpal's native format.
        let mix_format = audio_client.get_mixformat().context("querying playback device's mix format")?;
        let native_rate = mix_format.get_samplespersec();
        let channels = mix_format.get_nchannels() as usize;
        let bytes_per_sample = mix_format.get_bitspersample() as usize / 8;
        let sample_type = mix_format.get_subformat().context("unsupported playback device mix format")?;

        let (_default_period, min_period) = audio_client.get_device_period().context("querying device period")?;
        let mode = StreamMode::EventsShared { autoconvert: false, buffer_duration_hns: min_period };
        audio_client
            .initialize_client(&mix_format, &Direction::Capture, &mode)
            .context("initializing loopback capture")?;
        let h_event = audio_client.set_get_eventhandle().context("creating loopback event handle")?;
        let capture_client = audio_client.get_audiocaptureclient().context("getting loopback capture client")?;

        Ok((audio_client, native_rate, channels, bytes_per_sample, sample_type, h_event, capture_client))
    }

    /// Averages every channel of each frame down to mono and rescales to i16,
    /// matching the downmix `mic::start_mic_capture`'s cpal callbacks do.
    /// Handles the two mix formats WASAPI shared mode actually reports in
    /// practice (32-bit float, near-universal; 16/32-bit int, just in case) —
    /// anything else is treated as silence rather than misinterpreted.
    fn downmix_to_mono_i16(
        byte_queue: &mut VecDeque<u8>,
        frames: usize,
        channels: usize,
        bytes_per_sample: usize,
        sample_type: SampleType,
    ) -> Vec<i16> {
        let mut mono = Vec::with_capacity(frames);
        for _ in 0..frames {
            let mut sum = 0f32;
            for _ in 0..channels {
                let mut raw = [0u8; 4];
                for slot in raw.iter_mut().take(bytes_per_sample) {
                    *slot = byte_queue.pop_front().unwrap_or(0);
                }
                let sample = match (sample_type, bytes_per_sample) {
                    (SampleType::Float, 4) => f32::from_le_bytes(raw),
                    (SampleType::Int, 2) => i16::from_le_bytes([raw[0], raw[1]]) as f32 / i16::MAX as f32,
                    (SampleType::Int, 4) => i32::from_le_bytes(raw) as f32 / i32::MAX as f32,
                    _ => 0.0,
                };
                sum += sample;
            }
            let avg = sum / channels as f32;
            mono.push((avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
        mono
    }
}

#[cfg(not(windows))]
mod backend {
    use anyhow::{bail, Result};
    use tokio::sync::mpsc;

    /// No-op stand-in so the rest of the app can call `system_audio::start`
    /// unconditionally — see this module's doc comment.
    pub struct SystemAudioCapture;

    pub fn start(_tx: mpsc::UnboundedSender<Vec<i16>>) -> Result<(SystemAudioCapture, u32)> {
        bail!("system audio capture is only supported on Windows")
    }
}

pub use backend::SystemAudioCapture;

/// Starts capturing the teacher's system audio — see [`backend::start`] (the
/// Windows implementation) and this module's doc comment (the macOS/Linux
/// no-op stub). Callers should treat a failure here as "no system audio for
/// this demo" and continue without it, not as a reason to cancel the demo.
pub fn start_system_audio_capture(tx: mpsc::UnboundedSender<Vec<i16>>) -> Result<(SystemAudioCapture, u32)> {
    backend::start(tx)
}
