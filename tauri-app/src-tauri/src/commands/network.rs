//! Network command — a thin wrapper around `lingua_common::run_discovery_listener`,
//! the same LAN-broadcast discovery the student client's "Доступные
//! преподаватели в сети" list already uses. A real UDP listen, not a stub:
//! returns whatever teacher announcements actually arrive on this network
//! within the timeout, which is legitimately empty if none are broadcasting.

use serde::Serialize;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredTeacherDto {
    pub ip: String,
    pub teacher_name: String,
    pub control_port: u16,
}

/// Listens for teacher discovery broadcasts for `timeout_ms` (capped at 30s
/// so a stray huge value from the frontend can't hang a command
/// indefinitely) and returns everyone seen in that window.
#[tauri::command]
pub async fn discover_teachers(timeout_ms: u64) -> Result<Vec<DiscoveredTeacherDto>, String> {
    let timeout = Duration::from_millis(timeout_ms.min(30_000));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let listener = tauri::async_runtime::spawn(lingua_common::run_discovery_listener(tx));

    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            sighting = rx.recv() => {
                match sighting {
                    Some((addr, announce)) => found.push(DiscoveredTeacherDto {
                        ip: addr.ip().to_string(),
                        teacher_name: announce.teacher_name,
                        control_port: announce.control_port,
                    }),
                    None => break,
                }
            }
        }
    }

    listener.abort();
    Ok(found)
}

/// The fixed TCP control port every teacher session listens on (`lingua_common::CONTROL_PORT`) — a real
/// discovered teacher's own `controlPort` already comes with its sighting, but the manual "ввести адрес
/// вручную" fallback (`StudentFlow.tsx`, for when LAN broadcast discovery can't reach a student at all —
/// e.g. school Wi-Fi with client/AP isolation) only ever has an IP to go on, never a discovery packet to
/// read a port out of. Exposed as a real command rather than hardcoded on the frontend so there's one
/// source of truth for it, not two that could silently drift apart.
#[tauri::command]
pub fn control_port() -> u16 {
    lingua_common::CONTROL_PORT
}
