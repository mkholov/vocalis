//! The student's own voice recordings (step 7.5 item 6 — `vocalis_roadmap.md`,
//! section 8): record, list, play back, delete. Reuses `student::recording`
//! (`save`/`list_existing`/`delete` — the WAV writer and the on-disk library
//! the egui student app already uses) and the existing recording *tap* in
//! `student::audio::run_outbound_and_group_audio` unchanged: recording isn't a
//! second capture, it's `SharedState.recording` being set, which makes that
//! already-running loop append the mic's raw native-rate PCM to it (see its
//! "Local self-recording taps the same raw, native-rate PCM" comment). So
//! `start_recording`/`stop_recording` need the live student session — that's
//! where the mic capture is — while listing, playing and deleting only touch
//! files on disk.
//!
//! Not here on purpose: comparing a recording against the teacher's reference
//! (`student::state::reference`, `recording::save_reference`) and sending one
//! to the teacher (`ClientToServer::FileOffer`) — separate follow-ups.
//!
//! Recordings are addressed by **file name**, never by path. The webview must
//! not be able to make `read_recording`/`delete_recording` touch an arbitrary
//! file, so a name is only accepted if it exactly matches an entry
//! `recording::list_existing()` returns from the recordings directory.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tauri::State;
use vocalis::student::recording;
use vocalis::student::state::{ActiveRecording, RecordingEntry};

use super::student_session::StudentSessionState;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    /// The file name (`recording_<epoch>.wav`) — the recording's id everywhere below.
    pub name: String,
    pub duration_secs: f32,
    /// Unix seconds parsed out of the file name, if it has the `recording_<epoch>.wav`
    /// shape `recording::save` produces — lets the UI show when it was made.
    pub recorded_at_epoch: Option<u64>,
}

fn to_dto(entry: &RecordingEntry) -> Option<RecordingDto> {
    let name = entry.path.file_name()?.to_string_lossy().to_string();
    let recorded_at_epoch = name.strip_prefix("recording_").and_then(|r| r.strip_suffix(".wav")).and_then(|e| e.parse().ok());
    Some(RecordingDto { name, duration_secs: entry.duration_secs, recorded_at_epoch })
}

/// Resolves a file name from the frontend to a recording that really is in the
/// recordings directory — the only way a name becomes a path.
fn find_recording(name: &str) -> Result<RecordingEntry, String> {
    recording::list_existing()
        .into_iter()
        .find(|r| r.path.file_name().is_some_and(|n| n.to_string_lossy() == name))
        .ok_or_else(|| "запись не найдена".to_string())
}

/// Starts recording the student's mic. Idempotent. Errors if there's no live
/// session or this machine has no usable input device (`outbound_mic` is `None`
/// then — the same non-fatal "no mic" state listen-in and groups already treat as
/// unavailable): there'd be nothing to record.
#[tauri::command]
pub fn start_recording(session: State<StudentSessionState>) -> Result<(), String> {
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет подключения к преподавателю")?;
    let mic = student.outbound_mic.as_ref().ok_or("микрофон недоступен")?;
    let mut state = student.app_state.lock().unwrap();
    if state.recording.is_none() {
        state.recording = Some(ActiveRecording { samples: Vec::new(), sample_rate: mic.native_rate });
    }
    Ok(())
}

/// Saves what `active` captured as a WAV (`recording::save`); `None` if it captured
/// nothing (stopping instantly would otherwise leave an empty 0-second file in the
/// library). Shared by `stop_recording` and `disconnect_student_session`, which must
/// not let a recording in progress vanish with the session's state.
pub fn save_active(active: ActiveRecording) -> Result<Option<RecordingDto>, String> {
    if active.samples.is_empty() {
        return Ok(None);
    }
    let entry = recording::save(&active.samples, active.sample_rate).map_err(|e| format!("не удалось сохранить запись: {e:#}"))?;
    Ok(to_dto(&entry))
}

/// Ends the current recording and saves it. `None` if nothing was being recorded
/// or nothing was captured.
#[tauri::command]
pub fn stop_recording(session: State<StudentSessionState>) -> Result<Option<RecordingDto>, String> {
    let guard = session.0.lock().unwrap();
    let Some(student) = guard.as_ref() else { return Ok(None) };
    let active = student.app_state.lock().unwrap().recording.take();
    match active {
        Some(active) => save_active(active),
        None => Ok(None),
    }
}

/// The saved recordings, newest first — read straight from disk, so they
/// survive an app restart exactly like the egui app's list does.
#[tauri::command]
pub fn list_recordings() -> Vec<RecordingDto> {
    recording::list_existing().iter().filter_map(to_dto).collect()
}

/// A recording's audio as a `data:audio/wav;base64,…` URL, for the webview's own
/// `<audio>`/`Audio` playback — playing it in-app rather than handing it to an
/// external player (which is what the egui app's `open::that` does).
#[tauri::command]
pub fn read_recording(name: String) -> Result<String, String> {
    let entry = find_recording(&name)?;
    let bytes = std::fs::read(&entry.path).map_err(|e| format!("не удалось прочитать запись: {e}"))?;
    Ok(format!("data:audio/wav;base64,{}", BASE64.encode(bytes)))
}

#[tauri::command]
pub fn delete_recording(name: String) -> Result<(), String> {
    let entry = find_recording(&name)?;
    recording::delete(&entry.path).map_err(|e| format!("не удалось удалить запись: {e:#}"))
}
