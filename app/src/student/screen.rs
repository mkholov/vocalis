use anyhow::Result;
use lingua_common::ClientToServer;
use tokio::sync::mpsc;

use crate::screen_capture;

use super::state::AppState;

/// Periodically captures the primary monitor and sends it to the teacher over the
/// control channel — at the passive monitoring tier normally, or the demo tier
/// while `state.screen_boosted` is set (the rest of the class is watching along,
/// via the teacher relaying these same frames as `ScreenDemoFrame`). Exits once
/// `to_server` is closed (i.e. the control connection dropped).
pub async fn run_screen_capture(state: AppState, to_server: mpsc::UnboundedSender<ClientToServer>) -> Result<()> {
    loop {
        if to_server.is_closed() {
            return Ok(());
        }
        let boosted = state.lock().unwrap().screen_boosted;
        let (width, quality, interval) = if boosted {
            (
                screen_capture::DEMO_PREVIEW_WIDTH,
                screen_capture::DEMO_JPEG_QUALITY,
                screen_capture::DEMO_CAPTURE_INTERVAL,
            )
        } else {
            (
                screen_capture::MONITOR_PREVIEW_WIDTH,
                screen_capture::MONITOR_JPEG_QUALITY,
                screen_capture::MONITOR_CAPTURE_INTERVAL,
            )
        };
        match screen_capture::capture_primary_monitor_jpeg(width, quality) {
            Ok(jpeg) => {
                if to_server.send(ClientToServer::ScreenFrame { jpeg }).is_err() {
                    return Ok(());
                }
            }
            Err(e) => tracing::warn!("screen capture failed: {e:#}"),
        }
        tokio::time::sleep(interval).await;
    }
}
