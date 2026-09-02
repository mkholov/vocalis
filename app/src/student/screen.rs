use std::net::IpAddr;

use anyhow::Result;
use lingua_common::{crypto, ClientToServer, SessionKey, SCREEN_VIDEO_PORT, TEACHER_SCREEN_UPLOAD_PORT};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::screen_capture::{self, MonitorCapture};
use crate::video;

use super::state::{AppState, DemoFrame};

/// Periodically captures the primary monitor and sends it to the teacher over
/// the control channel, for the passive per-student thumbnail in the teacher's
/// grid — always at the same light cadence/quality, regardless of whether this
/// student's screen also happens to be live-demoed to the class right now (see
/// `run_video_upload` for that separate, independent H.264 stream). Exits once
/// `to_server` is closed (i.e. the control connection dropped).
pub async fn run_screen_capture(to_server: mpsc::UnboundedSender<ClientToServer>) -> Result<()> {
    let capture = MonitorCapture::primary()?;
    loop {
        if to_server.is_closed() {
            return Ok(());
        }
        match capture.capture_jpeg(screen_capture::MONITOR_PREVIEW_WIDTH, screen_capture::MONITOR_JPEG_QUALITY) {
            Ok(jpeg) => {
                if to_server.send(ClientToServer::ScreenFrame { jpeg }).is_err() {
                    return Ok(());
                }
            }
            Err(e) => tracing::warn!("screen capture failed: {e:#}"),
        }
        tokio::time::sleep(screen_capture::MONITOR_CAPTURE_INTERVAL).await;
    }
}

/// Captures this student's own screen, H.264-encodes it, and UDP-streams it to
/// the teacher's `TEACHER_SCREEN_UPLOAD_PORT` for relaying on to the rest of
/// the class — the outbound leg of a student-sourced screen demo, started and
/// stopped by `net::connect_to_teacher` in response to
/// `ServerToClient::Start`/`StopVideoUpload`. Entirely independent of
/// `run_screen_capture`'s passive JPEG monitoring upload, which keeps running
/// unchanged the whole time this is also active. `teacher_ip`/`key` are fixed
/// for the task's lifetime (mirrors `mic::run_intercom_send`'s shape) since
/// neither can change without the connection itself being torn down, which
/// also tears down this task via `AbortOnDrop`.
pub async fn run_video_upload(teacher_ip: IpAddr, key: SessionKey) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    let target = (teacher_ip, TEACHER_SCREEN_UPLOAD_PORT);
    let capture = MonitorCapture::primary()?;
    let mut encoder = video::new_encoder()?;
    let mut frame_seq: u32 = 0;
    let mut ticker = tokio::time::interval(video::VIDEO_CAPTURE_INTERVAL);

    loop {
        ticker.tick().await;
        let yuv = match video::capture_frame_yuv(&capture) {
            Ok(y) => y,
            Err(e) => {
                tracing::warn!("screen capture for video upload failed: {e:#}");
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
        for packet in &packets {
            let encrypted = crypto::encrypt(&key, packet);
            let _ = socket.send_to(&encrypted, target).await;
        }
    }
}

/// Always-on receiver for the class-wide screen-demo video stream (whether
/// its source is the teacher's own screen or a relayed student's) — decodes
/// each reassembled H.264 frame into RGBA and stores it (plus a bumped
/// version counter) for the UI to upload as a texture, off the UI thread so a
/// 15fps video stream never blocks a frame render. Idle whenever no demo is
/// running; the steady state between demos is simply nothing arriving.
pub async fn run_screen_demo_receiver(state: AppState) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", SCREEN_VIDEO_PORT)).await?;
    let mut decoder = video::new_decoder()?;
    let mut reassembler = lingua_common::video::FrameReassembler::new();
    let mut buf = [0u8; 2048];
    loop {
        let (len, _from) = socket.recv_from(&mut buf).await?;
        let Some(key) = state.lock().unwrap().session_key else {
            continue;
        };
        let Ok(plaintext) = crypto::decrypt(&key, &buf[..len]) else {
            continue;
        };
        let Some((header, payload)) = lingua_common::split_video_packet(&plaintext) else {
            continue;
        };
        let Some(bitstream) = reassembler.push(header, payload) else {
            continue;
        };
        match video::decode_frame(&mut decoder, &bitstream) {
            Ok(Some((width, height, rgba))) => {
                let mut guard = state.lock().unwrap();
                guard.demo_frame = Some(DemoFrame { width, height, rgba: rgba.into() });
                guard.demo_frame_version = guard.demo_frame_version.wrapping_add(1);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("H.264 decode failed: {e:#}"),
        }
    }
}
