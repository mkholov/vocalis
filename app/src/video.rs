//! H.264 encode/decode for the screen-demo video pipeline — the codec layer
//! `codec-check/` verified builds and round-trips correctly on both macOS and
//! Windows, now wired into the real app. This module is purely the openh264
//! capture-frame <-> H.264 plumbing shared by both roles (teacher's own-screen
//! sender, a presenting student's uploader, and every student's receiver);
//! the UDP wire format (packetization/reassembly) lives in
//! `lingua_common::video`, and screen capture itself in `screen_capture`.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use openh264::decoder::Decoder;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType};
use openh264::formats::{RgbaSliceU8, YUVBuffer, YUVSource};
use openh264::OpenH264API;

use crate::screen_capture::MonitorCapture;

/// 15fps (not 30) is deliberate: plenty for following a presentation/video,
/// noticeably lighter on the presenter's CPU than double the rate — the
/// capture+encode budget matters here since this runs on ordinary school PCs.
pub const VIDEO_FPS: u32 = 15;
/// First degradation step (see [`AdaptiveQuality`]) when a machine can't keep
/// up at `VIDEO_FPS`.
pub const VIDEO_FPS_LOW: u32 = 10;
/// Downscale target — matches the old JPEG demo tier's width, still
/// comfortably readable for following along without pushing encode time or
/// bandwidth much past what that tier already cost.
pub const VIDEO_WIDTH: u32 = 1280;
/// Second degradation step, tried only once dropping to `VIDEO_FPS_LOW` alone
/// wasn't enough.
pub const VIDEO_WIDTH_LOW: u32 = 960;
const VIDEO_BITRATE_BPS: u32 = 1_500_000;
/// One keyframe roughly every 3s at `VIDEO_FPS`. Bounds how long a dropped
/// packet's glitch can persist before the stream self-heals (see
/// `lingua_common::video`'s module doc), without spending too much bandwidth
/// on keyframes given how cheaply delta frames compress for mostly-static
/// screen content.
const KEYFRAME_INTERVAL_FRAMES: u32 = VIDEO_FPS * 3;

/// (width, fps) at each degradation step, best quality first.
const QUALITY_LADDER: [(u32, u32); 3] = [(VIDEO_WIDTH, VIDEO_FPS), (VIDEO_WIDTH, VIDEO_FPS_LOW), (VIDEO_WIDTH_LOW, VIDEO_FPS_LOW)];

/// Consecutive over-budget frames required before stepping down a level —
/// high enough that one slow frame (a GC-style pause, another app briefly
/// stealing the CPU) doesn't trigger it, low enough to react within about a
/// second of genuinely sustained trouble.
const OVERRUN_STREAK: u32 = 15;

/// Step-degrades capture width/fps when a send loop's capture+encode work
/// can't keep up with its own tick interval, instead of letting a slow
/// machine accumulate ever-growing latency. `tokio::time::interval`'s default
/// "burst" catch-up behavior would otherwise fire a backlog of missed ticks
/// back-to-back the moment a loop falls behind — every caller using this
/// should also set `MissedTickBehavior::Delay` on its own ticker (see
/// `teacher::screen::run_own_screen_demo`/`student::screen::run_video_upload`)
/// so the two mechanisms work together: ticks never pile up, and the ladder
/// brings the *target* rate down to something the machine can actually sustain.
///
/// Degrades one step at a time, only after `OVERRUN_STREAK` *consecutive*
/// over-budget frames, and never climbs back up — deliberately simple, not an
/// adaptive bitrate controller. Once the lowest step still can't keep up,
/// `record_frame_time` just keeps returning `false`: the loop stays at that
/// floor and keeps running (slower than its nominal rate, but never hanging,
/// crashing, or growing an unbounded backlog) rather than trying anything
/// fancier.
pub struct AdaptiveQuality {
    level: usize,
    consecutive_overruns: u32,
}

impl Default for AdaptiveQuality {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveQuality {
    pub fn new() -> Self {
        Self { level: 0, consecutive_overruns: 0 }
    }

    pub fn width(&self) -> u32 {
        QUALITY_LADDER[self.level].0
    }

    pub fn fps(&self) -> u32 {
        QUALITY_LADDER[self.level].1
    }

    pub fn capture_interval(&self) -> Duration {
        Duration::from_millis(1000 / u64::from(self.fps()))
    }

    /// Records how long one capture+encode cycle actually took. Returns
    /// `true` exactly when this call caused a step-down — the caller should
    /// then rebuild its ticker at the new (lower) fps via `capture_interval()`.
    pub fn record_frame_time(&mut self, elapsed: Duration) -> bool {
        let budget = self.capture_interval();
        if elapsed <= budget {
            self.consecutive_overruns = 0;
            return false;
        }
        self.consecutive_overruns += 1;
        if self.consecutive_overruns < OVERRUN_STREAK || self.level + 1 >= QUALITY_LADDER.len() {
            return false;
        }
        self.level += 1;
        self.consecutive_overruns = 0;
        tracing::warn!(
            "screen-demo capture/encode exceeded its {budget:?} budget for {OVERRUN_STREAK} frames in a row \
             (last frame took {elapsed:?}) — degrading to {}px/{}fps",
            self.width(),
            self.fps(),
        );
        true
    }
}

/// Creates an encoder tuned for screen content at `VIDEO_FPS`/`VIDEO_WIDTH`.
/// One instance is meant to live for a whole demo session (its internal
/// P-frame reference state needs continuity across calls), not be recreated
/// per frame.
pub fn new_encoder() -> Result<Encoder> {
    let config = EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .max_frame_rate(FrameRate::from_hz(VIDEO_FPS as f32))
        .bitrate(BitRate::from_bps(VIDEO_BITRATE_BPS))
        .rate_control_mode(RateControlMode::Bitrate)
        .intra_frame_period(IntraFramePeriod::from_num_frames(KEYFRAME_INTERVAL_FRAMES))
        // Neither is supported for screen content — openh264 auto-disables
        // both anyway (logging a warning to stdout each time), so turn them
        // off up front instead.
        .adaptive_quantization(false)
        .background_detection(false);
    Encoder::with_api_config(OpenH264API::from_source(), config).context("creating H.264 encoder")
}

/// Creates a decoder. Like the encoder, one instance is meant to be reused
/// across many frames (and, safely, across multiple separate demo sessions —
/// each one's first frame is always a keyframe, which resyncs the decoder
/// regardless of whatever came before).
pub fn new_decoder() -> Result<Decoder> {
    Decoder::new().context("creating H.264 decoder")
}

/// Captures the primary monitor at `width` and converts it to YUV420, ready
/// for `encode_frame`. Takes an already-resolved `MonitorCapture` (rather than
/// resolving the monitor itself) so a capture loop can reuse the same handle
/// across every tick — see `MonitorCapture`'s doc comment for why that
/// matters. `width` is a parameter (not always `VIDEO_WIDTH`) so a caller
/// using `AdaptiveQuality` can pass its current, possibly degraded, target.
pub fn capture_frame_yuv(capture: &MonitorCapture, width: u32) -> Result<YUVBuffer> {
    let rgba = capture.capture_rgba_even(width)?;
    let (width, height) = rgba.dimensions();
    let source = RgbaSliceU8::new(rgba.as_raw(), (width as usize, height as usize));
    Ok(YUVBuffer::from_rgba8_source(source))
}

/// Test-only hook for exercising `AdaptiveQuality`'s degradation without
/// needing an actually slow machine: if `VOCALIS_TEST_ENCODE_DELAY_MS` is set
/// to a number, every `encode_frame` call below blocks that long before
/// returning, simulating a CPU too slow to keep up. Read once — a real
/// deployment never sets this env var, so this is a single cached duration
/// check (typically `Duration::ZERO`, no sleep) per call in production.
fn artificial_encode_delay() -> Duration {
    static DELAY: OnceLock<Duration> = OnceLock::new();
    *DELAY.get_or_init(|| {
        std::env::var("VOCALIS_TEST_ENCODE_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_default()
    })
}

/// Encodes one already-captured frame to a flat H.264 bitstream — SPS/PPS (on
/// a keyframe) plus the frame's own NAL units, concatenated exactly as
/// `Decoder::decode` expects to receive them in one call.
pub fn encode_frame(encoder: &mut Encoder, yuv: &YUVBuffer) -> Result<Vec<u8>> {
    let delay = artificial_encode_delay();
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    Ok(encoder.encode(yuv).context("encoding video frame")?.to_vec())
}

/// Test-only hook mirroring `artificial_encode_delay`, for the receive side:
/// if `VOCALIS_TEST_DECODE_DELAY_MS` is set, every `decode_frame` call blocks
/// that long first, simulating a student's machine too slow to decode in
/// real time. Used to verify `student::screen::run_screen_demo_receiver`
/// doesn't accumulate a growing backlog when that happens — see that
/// function's doc comment for why it structurally can't: there's no queue to
/// grow in the first place, only ever one in-flight frame's worth of state.
fn artificial_decode_delay() -> Duration {
    static DELAY: OnceLock<Duration> = OnceLock::new();
    *DELAY.get_or_init(|| {
        std::env::var("VOCALIS_TEST_DECODE_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_default()
    })
}

/// Decodes one reassembled bitstream and, if a picture was produced (decoding
/// a NAL unit doesn't always yield one immediately — see
/// `openh264::decoder::Decoder::decode`'s own docs), returns its RGBA pixels
/// alongside its dimensions.
pub fn decode_frame(decoder: &mut Decoder, bitstream: &[u8]) -> Result<Option<(u32, u32, Vec<u8>)>> {
    let delay = artificial_decode_delay();
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    let Some(image) = decoder.decode(bitstream).context("decoding video frame")? else {
        return Ok(None);
    };
    let (width, height) = image.dimensions();
    let mut rgba = vec![0u8; width * height * 4];
    image.write_rgba8(&mut rgba);
    Ok(Some((width as u32, height as u32, rgba)))
}
