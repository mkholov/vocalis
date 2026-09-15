//! Isolated benchmark for step 7 part B (vocalis_roadmap.md, section 8) —
//! same spirit as ../../codec-check/, which proved the H.264 codec itself
//! builds/round-trips correctly. This measures what a *webview* path would
//! add on top of that: re-encoding the decoded RGBA frame to JPEG, and the
//! Tauri IPC latency of getting it to the frontend, compared across the two
//! transports the roadmap asked about (plain `emit`/`listen` with a base64
//! string vs `tauri::ipc::Channel` with raw bytes).
//!
//! Reuses the real pipeline unchanged: `vocalis::screen_capture::MonitorCapture`
//! + `vocalis::video::{new_encoder, new_decoder, capture_frame_yuv,
//! encode_frame, decode_frame}` are exactly what `teacher::screen`/
//! `student::screen` call in the shipped app. Nothing here reimplements or
//! modifies that pipeline — it only measures what's built on top of it.
//!
//! Run with `cargo run --release --bin video-bench` (release matters: JPEG/
//! base64/H.264 timings in debug builds are not representative). Opens one
//! small window (the harness page is inlined below, no frontend build step)
//! and prints a report to stdout, then exits on its own.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

/// Matches `vocalis::video::VIDEO_WIDTH` — the resolution the real demo
/// pipeline captures/encodes at today.
const CAPTURE_WIDTH: u32 = vocalis::video::VIDEO_WIDTH;
/// Matches `vocalis::video::VIDEO_FPS` — only used to compute the frame
/// budget frames are compared against, not to pace sending (see the module
/// doc comment: frames run back-to-back so a slower-than-budget stage shows
/// up as reduced achieved fps rather than being hidden by throttling).
const TARGET_FPS: u32 = vocalis::video::VIDEO_FPS;
const FRAME_BUDGET_MS: f64 = 1000.0 / TARGET_FPS as f64;
const FRAMES_PER_PHASE: usize = 45;
/// A single, reasonable real-time JPEG quality — not copied from
/// `screen_capture::MONITOR_JPEG_QUALITY` (that constant is documented for a
/// different tier: 800px passive monitoring, not 1280px live video), chosen
/// fresh for this resolution/use case.
const JPEG_QUALITY: u8 = 75;

#[derive(Serialize, Clone)]
struct FrameTimings {
    seq: u64,
    capture_ms: f64,
    h264_encode_ms: f64,
    h264_decode_ms: f64,
    jpeg_encode_ms: f64,
    /// Only meaningful for the event/base64 phase — zero for the channel
    /// phase, which sends raw bytes with no separate encoding step.
    base64_encode_ms: f64,
    jpeg_bytes: usize,
    payload_bytes: usize,
    rtt_ms: Option<f64>,
}

#[derive(Serialize, Clone)]
struct EventFramePayload {
    seq: u64,
    data: String,
}

#[derive(Default)]
struct BenchState {
    channel: Mutex<Option<Channel<Vec<u8>>>>,
    pending: Mutex<HashMap<u64, Instant>>,
    acked: Mutex<HashMap<u64, Duration>>,
}

#[tauri::command]
fn debug(msg: String) {
    println!("[webview] {msg}");
}

#[tauri::command]
fn ack(seq: u64, state: State<BenchState>) {
    if let Some(sent_at) = state.pending.lock().unwrap().remove(&seq) {
        state.acked.lock().unwrap().insert(seq, sent_at.elapsed());
    }
}

#[tauri::command]
fn start_channel_bench(channel: Channel<Vec<u8>>, state: State<BenchState>) {
    *state.channel.lock().unwrap() = Some(channel);
}

/// One real capture → H.264 encode → H.264 decode round trip, reusing
/// `vocalis::video` exactly as `teacher::screen`/`student::screen` do.
fn capture_encode_decode(
    capture: &vocalis::screen_capture::MonitorCapture,
    encoder: &mut openh264::encoder::Encoder,
    decoder: &mut openh264::decoder::Decoder,
) -> anyhow::Result<(f64, f64, f64, usize, u32, u32, Vec<u8>)> {
    let t_capture = Instant::now();
    let yuv = vocalis::video::capture_frame_yuv(capture, CAPTURE_WIDTH)?;
    let capture_ms = t_capture.elapsed().as_secs_f64() * 1000.0;

    let t_encode = Instant::now();
    let bitstream = vocalis::video::encode_frame(encoder, &yuv)?;
    let encode_ms = t_encode.elapsed().as_secs_f64() * 1000.0;
    let bitstream_len = bitstream.len();

    let t_decode = Instant::now();
    let decoded = vocalis::video::decode_frame(decoder, &bitstream)?;
    let decode_ms = t_decode.elapsed().as_secs_f64() * 1000.0;

    // A decoder can legitimately buffer internally and return `None` for a
    // frame or two before its first real output (openh264's own documented
    // behavior) — retry immediately with a fresh capture rather than
    // treating that as a failure, exactly like a real receive loop would
    // just wait for the next packet.
    let Some((w, h, rgba)) = decoded else {
        return capture_encode_decode(capture, encoder, decoder);
    };
    Ok((capture_ms, encode_ms, decode_ms, bitstream_len, w, h, rgba))
}

fn jpeg_encode(width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<(f64, Vec<u8>)> {
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| anyhow::anyhow!("bad rgba buffer"))?;
    let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let t = Instant::now();
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)?;
    Ok((t.elapsed().as_secs_f64() * 1000.0, jpeg_bytes))
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn report_stage(name: &str, values: &[f64], unit: &str) {
    if values.is_empty() {
        println!("  {name}: (нет данных)");
        return;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum: f64 = sorted.iter().sum();
    let mean = sum / sorted.len() as f64;
    println!(
        "  {name}: mean={mean:.2}{unit} median={:.2}{unit} p95={:.2}{unit} min={:.2}{unit} max={:.2}{unit}",
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.95),
        sorted.first().unwrap(),
        sorted.last().unwrap(),
    );
}

fn run_phase(
    app: &AppHandle,
    state: &State<BenchState>,
    capture: &vocalis::screen_capture::MonitorCapture,
    encoder: &mut openh264::encoder::Encoder,
    decoder: &mut openh264::decoder::Decoder,
    label: &str,
    use_channel: bool,
) -> Vec<FrameTimings> {
    let mut timings = Vec::with_capacity(FRAMES_PER_PHASE);
    let phase_start = Instant::now();

    for i in 0..FRAMES_PER_PHASE {
        let seq = (if use_channel { 1_000_000 } else { 0 }) + i as u64;
        let Ok((capture_ms, h264_encode_ms, h264_decode_ms, _bitstream_len, w, h, rgba)) =
            capture_encode_decode(capture, encoder, decoder)
        else {
            continue;
        };
        let Ok((jpeg_encode_ms, jpeg_bytes)) = jpeg_encode(w, h, &rgba) else { continue };

        let send_time = Instant::now();
        state.pending.lock().unwrap().insert(seq, send_time);

        let (base64_encode_ms, payload_bytes) = if use_channel {
            let mut payload = Vec::with_capacity(8 + jpeg_bytes.len());
            payload.extend_from_slice(&seq.to_le_bytes());
            payload.extend_from_slice(&jpeg_bytes);
            let payload_len = payload.len();
            let channel = state.channel.lock().unwrap().clone();
            if let Some(channel) = channel {
                let _ = channel.send(payload);
            }
            (0.0, payload_len)
        } else {
            let t = Instant::now();
            let data = BASE64.encode(&jpeg_bytes);
            let b64_ms = t.elapsed().as_secs_f64() * 1000.0;
            let payload_len = data.len();
            let _ = app.emit("bench-frame-event", EventFramePayload { seq, data });
            (b64_ms, payload_len)
        };

        timings.push(FrameTimings {
            seq,
            capture_ms,
            h264_encode_ms,
            h264_decode_ms,
            jpeg_encode_ms,
            base64_encode_ms,
            jpeg_bytes: jpeg_bytes.len(),
            payload_bytes,
            rtt_ms: None,
        });
    }

    // Let in-flight acks land — 300ms is generous for a same-machine IPC
    // round trip that isn't itself the thing under test at this point.
    std::thread::sleep(Duration::from_millis(300));
    let acked = state.acked.lock().unwrap();
    for t in &mut timings {
        t.rtt_ms = acked.get(&t.seq).map(|d| d.as_secs_f64() * 1000.0);
    }
    let elapsed = phase_start.elapsed();
    let achieved_fps = FRAMES_PER_PHASE as f64 / elapsed.as_secs_f64();
    println!(
        "\n=== {label}: {FRAMES_PER_PHASE} кадров за {:.2}с — достигнуто {achieved_fps:.1} fps (цель {TARGET_FPS} fps) ===",
        elapsed.as_secs_f64()
    );
    timings
}

fn print_report(label: &str, timings: &[FrameTimings]) {
    let capture: Vec<f64> = timings.iter().map(|t| t.capture_ms).collect();
    let encode: Vec<f64> = timings.iter().map(|t| t.h264_encode_ms).collect();
    let decode: Vec<f64> = timings.iter().map(|t| t.h264_decode_ms).collect();
    let jpeg: Vec<f64> = timings.iter().map(|t| t.jpeg_encode_ms).collect();
    let b64: Vec<f64> = timings.iter().map(|t| t.base64_encode_ms).collect();
    let rtt: Vec<f64> = timings.iter().filter_map(|t| t.rtt_ms).collect();
    let pipeline_ms_no_transport: Vec<f64> =
        timings.iter().map(|t| t.capture_ms + t.h264_encode_ms + t.h264_decode_ms + t.jpeg_encode_ms + t.base64_encode_ms).collect();
    let avg_jpeg_bytes = timings.iter().map(|t| t.jpeg_bytes).sum::<usize>() / timings.len().max(1);
    let avg_payload_bytes = timings.iter().map(|t| t.payload_bytes).sum::<usize>() / timings.len().max(1);
    let acked_count = timings.iter().filter(|t| t.rtt_ms.is_some()).count();

    println!("--- {label} ---");
    report_stage("захват экрана (capture_rgba_even)", &capture, "мс");
    report_stage("H.264 encode", &encode, "мс");
    report_stage("H.264 decode", &decode, "мс");
    report_stage("JPEG encode (q=75)", &jpeg, "мс");
    if label.contains("base64") {
        report_stage("base64 encode", &b64, "мс");
    }
    report_stage("пайплайн до отправки (без транспорта)", &pipeline_ms_no_transport, "мс");
    println!("  бюджет кадра при {TARGET_FPS} fps: {FRAME_BUDGET_MS:.2}мс");
    println!("  средний размер JPEG: {avg_jpeg_bytes} байт, средний размер payload: {avg_payload_bytes} байт");
    println!("  ack получен: {acked_count}/{}", timings.len());
    report_stage("RTT emit→JS→ack (round-trip, не one-way!)", &rtt, "мс");
}

fn main() -> anyhow::Result<()> {
    println!("video-bench: измеряю реальный пайплайн capture→H.264 encode→decode→JPEG→транспорт webview");
    println!("(release-сборка обязательна для честных цифр; см. `cargo run --release --bin video-bench`)\n");

    tauri::Builder::default()
        .manage(BenchState::default())
        .invoke_handler(tauri::generate_handler![ack, start_channel_bench, debug])
        .setup(move |app| {
            let app_handle = app.handle().clone();
            // Declared in tauri.conf.json (`app.windows`) with url
            // "index.html" served over tauri://localhost — real IPC bridge
            // injection needs the app's own origin, which a `data:` URL page
            // does not get (confirmed: with a data: URL, `invoke()` calls
            // silently never reached any Rust command). This is also more
            // representative of Part B, where the video overlay would be a
            // real bundled asset, not a data: URL.
            let _window = app.get_webview_window("bench").expect("window \"bench\" declared in tauri.conf.json");

            std::thread::spawn(move || {
                // Give the webview a moment to load and register its
                // listener/channel — see the module doc comment on why this
                // is a fixed wait rather than a readiness handshake: it's a
                // one-off manual benchmark, not something that needs to be
                // robust under CI-style load.
                std::thread::sleep(Duration::from_millis(800));

                let state = app_handle.state::<BenchState>();
                let capture = vocalis::screen_capture::MonitorCapture::primary().expect("open primary monitor for capture");
                let mut encoder = vocalis::video::new_encoder().expect("create H.264 encoder");
                let mut decoder = vocalis::video::new_decoder().expect("create H.264 decoder");

                let event_timings = run_phase(&app_handle, &state, &capture, &mut encoder, &mut decoder, "событие + base64 (emit/listen)", false);
                let channel_timings = run_phase(&app_handle, &state, &capture, &mut encoder, &mut decoder, "tauri::ipc::Channel (бинарно)", true);

                // `AppHandle::exit` calls `std::process::exit` under the hood
                // on this Tauri version, which skips any code after `.run()`
                // returns — so the report has to be printed here, before
                // exiting, not after.
                println!();
                print_report("Событие + base64 (emit/listen)", &event_timings);
                println!();
                print_report("tauri::ipc::Channel (бинарно)", &channel_timings);
                app_handle.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
