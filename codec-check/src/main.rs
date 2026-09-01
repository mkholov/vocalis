//! Standalone check that `openh264` actually builds and works on this
//! platform — step one of maybe eventually replacing the screen-demo feature's
//! periodic JPEG snapshots (`app::screen_capture`, untouched by this crate)
//! with real H.264 video. Captures one real screen frame via `xcap` (the same
//! capture crate the app already uses), round-trips it through an
//! openh264 encode -> decode, and writes both the original and the
//! decoded-after-encoding frame as PNGs so the two can be compared by eye.
//!
//! Not wired into networking or the rest of the app in any way — this only
//! proves the codec itself works, on whatever machine/OS runs it.

use anyhow::{Context, Result};
use image::RgbaImage;
use openh264::decoder::Decoder;
use openh264::encoder::Encoder;
use openh264::formats::{RgbaSliceU8, YUVBuffer, YUVSource};

/// H.264 (and this crate's YUV420 conversion) needs even width/height —
/// rounds a captured frame down to the nearest even dimensions by cropping
/// off at most one trailing row/column, which is invisible for a visual
/// comparison check like this one.
fn even_crop(image: RgbaImage) -> RgbaImage {
    let width = image.width() - (image.width() % 2);
    let height = image.height() - (image.height() % 2);
    image::imageops::crop_imm(&image, 0, 0, width, height).to_image()
}

fn capture_primary_monitor() -> Result<RgbaImage> {
    let monitors = xcap::Monitor::all().context("listing monitors")?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .context("no monitor found")?;
    let image = monitor.capture_image().context("capturing monitor image")?;
    Ok(even_crop(image))
}

fn main() -> Result<()> {
    let out_dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let out_dir = std::path::Path::new(&out_dir);
    std::fs::create_dir_all(out_dir).context("creating output directory")?;

    let original = capture_primary_monitor()?;
    let (width, height) = (original.width(), original.height());
    println!("captured {width}x{height} frame from the primary monitor");

    let original_path = out_dir.join("original.png");
    original.save(&original_path).context("saving original.png")?;
    println!("wrote {}", original_path.display());

    let rgba_source = RgbaSliceU8::new(original.as_raw(), (width as usize, height as usize));
    let yuv = YUVBuffer::from_rgba8_source(rgba_source);

    let mut encoder = Encoder::new().context("creating openh264 encoder")?;
    let bitstream = encoder.encode(&yuv).context("encoding frame to H.264")?.to_vec();
    let raw_len = (width as usize) * (height as usize) * 4;
    println!(
        "encoded to {} bytes of H.264 (raw RGBA was {} bytes, {:.1}x smaller)",
        bitstream.len(),
        raw_len,
        raw_len as f64 / bitstream.len() as f64
    );

    let mut decoder = Decoder::new().context("creating openh264 decoder")?;
    let decoded = decoder
        .decode(&bitstream)
        .context("decoding H.264 bitstream")?
        .context("decoder produced no image for a single-frame IDR bitstream")?;
    let (decoded_width, decoded_height) = decoded.dimensions();
    anyhow::ensure!(
        (decoded_width, decoded_height) == (width as usize, height as usize),
        "decoded dimensions {decoded_width}x{decoded_height} don't match the {width}x{height} original"
    );

    let mut rgba_out = vec![0u8; decoded_width * decoded_height * 4];
    decoded.write_rgba8(&mut rgba_out);
    let decoded_image = RgbaImage::from_raw(decoded_width as u32, decoded_height as u32, rgba_out)
        .context("decoded RGBA buffer had the wrong size for its own dimensions")?;

    let decoded_path = out_dir.join("decoded_roundtrip.png");
    decoded_image.save(&decoded_path).context("saving decoded_roundtrip.png")?;
    println!("wrote {}", decoded_path.display());

    println!("\nOK: openh264 encode -> decode round-trip succeeded.");
    println!("Compare {} and {} by eye.", original_path.display(), decoded_path.display());
    Ok(())
}
