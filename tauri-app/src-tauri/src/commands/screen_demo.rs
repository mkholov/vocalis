//! Live screen-demo video for the webview (step 7, part B —
//! `vocalis_roadmap.md`, section 8). Reuses `vocalis::screen_capture` and
//! `vocalis::video` exactly as `teacher::screen`/`student::screen` (and
//! `../../video-bench/`, which measured this) do — capture → H.264 encode →
//! H.264 decode, governed by the same `video::AdaptiveQuality` ladder those
//! network paths already use, nothing here reimplements or tunes it
//! differently.
//!
//! This is a *self-preview* loop, not the class-wide network relay: it
//! captures this machine's own screen and emits it into this machine's own
//! webview. Fanning a teacher's own-screen (or a presenting student's)
//! stream out to a whole class over the network is `teacher::screen`'s job
//! (`run_own_screen_demo`/`run_screen_relay_receiver`) and the student's
//! `run_screen_demo_receiver` — none of that control-message/UDP wiring
//! exists in the Tauri layer yet, so a real cross-machine demo isn't what
//! this drives today. What it does give both consoles is a genuinely live
//! feed to render (real capture, real codec round trip), replacing the
//! static placeholder gradient.
//!
//! JPEG+base64 over plain `emit`/`listen` (not `tauri::ipc::Channel`) is a
//! deliberate choice, not the simpler default: `../../video-bench/` measured
//! both transports and found base64/emit's round trip consistently *lower*
//! than the binary Channel's on this stack (~1.2ms vs ~3.3ms, stable across
//! runs) — see that crate's report for the numbers and the reasoning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::time::MissedTickBehavior;
use vocalis::screen_capture::MonitorCapture;
use vocalis::video;

/// Chosen (not reused from `screen_capture::MONITOR_JPEG_QUALITY`, which is
/// documented for a different tier: 800px passive monitoring, not this
/// 1280px live-video tier) to match exactly what `video-bench` measured its
/// ~7-8ms JPEG-encode timing and ~90-100KB frame size at.
const JPEG_QUALITY: u8 = 75;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScreenDemoFrameDto {
    pub width: u32,
    pub height: u32,
    /// A ready-to-use `data:image/jpeg;base64,...` URL, so the frontend can
    /// drop it straight into an `<img src>` with no further decoding.
    pub data_url: String,
}

/// `MonitorCapture` already carries its own `unsafe impl Send` justification
/// (each `capture_image()` call is self-contained, no thread-affine state
/// held across calls) — unlike `student_mic.rs`'s `cpal::Stream`, nothing
/// here strictly requires a dedicated OS thread for soundness. It still gets
/// one anyway, for the same reason `MicMeter` does: capture+encode+decode+
/// JPEG is real CPU-bound work (tens of ms per frame — see video-bench's
/// numbers), and running it inline on a Tauri async-runtime worker thread
/// would compete with that runtime's other work rather than being isolated
/// to a thread of its own.
pub struct ScreenDemo {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl Drop for ScreenDemo {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct ScreenDemoState(pub Mutex<Option<ScreenDemo>>);

fn jpeg_encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("bad rgba buffer")?;
    let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(jpeg_bytes)
}

fn run_screen_demo_loop<R: tauri::Runtime>(app: AppHandle<R>, stop: Arc<AtomicBool>, ready_tx: std::sync::mpsc::Sender<Result<(), String>>) {
    let capture = match MonitorCapture::primary() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let mut encoder = match video::new_encoder() {
        Ok(e) => e,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let mut decoder = match video::new_decoder() {
        Ok(d) => d,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));

    let rt = match tokio::runtime::Builder::new_current_thread().enable_time().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("screen demo: failed to start runtime: {e:#}");
            return;
        }
    };
    rt.block_on(async move {
        let mut quality = video::AdaptiveQuality::new(0);
        // Same `Delay` reasoning as `teacher::screen::run_own_screen_demo`: a
        // slow tick just ticks less often from here on rather than firing a
        // backlog of missed ticks back-to-back.
        let mut ticker = tokio::time::interval(quality.capture_interval());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            if stop.load(Ordering::Relaxed) {
                return;
            }

            let frame_started = Instant::now();
            let yuv = match video::capture_frame_yuv(&capture, quality.width()) {
                Ok(y) => y,
                Err(e) => {
                    eprintln!("screen demo capture failed: {e:#}");
                    continue;
                }
            };
            let bitstream = match video::encode_frame(&mut encoder, &yuv) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("screen demo H.264 encode failed: {e:#}");
                    continue;
                }
            };
            if quality.record_frame_time(frame_started.elapsed()) {
                ticker = tokio::time::interval(quality.capture_interval());
                ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            }

            let decoded = match video::decode_frame(&mut decoder, &bitstream) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("screen demo H.264 decode failed: {e:#}");
                    continue;
                }
            };
            let Some((width, height, rgba)) = decoded else { continue };

            let jpeg = match jpeg_encode(width, height, &rgba) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("screen demo JPEG encode failed: {e:#}");
                    continue;
                }
            };
            let data_url = format!("data:image/jpeg;base64,{}", BASE64.encode(&jpeg));
            let _ = app.emit("screen-demo-frame", ScreenDemoFrameDto { width, height, data_url });
        }
    });
}

#[tauri::command]
pub fn start_screen_demo<R: tauri::Runtime>(app: AppHandle<R>, demo: State<ScreenDemoState>) -> Result<(), String> {
    let mut guard = demo.0.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread = std::thread::spawn(move || run_screen_demo_loop(app, thread_stop, ready_tx));

    match ready_rx.recv() {
        Ok(Ok(())) => {
            *guard = Some(ScreenDemo { stop, _thread: thread });
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err("screen demo thread exited before reporting readiness".to_string()),
    }
}

#[tauri::command]
pub fn stop_screen_demo(demo: State<ScreenDemoState>) {
    *demo.0.lock().unwrap() = None;
}
