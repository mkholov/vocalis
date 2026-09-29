//! Real pipeline benchmark for the screen-demo quality decision: is raising the shipped default above its
//! current 1280px/15fps affordable? Measures the actual cost of each stage a frame goes through end to
//! end — capture, H.264 encode, real UDP packetize → encrypt → send → recv → decrypt → reassemble, H.264
//! decode — at the current profile and three candidate profiles, so the answer is based on real numbers
//! from this pipeline rather than a guess.
//!
//! Reuses real, unmodified code throughout: `vocalis::screen_capture::MonitorCapture` +
//! `vocalis::video::{capture_frame_yuv, encode_frame, decode_frame, new_decoder}` are exactly what
//! `teacher::screen`/`student::screen` call in the shipped app, and `lingua_common::video::
//! {encode_video_packets, split_video_packet, FrameReassembler}` + `lingua_common::crypto::{derive_key,
//! encrypt, decrypt}` are the real wire format and real encryption, not a stand-in. The one thing that
//! can't reuse a real function as-is is the *encoder's* tuning: `vocalis::video::new_encoder()` hardcodes
//! the current profile's bitrate/fps, so raising resolution through it alone would silently keep today's
//! 1.5 Mbps target — quietly *worse* quality at a higger resolution, not "what if we raised quality".
//! `build_encoder` below mirrors its exact settings but scales bitrate with the profile's own pixel
//! throughput, modeling "a sane retune to match", not a claim about what the shipped code does today.
//!
//! Sender and receiver run sequentially on one thread, one frame at a time (capture → encode → send this
//! frame's real UDP packets → receive+decrypt+reassemble them → decode), rather than as two real
//! concurrently-running processes. That's a deliberate simplification, not a shortcut taken to save code:
//! macOS has no portable per-thread CPU-time counter the way Linux's `getrusage(RUSAGE_THREAD)` does, so
//! attributing CPU cost cleanly to "encode" vs "decode" specifically (as opposed to "the process as a
//! whole, while two threads were both busy") needs each stage to run with nothing else on CPU at the same
//! time. Every stage is still the real function doing real work over a real (loopback) socket — only the
//! *concurrency* between sender and receiver is simulated away, which if anything makes the total
//! pipeline-latency number reported here a pessimistic upper bound (a real pipelined implementation
//! overlaps these stages across frames) rather than an optimistic one.
//!
//! Run with `cargo run --release --bin pipeline_bench` (release matters — these timings are meaningless in
//! debug). Takes about 25–30s (four ~6s profiles plus warmup) and prints a report, then exits.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lingua_common::crypto::{decrypt, derive_key, encrypt, generate_salt, SessionKey};
use lingua_common::video::{encode_video_packets, split_video_packet, FrameReassembler};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType};
use openh264::formats::YUVSource;
use openh264::OpenH264API;
use vocalis::screen_capture::MonitorCapture;
use vocalis::video::{capture_frame_yuv, decode_frame, encode_frame, new_decoder};

struct Profile {
    label: &'static str,
    /// Capture width in pixels — height follows the real monitor's aspect ratio (see
    /// `MonitorCapture::capture_rgba_even`, unchanged), so "1080p" here means "same width tier as a 1080p
    /// 16:9 screen", not a hardcoded 1920x1080. The real captured size is printed per profile.
    width: u32,
    fps: u32,
}

const PROFILES: &[Profile] = &[
    Profile { label: "720p/15fps (текущий)", width: 1280, fps: 15 },
    Profile { label: "1080p/15fps", width: 1920, fps: 15 },
    Profile { label: "1080p/30fps", width: 1920, fps: 30 },
    Profile { label: "720p/30fps", width: 1280, fps: 30 },
];

/// How long each profile's *measured* phase runs, wall-clock — long enough to average out one-off jitter
/// (window manager redraws, background OS work) without making the whole run (4 profiles) unreasonably
/// slow to iterate on.
const RUN_SECONDS: f64 = 6.0;
/// Discarded frames before measuring, letting the encoder's own rate control and this process's warm
/// caches settle first.
const WARMUP_FRAMES: usize = 10;

/// Mirrors `vocalis::video::VIDEO_BITRATE_BPS` (private there, so duplicated here) — the real bitrate the
/// shipped app targets at its real shipped profile (1280 capture width, 15fps), and the baseline every
/// other profile's bitrate is scaled from below.
const BASELINE_BITRATE_BPS: f64 = 1_500_000.0;
const BASELINE_PIXELS_X_FPS: f64 = 1280.0 * 720.0 * 15.0;

/// Scales the baseline bitrate by how much more (or less) raw pixel throughput (`width * height * fps`) a
/// profile pushes through the encoder relative to the current shipped one — the standard rule-of-thumb
/// starting point for retuning a bitrate to a new resolution/frame rate, not a value taken from anywhere
/// in the real app (which doesn't support these profiles today, so there is nothing to copy from).
fn scaled_bitrate_bps(width: u32, height: u32, fps: u32) -> u32 {
    let ratio = (width as f64 * height as f64 * fps as f64) / BASELINE_PIXELS_X_FPS;
    (BASELINE_BITRATE_BPS * ratio).round() as u32
}

/// Same encoder tuning as `vocalis::video::new_encoder()` (usage type, rate-control mode, disabled
/// adaptive-quantization/background-detection — all copied verbatim), parameterized by resolution/fps
/// instead of hardcoded to the current shipped profile, and with `scaled_bitrate_bps`'s bitrate instead of
/// the fixed 1.5 Mbps. See the module doc comment for why this can't just call the real function.
fn build_encoder(width: u32, height: u32, fps: u32) -> Result<Encoder> {
    let bitrate = scaled_bitrate_bps(width, height, fps);
    let config = EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .bitrate(BitRate::from_bps(bitrate))
        .rate_control_mode(RateControlMode::Bitrate)
        .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 3))
        .adaptive_quantization(false)
        .background_detection(false);
    Encoder::with_api_config(OpenH264API::from_source(), config).context("creating H.264 encoder")
}

/// This process's total CPU time (user + system, across all its threads) so far, in milliseconds. Bracket
/// a single stage's real work with two calls and subtract — since this benchmark otherwise never runs
/// anything concurrently on another thread while a stage is being timed (see the module doc comment), the
/// delta is that stage's own real CPU cost, not a mix of several things running at once.
fn process_cpu_ms() -> f64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let user = usage.ru_utime.tv_sec as f64 * 1000.0 + usage.ru_utime.tv_usec as f64 / 1000.0;
        let sys = usage.ru_stime.tv_sec as f64 * 1000.0 + usage.ru_stime.tv_usec as f64 / 1000.0;
        user + sys
    }
}

/// One real capture -> encode -> send -> receive -> decode round trip's own timings. `None` fields mean
/// "no output yet at this stage" (an H.264 decoder can legitimately buffer internally for a frame or two —
/// see `decode_frame`'s own doc comment), not an error; such a frame is excluded from the profile's stats
/// entirely, exactly like a real receiver would just wait for the next one.
struct FrameSample {
    capture_ms: f64,
    encode_ms: f64,
    encode_cpu_ms: f64,
    network_ms: f64,
    decode_ms: f64,
    decode_cpu_ms: f64,
    /// Total bytes that actually went over the (real, loopback) socket for this frame: every packet's
    /// encrypted length, header included — what a real LAN would have to carry.
    wire_bytes: usize,
    /// capture_ms + encode_ms + network_ms + decode_ms — this one frame's own one-way trip through the
    /// pipeline, from "screen sampled" to "pixels decoded and ready to show". See the module doc comment:
    /// since stages run sequentially here rather than pipelined across frames, this is a pessimistic upper
    /// bound on real end-to-end latency, not an exact figure.
    pipeline_ms: f64,
    /// Wall-clock time since the *previous* successfully decoded frame finished — i.e. how evenly frames
    /// would actually arrive on screen. `None` for the first frame of a run (nothing to compare to yet).
    interarrival_ms: Option<f64>,
}

struct ProfileReport {
    label: String,
    target_fps: u32,
    captured_width: u32,
    captured_height: u32,
    bitrate_bps: u32,
    samples: Vec<FrameSample>,
    measured_wall_secs: f64,
}

#[allow(clippy::too_many_arguments)]
fn run_profile(
    capture: &MonitorCapture,
    profile: &Profile,
    sender_socket: &UdpSocket,
    receiver_socket: &UdpSocket,
    receiver_addr: std::net::SocketAddr,
    key: &SessionKey,
) -> Result<ProfileReport> {
    // Learn the real captured size for this width up front (depends on this machine's actual monitor
    // aspect ratio — see `MonitorCapture::capture_rgba_even`) so the encoder can be tuned for it and the
    // report can print the real numbers rather than a nominal request.
    let probe = capture_frame_yuv(capture, profile.width)?;
    let (captured_width, captured_height) = {
        let (w, h) = probe.dimensions();
        (w as u32, h as u32)
    };

    let mut encoder = build_encoder(captured_width, captured_height, profile.fps)?;
    let mut decoder = new_decoder()?;
    let mut reassembler = FrameReassembler::new();

    let mut frame_seq: u32 = 0;
    let mut recv_buf = [0u8; 2048];

    let mut run_one = |frame_seq: u32| -> Result<Option<FrameSample>> {
        let t_capture = Instant::now();
        let cpu_before = process_cpu_ms();
        let yuv = capture_frame_yuv(capture, profile.width)?;
        let capture_ms = t_capture.elapsed().as_secs_f64() * 1000.0;
        let _capture_cpu_ms = process_cpu_ms() - cpu_before; // not reported separately — see report_stage calls below

        let t_encode = Instant::now();
        let cpu_before = process_cpu_ms();
        let bitstream = encode_frame(&mut encoder, &yuv)?;
        let encode_ms = t_encode.elapsed().as_secs_f64() * 1000.0;
        let encode_cpu_ms = process_cpu_ms() - cpu_before;

        let t_network = Instant::now();
        let packets = encode_video_packets(frame_seq, &bitstream);
        let mut wire_bytes = 0usize;
        for packet in &packets {
            let encrypted = encrypt(key, packet);
            wire_bytes += encrypted.len();
            sender_socket.send_to(&encrypted, receiver_addr).context("sending real UDP video packet")?;
        }
        let mut reassembled = None;
        for _ in 0..packets.len() {
            let (len, _addr) = receiver_socket.recv_from(&mut recv_buf).context("receiving real UDP video packet")?;
            let plaintext = decrypt(key, &recv_buf[..len]).context("decrypting real video packet")?;
            let Some((header, payload)) = split_video_packet(&plaintext) else { continue };
            if let Some(complete) = reassembler.push(header, payload) {
                reassembled = Some(complete);
            }
        }
        let network_ms = t_network.elapsed().as_secs_f64() * 1000.0;

        let Some(bitstream_out) = reassembled else {
            return Ok(None); // shouldn't happen on loopback with no packet loss, but treat like a real receiver would
        };

        let t_decode = Instant::now();
        let cpu_before = process_cpu_ms();
        let decoded = decode_frame(&mut decoder, &bitstream_out)?;
        let decode_ms = t_decode.elapsed().as_secs_f64() * 1000.0;
        let decode_cpu_ms = process_cpu_ms() - cpu_before;

        if decoded.is_none() {
            return Ok(None);
        }

        Ok(Some(FrameSample {
            capture_ms,
            encode_ms,
            encode_cpu_ms,
            network_ms,
            decode_ms,
            decode_cpu_ms,
            wire_bytes,
            pipeline_ms: capture_ms + encode_ms + network_ms + decode_ms,
            interarrival_ms: None, // filled in by the caller, which alone tracks "previous frame done" time
        }))
    };

    for _ in 0..WARMUP_FRAMES {
        frame_seq += 1;
        run_one(frame_seq)?;
    }

    let mut samples = Vec::new();
    let mut last_done: Option<Instant> = None;
    let phase_start = Instant::now();
    while phase_start.elapsed().as_secs_f64() < RUN_SECONDS {
        frame_seq += 1;
        if let Some(mut sample) = run_one(frame_seq)? {
            let now = Instant::now();
            sample.interarrival_ms = last_done.map(|prev| now.duration_since(prev).as_secs_f64() * 1000.0);
            last_done = Some(now);
            samples.push(sample);
        }
    }
    let measured_wall_secs = phase_start.elapsed().as_secs_f64();

    Ok(ProfileReport {
        label: profile.label.to_string(),
        target_fps: profile.fps,
        captured_width,
        captured_height,
        bitrate_bps: scaled_bitrate_bps(captured_width, captured_height, profile.fps),
        samples,
        measured_wall_secs,
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn stats(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    (mean, percentile(&sorted, 0.5), percentile(&sorted, 0.95))
}

fn machine_info() -> String {
    let run = |cmd: &str, args: &[&str]| -> String {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "?".to_string())
    };
    let brand = run("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let model = run("sysctl", &["-n", "hw.model"]);
    let ncpu = run("sysctl", &["-n", "hw.ncpu"]);
    format!("{model}, {brand}, {ncpu} логических ядер, macOS")
}

fn print_report(r: &ProfileReport) {
    let capture: Vec<f64> = r.samples.iter().map(|s| s.capture_ms).collect();
    let encode: Vec<f64> = r.samples.iter().map(|s| s.encode_ms).collect();
    let network: Vec<f64> = r.samples.iter().map(|s| s.network_ms).collect();
    let decode: Vec<f64> = r.samples.iter().map(|s| s.decode_ms).collect();
    let pipeline: Vec<f64> = r.samples.iter().map(|s| s.pipeline_ms).collect();
    let interarrival: Vec<f64> = r.samples.iter().filter_map(|s| s.interarrival_ms).collect();

    let total_encode_cpu_ms: f64 = r.samples.iter().map(|s| s.encode_cpu_ms).sum();
    let total_decode_cpu_ms: f64 = r.samples.iter().map(|s| s.decode_cpu_ms).sum();
    let total_wire_bytes: usize = r.samples.iter().map(|s| s.wire_bytes).sum();
    let avg_wire_bytes = total_wire_bytes as f64 / r.samples.len().max(1) as f64;

    let achieved_fps = r.samples.len() as f64 / r.measured_wall_secs;
    let encode_cpu_pct = total_encode_cpu_ms / (r.measured_wall_secs * 1000.0) * 100.0;
    let decode_cpu_pct = total_decode_cpu_ms / (r.measured_wall_secs * 1000.0) * 100.0;
    let traffic_bytes_per_sec = total_wire_bytes as f64 / r.measured_wall_secs;

    let (cap_mean, cap_med, cap_p95) = stats(&capture);
    let (enc_mean, enc_med, enc_p95) = stats(&encode);
    let (net_mean, net_med, net_p95) = stats(&network);
    let (dec_mean, dec_med, dec_p95) = stats(&decode);
    let (pipe_mean, pipe_med, pipe_p95) = stats(&pipeline);
    let (inter_mean, inter_med, inter_p95) = stats(&interarrival);

    // The benchmark runs sender (capture+encode) and receiver (network+decode) sequentially on one thread
    // — see the module doc comment — so `pipeline_ms`/achieved fps above are their *sum*, which understates
    // what a real two-machine deployment could sustain: there, the sender's own loop only has to keep up
    // with capture+encode, and a receiving student's loop only has to keep up with network+decode, in
    // parallel with each other. These two ceilings are the more realistic "can this actually run at the
    // target fps" figures.
    let sender_ms = cap_mean + enc_mean;
    let receiver_ms = net_mean + dec_mean;
    let sender_fps_ceiling = 1000.0 / sender_ms;
    let receiver_fps_ceiling = 1000.0 / receiver_ms;

    println!("\n=== {} — захват {}x{}, битрейт {:.2} Мбит/с, цель {} fps ===", r.label, r.captured_width, r.captured_height, r.bitrate_bps as f64 / 1_000_000.0, r.target_fps);
    println!("  кадров измерено: {} за {:.2}с — реальный fps на приёме (этот бенч, последовательно): {achieved_fps:.1}", r.samples.len(), r.measured_wall_secs);
    println!(
        "  реалистичный потолок при двух отдельных машинах: отправитель (capture+encode) {sender_fps_ceiling:.1} fps, получатель (сеть+decode) {receiver_fps_ceiling:.1} fps — держит меньший из двух"
    );
    println!("  захват экрана:      mean={cap_mean:.2}мс median={cap_med:.2}мс p95={cap_p95:.2}мс");
    println!("  H.264 encode:       mean={enc_mean:.2}мс median={enc_med:.2}мс p95={enc_p95:.2}мс   CPU: {encode_cpu_pct:.1}% от одного ядра (устойчиво)");
    println!("  сеть (packetize→encrypt→send→recv→decrypt→reassemble, реальный UDP): mean={net_mean:.2}мс median={net_med:.2}мс p95={net_p95:.2}мс");
    println!("  H.264 decode:       mean={dec_mean:.2}мс median={dec_med:.2}мс p95={dec_p95:.2}мс   CPU: {decode_cpu_pct:.1}% от одного ядра (устойчиво)");
    println!("  пайплайн кадра (capture→decode последовательно в этом бенче, верхняя граница задержки одного кадра): mean={pipe_mean:.2}мс median={pipe_med:.2}мс p95={pipe_p95:.2}мс");
    println!("  задержка между последовательными кадрами на приёме (в этом бенче): mean={inter_mean:.2}мс median={inter_med:.2}мс p95={inter_p95:.2}мс");
    println!("  средний размер кадра на проводе: {avg_wire_bytes:.0} байт   трафик: {:.0} КБ/с ({:.2} Мбит/с)", traffic_bytes_per_sec / 1024.0, traffic_bytes_per_sec * 8.0 / 1_000_000.0);
}

fn main() -> Result<()> {
    println!("pipeline-bench: реальный пайплайн capture → H.264 encode → real UDP (packetize/encrypt/send/recv/decrypt/reassemble) → H.264 decode");
    println!("машина: {}", machine_info());
    println!("(это верхняя граница по железу — Mac разработчика, не типичный школьный компьютер; release-сборка обязательна для честных цифр)\n");

    let capture = MonitorCapture::primary().context("open primary monitor for capture")?;

    let sender_socket = UdpSocket::bind("127.0.0.1:0").context("bind sender socket")?;
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").context("bind receiver socket")?;
    receiver_socket.set_read_timeout(Some(Duration::from_secs(2))).context("set receiver read timeout")?;
    let receiver_addr = receiver_socket.local_addr().context("receiver local_addr")?;

    // A real session key, derived exactly the way a real connection derives one (`derive_key`, the same
    // 200,000-round KDF) — done once, like a real connection's handshake, not per packet/per frame.
    let salt = generate_salt();
    let key = derive_key("000000", &salt);

    let mut reports = Vec::new();
    for profile in PROFILES {
        println!("--- измеряю {} ({}с, после {} кадров разогрева) ---", profile.label, RUN_SECONDS, WARMUP_FRAMES);
        let report = run_profile(&capture, profile, &sender_socket, &receiver_socket, receiver_addr, &key)?;
        print_report(&report);
        reports.push(report);
    }

    println!("\n=== Сравнение с текущим профилем ({}) ===", reports[0].label);
    let baseline = &reports[0];
    let baseline_encode_cpu: f64 = baseline.samples.iter().map(|s| s.encode_cpu_ms).sum::<f64>() / baseline.measured_wall_secs;
    let baseline_decode_cpu: f64 = baseline.samples.iter().map(|s| s.decode_cpu_ms).sum::<f64>() / baseline.measured_wall_secs;
    let baseline_traffic: f64 = baseline.samples.iter().map(|s| s.wire_bytes).sum::<usize>() as f64 / baseline.measured_wall_secs;
    let baseline_fps = baseline.samples.len() as f64 / baseline.measured_wall_secs;
    for r in &reports[1..] {
        let encode_cpu: f64 = r.samples.iter().map(|s| s.encode_cpu_ms).sum::<f64>() / r.measured_wall_secs;
        let decode_cpu: f64 = r.samples.iter().map(|s| s.decode_cpu_ms).sum::<f64>() / r.measured_wall_secs;
        let traffic: f64 = r.samples.iter().map(|s| s.wire_bytes).sum::<usize>() as f64 / r.measured_wall_secs;
        let fps = r.samples.len() as f64 / r.measured_wall_secs;
        println!(
            "  {}: CPU encode x{:.2}, CPU decode x{:.2}, трафик x{:.2}, реальный fps на приёме x{:.2}",
            r.label,
            encode_cpu / baseline_encode_cpu.max(0.001),
            decode_cpu / baseline_decode_cpu.max(0.001),
            traffic / baseline_traffic.max(0.001),
            fps / baseline_fps.max(0.001),
        );
    }

    Ok(())
}
