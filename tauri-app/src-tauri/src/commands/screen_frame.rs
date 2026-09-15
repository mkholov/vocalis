//! Shared JPEG+base64 framing for screen-demo video frames — used by both
//! the self-preview loop (`screen_demo.rs`) and the real network receiver
//! (`student_session.rs`), so the two paths can't drift apart on quality,
//! format, or the event payload shape. Quality/format choices (q=75,
//! base64/emit `data:image/jpeg;base64,...`) come from `../../video-bench/`'s
//! measurement — see that crate's report.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;

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

/// JPEG-encodes an already-decoded RGBA frame and wraps it as a data URL.
pub fn jpeg_data_url(width: u32, height: u32, rgba: &[u8]) -> Result<String, String> {
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("bad rgba buffer")?;
    let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", BASE64.encode(&jpeg_bytes)))
}
