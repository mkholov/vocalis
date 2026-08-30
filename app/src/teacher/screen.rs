//! Capture loop for the teacher demonstrating their *own* screen to the class.
//! (Demoing a *student's* screen instead doesn't need a loop here at all — the
//! teacher just relays that student's own `ClientToServer::ScreenFrame` uploads
//! as they arrive, in `teacher::net`'s message handler.)

use lingua_common::{ServerToClient, StudentId};

use crate::screen_capture;

use super::state::AppState;

/// Captures the teacher's own screen at demo quality and fans each frame out to
/// `targets` until `state.screen_demo` is cleared (by the "Stop" button, or by
/// starting a different demo) — checked once per frame rather than driven by a
/// cancellation signal, since the loop already wakes up on that cadence anyway.
pub async fn run_own_screen_demo(state: AppState, targets: Vec<StudentId>) {
    loop {
        {
            let guard = state.lock().unwrap();
            if guard.screen_demo.is_none() {
                return;
            }
        }
        match screen_capture::capture_primary_monitor_jpeg(
            screen_capture::DEMO_PREVIEW_WIDTH,
            screen_capture::DEMO_JPEG_QUALITY,
        ) {
            Ok(jpeg) => {
                let guard = state.lock().unwrap();
                for id in &targets {
                    if let Some(s) = guard.students.get(id) {
                        let _ = s.to_client.send(ServerToClient::ScreenDemoFrame { jpeg: jpeg.clone() });
                    }
                }
            }
            Err(e) => tracing::warn!("teacher screen capture failed: {e:#}"),
        }
        tokio::time::sleep(screen_capture::DEMO_CAPTURE_INTERVAL).await;
    }
}
