//! Screen capture shared by both roles: the student's passive monitoring upload
//! (`student::screen`) and the screen-demo video feature's H.264 capture (see
//! `app::video`, `teacher::screen`, `student::screen`).

use std::time::Duration;

use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::RgbaImage;

/// Passive monitoring tier — cheap enough to run continuously for every connected
/// student regardless of whether anyone's actually looking. 800px/quality 75 was
/// picked by measuring a real desktop screenshot: readable menu bars/body text,
/// ~10-40KB/frame depending on content, trivial even for a full class at 2fps.
pub const MONITOR_PREVIEW_WIDTH: u32 = 800;
pub const MONITOR_JPEG_QUALITY: u8 = 75;
pub const MONITOR_CAPTURE_INTERVAL: Duration = Duration::from_millis(500);

/// A handle to the primary monitor, resolved once and reused for every
/// subsequent capture. `xcap::Monitor::all()` re-enumerates and re-queries
/// every display on the system, which turns out to cost the better part of a
/// second on some setups — fine to pay once per capture *task*, ruinous to
/// pay on every single tick of a 15fps video loop (or even a 2fps monitoring
/// one). Every capture loop in the app should create one of these at startup
/// and keep reusing it rather than re-resolving the monitor each frame.
pub struct MonitorCapture {
    monitor: xcap::Monitor,
}

impl MonitorCapture {
    pub fn primary() -> Result<Self> {
        let monitors = xcap::Monitor::all().context("listing monitors")?;
        let monitor = monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .context("no monitor found")?
            .clone();
        Ok(Self { monitor })
    }

    /// Captures the monitor and downsizes it to `width` (preserving aspect
    /// ratio), shared by both the JPEG and raw-RGBA capture paths below.
    fn capture_resized(&self, width: u32) -> Result<RgbaImage> {
        let image = self.monitor.capture_image().context("capturing monitor image")?;
        let scale = width as f32 / image.width() as f32;
        let height = ((image.height() as f32) * scale).round().max(1.0) as u32;
        Ok(image::imageops::resize(&image, width, height, FilterType::Triangle))
    }

    /// Captures, downsizes to `width` and JPEG-encodes at `quality`.
    pub fn capture_jpeg(&self, width: u32, quality: u8) -> Result<Vec<u8>> {
        let resized = self.capture_resized(width)?;
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

    /// Captures, downsizes to `width`, and crops off at most one trailing
    /// row/column so both dimensions come out even — YUV420 (and so H.264)
    /// requires that, and losing a single edge pixel is invisible.
    pub fn capture_rgba_even(&self, width: u32) -> Result<RgbaImage> {
        let resized = self.capture_resized(width)?;
        let even_width = resized.width() - (resized.width() % 2);
        let even_height = resized.height() - (resized.height() % 2);
        Ok(image::imageops::crop_imm(&resized, 0, 0, even_width, even_height).to_image())
    }
}
