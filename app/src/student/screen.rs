use std::time::Duration;

use anyhow::{Context, Result};
use image::imageops::FilterType;
use lingua_common::ClientToServer;
use tokio::sync::mpsc;

// 800px wide / quality 75 was picked by actually measuring: on a real desktop
// screenshot this comes out to roughly 10-20KB per frame (~20-40KB/s per student
// at the 2fps capture rate below) — trivial even for a full class on a school LAN
// — while being wide enough that menu bars and body text stay legible in the
// teacher's focus view. The previous 320px/quality 55 was thumbnail-sized and left
// text an illegible blur once scaled up; there's no need to go anywhere near
// full-HD to fix that.
const PREVIEW_WIDTH: u32 = 800;
const JPEG_QUALITY: u8 = 75;
const CAPTURE_INTERVAL: Duration = Duration::from_millis(500);

/// Periodically captures the primary monitor, downsizes it to a readable preview and
/// sends it to the teacher over the control channel. Exits once `to_server` is closed
/// (i.e. the control connection dropped).
pub async fn run_screen_capture(to_server: mpsc::UnboundedSender<ClientToServer>) -> Result<()> {
    loop {
        if to_server.is_closed() {
            return Ok(());
        }
        match capture_thumbnail_jpeg() {
            Ok(jpeg) => {
                if to_server.send(ClientToServer::ScreenFrame { jpeg }).is_err() {
                    return Ok(());
                }
            }
            Err(e) => tracing::warn!("screen capture failed: {e:#}"),
        }
        tokio::time::sleep(CAPTURE_INTERVAL).await;
    }
}

fn capture_thumbnail_jpeg() -> Result<Vec<u8>> {
    let monitors = xcap::Monitor::all().context("listing monitors")?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .context("no monitor found")?;
    let image = monitor.capture_image().context("capturing monitor image")?;

    let scale = PREVIEW_WIDTH as f32 / image.width() as f32;
    let height = ((image.height() as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&image, PREVIEW_WIDTH, height, FilterType::Triangle);
    // JPEG has no alpha channel, so drop it before encoding.
    let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();

    let mut jpeg_bytes = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpeg_bytes)
}
