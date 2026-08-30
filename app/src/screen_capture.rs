//! Screen capture shared by both roles: the student's passive monitoring upload
//! (`student::screen`) and, for the screen-demo feature, both the teacher's own
//! screen and a demoed student's boosted upload (`teacher::screen`,
//! `student::screen` again). One `xcap` capture → resize → JPEG-encode function,
//! parameterized by resolution/quality so each caller picks its own tier.

use std::time::Duration;

use anyhow::{Context, Result};
use image::imageops::FilterType;

/// Passive monitoring tier — cheap enough to run continuously for every connected
/// student regardless of whether anyone's actually looking. 800px/quality 75 was
/// picked by measuring a real desktop screenshot: readable menu bars/body text,
/// ~10-40KB/frame depending on content, trivial even for a full class at 2fps.
pub const MONITOR_PREVIEW_WIDTH: u32 = 800;
pub const MONITOR_JPEG_QUALITY: u8 = 75;
pub const MONITOR_CAPTURE_INTERVAL: Duration = Duration::from_millis(500);

/// Active screen-demo tier — used for the teacher's own screen and for whichever
/// student is currently being shown to the rest of the class. Picked by measuring
/// a real, text-heavy screenshot (code/terminal, not a blank desktop): 1280px/
/// quality 80 comes out to roughly 90KB/frame. At the 2.5fps interval below, one
/// demoed stream fanned out to a full class of ~30 is on the order of 50-55 Mbps
/// aggregate egress from the teacher's machine — a deliberate step up from
/// passive monitoring (worth it for something everyone needs to actually read
/// along with), but still well short of what real 1080p/30fps screen share would
/// cost, and sized for the wired classroom LAN this app targets rather than
/// stretching to cover a saturated Wi-Fi worst case.
pub const DEMO_PREVIEW_WIDTH: u32 = 1280;
pub const DEMO_JPEG_QUALITY: u8 = 80;
pub const DEMO_CAPTURE_INTERVAL: Duration = Duration::from_millis(400);

/// Captures the primary monitor, downsizes it to `width` and JPEG-encodes at
/// `quality`.
pub fn capture_primary_monitor_jpeg(width: u32, quality: u8) -> Result<Vec<u8>> {
    let monitors = xcap::Monitor::all().context("listing monitors")?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .context("no monitor found")?;
    let image = monitor.capture_image().context("capturing monitor image")?;

    let scale = width as f32 / image.width() as f32;
    let height = ((image.height() as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&image, width, height, FilterType::Triangle);
    // JPEG has no alpha channel, so drop it before encoding.
    let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();

    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, quality);
    encoder.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpeg_bytes)
}
