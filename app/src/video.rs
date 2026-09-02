//! H.264 encode/decode for the screen-demo video pipeline — the codec layer
//! `codec-check/` verified builds and round-trips correctly on both macOS and
//! Windows, now wired into the real app. This module is purely the openh264
//! capture-frame <-> H.264 plumbing shared by both roles (teacher's own-screen
//! sender, a presenting student's uploader, and every student's receiver);
//! the UDP wire format (packetization/reassembly) lives in
//! `lingua_common::video`, and screen capture itself in `screen_capture`.

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
pub const VIDEO_CAPTURE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000 / VIDEO_FPS as u64);
/// Downscale target — matches the old JPEG demo tier's width, still
/// comfortably readable for following along without pushing encode time or
/// bandwidth much past what that tier already cost.
pub const VIDEO_WIDTH: u32 = 1280;
const VIDEO_BITRATE_BPS: u32 = 1_500_000;
/// One keyframe roughly every 3s at `VIDEO_FPS`. Bounds how long a dropped
/// packet's glitch can persist before the stream self-heals (see
/// `lingua_common::video`'s module doc), without spending too much bandwidth
/// on keyframes given how cheaply delta frames compress for mostly-static
/// screen content.
const KEYFRAME_INTERVAL_FRAMES: u32 = VIDEO_FPS * 3;

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

/// Captures the primary monitor at `VIDEO_WIDTH` and converts it to YUV420,
/// ready for `encode_frame`. Takes an already-resolved `MonitorCapture`
/// (rather than resolving the monitor itself) so a capture loop can reuse the
/// same handle across every tick — see `MonitorCapture`'s doc comment for why
/// that matters.
pub fn capture_frame_yuv(capture: &MonitorCapture) -> Result<YUVBuffer> {
    let rgba = capture.capture_rgba_even(VIDEO_WIDTH)?;
    let (width, height) = rgba.dimensions();
    let source = RgbaSliceU8::new(rgba.as_raw(), (width as usize, height as usize));
    Ok(YUVBuffer::from_rgba8_source(source))
}

/// Encodes one already-captured frame to a flat H.264 bitstream — SPS/PPS (on
/// a keyframe) plus the frame's own NAL units, concatenated exactly as
/// `Decoder::decode` expects to receive them in one call.
pub fn encode_frame(encoder: &mut Encoder, yuv: &YUVBuffer) -> Result<Vec<u8>> {
    Ok(encoder.encode(yuv).context("encoding video frame")?.to_vec())
}

/// Decodes one reassembled bitstream and, if a picture was produced (decoding
/// a NAL unit doesn't always yield one immediately — see
/// `openh264::decoder::Decoder::decode`'s own docs), returns its RGBA pixels
/// alongside its dimensions.
pub fn decode_frame(decoder: &mut Decoder, bitstream: &[u8]) -> Result<Option<(u32, u32, Vec<u8>)>> {
    let Some(image) = decoder.decode(bitstream).context("decoding video frame")? else {
        return Ok(None);
    };
    let (width, height) = image.dimensions();
    let mut rgba = vec![0u8; width * height * 4];
    image.write_rgba8(&mut rgba);
    Ok(Some((width as u32, height as u32, rgba)))
}
