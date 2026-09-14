//! Screen-capture command — a thin wrapper around `vocalis::screen_capture`,
//! the same `xcap`-based capture the screen-demo feature and the student's
//! passive-monitoring upload both already use. Returns one real JPEG-encoded
//! frame of the primary monitor; no live streaming yet (that's roadmap step 7
//! — bridging a continuous low-latency feed into the webview is a separate,
//! harder problem from "can the command reach the capture code at all").

use vocalis::screen_capture::MonitorCapture;

/// Width chosen to match the existing "passive monitoring" tier
/// (`screen_capture::MONITOR_PREVIEW_WIDTH`) rather than inventing a new
/// number — this command exercises the same capture path, just once instead
/// of on a timer.
#[tauri::command]
pub async fn capture_screen_preview() -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let capture = MonitorCapture::primary().map_err(|e| e.to_string())?;
        capture
            .capture_jpeg(vocalis::screen_capture::MONITOR_PREVIEW_WIDTH, vocalis::screen_capture::MONITOR_JPEG_QUALITY)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("capture task panicked: {e}"))?
}
