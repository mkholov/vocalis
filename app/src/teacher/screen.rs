//! Screen-demo video, teacher side. Two independent paths:
//!
//! - The teacher demoing their *own* screen: [`run_own_screen_demo`] captures,
//!   H.264-encodes, and fans each frame out (UDP, per-recipient encrypted)
//!   directly to every target — the same shape as `mic::run_mic_broadcast`,
//!   just on `SCREEN_VIDEO_PORT` instead of `MIC_PORT`.
//! - Demoing a *student's* screen instead needs no capture loop here at all:
//!   [`run_screen_relay_receiver`] just re-encrypts and forwards that
//!   student's own encoded packets on to the audience as they arrive, never
//!   decoding them.

use anyhow::Result;
use lingua_common::{crypto, StudentId, SCREEN_VIDEO_PORT, TEACHER_SCREEN_UPLOAD_PORT};
use tokio::net::UdpSocket;

use crate::screen_capture::MonitorCapture;
use crate::video;

use super::state::{self, AppState};

/// Captures the teacher's own screen at `video::VIDEO_FPS`, H.264-encodes it,
/// and UDP-sends each frame's packets to `targets` until `state.screen_demo`
/// is cleared (by the "Stop" button, or by starting a different demo) —
/// checked once per capture tick rather than driven by a cancellation signal,
/// since the loop already wakes up on that cadence anyway.
pub async fn run_own_screen_demo(state: AppState, targets: Vec<StudentId>) {
    let socket = match UdpSocket::bind(("0.0.0.0", 0)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to bind screen-demo send socket: {e:#}");
            return;
        }
    };
    let mut encoder = match video::new_encoder() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to create H.264 encoder: {e:#}");
            return;
        }
    };
    let capture = match MonitorCapture::primary() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("failed to resolve monitor for screen demo: {e:#}");
            return;
        }
    };
    let mut frame_seq: u32 = 0;
    let mut ticker = tokio::time::interval(video::VIDEO_CAPTURE_INTERVAL);

    loop {
        ticker.tick().await;
        {
            let guard = state.lock().unwrap();
            if guard.screen_demo.is_none() {
                return;
            }
        }

        let yuv = match video::capture_frame_yuv(&capture) {
            Ok(y) => y,
            Err(e) => {
                tracing::warn!("teacher screen capture failed: {e:#}");
                continue;
            }
        };
        let bitstream = match video::encode_frame(&mut encoder, &yuv) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("H.264 encode failed: {e:#}");
                continue;
            }
        };
        let packets = lingua_common::encode_video_packets(frame_seq, &bitstream);
        frame_seq = frame_seq.wrapping_add(1);

        let addrs_with_keys = {
            let guard = state.lock().unwrap();
            guard.addrs_with_keys(&targets, SCREEN_VIDEO_PORT)
        };
        for packet in &packets {
            for (addr, key) in &addrs_with_keys {
                let encrypted = crypto::encrypt(key, packet);
                let _ = socket.send_to(&encrypted, addr).await;
            }
        }
    }
}

/// Always-on relay for a *student*-sourced screen demo: receives that
/// student's own encoded H.264 packets on `TEACHER_SCREEN_UPLOAD_PORT` and
/// forwards each one, byte-for-byte, on to every current demo target — no
/// decode/re-encode, just decrypt-with-the-presenter's-key and re-encrypt per
/// recipient, mirroring how `mic::run_mic_broadcast` fans out one encoded
/// audio frame. Idle (packets silently dropped) whenever no student-sourced
/// demo is active, same as `listen::run_listen_receiver` when nobody's being
/// listened to.
pub async fn run_screen_relay_receiver(state: AppState) -> Result<()> {
    let recv_socket = UdpSocket::bind(("0.0.0.0", TEACHER_SCREEN_UPLOAD_PORT)).await?;
    let send_socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let mut buf = [0u8; 2048];
    loop {
        let (len, _from) = recv_socket.recv_from(&mut buf).await?;

        let Some((presenter_key, targets_with_keys)) = ({
            let guard = state.lock().unwrap();
            guard.screen_demo.as_ref().and_then(|demo| match demo.source {
                state::ScreenDemoSource::Student(presenter_id) => guard
                    .students
                    .get(&presenter_id)
                    .map(|s| (s.session_key, guard.addrs_with_keys(&demo.targets, SCREEN_VIDEO_PORT))),
                state::ScreenDemoSource::Teacher => None,
            })
        }) else {
            continue;
        };

        let Ok(plaintext) = crypto::decrypt(&presenter_key, &buf[..len]) else {
            continue;
        };
        for (addr, key) in &targets_with_keys {
            let encrypted = crypto::encrypt(key, &plaintext);
            let _ = send_socket.send_to(&encrypted, addr).await;
        }
    }
}
