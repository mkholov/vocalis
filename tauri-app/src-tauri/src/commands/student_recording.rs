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
//! Comparing a recording against the teacher's reference (`student::state::reference`,
//! `recording::save_reference`) and sending one to the teacher (`ClientToServer::FileOffer`) live here too
//! now — both real, both reusing unchanged `student::net`/`teacher::net` plumbing: the reference is
//! already fully captured by the time this reads it (`ServerToClient::MaterialPlaying`/`MaterialStopped`,
//! handled in `student::net` exactly like the egui app), and `FileOffer` already has a real, working
//! handler on the teacher's side (`teacher::net::handle_student`, saves the bytes to a real file and logs
//! who sent it) — nothing needed adding to the protocol for either.
//!
//! Recordings are addressed by **file name**, never by path. The webview must
//! not be able to make `read_recording`/`delete_recording` touch an arbitrary
//! file, so a name is only accepted if it exactly matches an entry
//! `recording::list_existing()` returns from the recordings directory.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use lingua_common::ClientToServer;
use serde::Serialize;
use tauri::State;
use vocalis::student::recording;
use vocalis::student::state::{self, ActiveRecording, RecordingEntry};

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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceRecordingDto {
    /// The material's title, exactly as the teacher's `play_material` named it — `None` only if a reference
    /// somehow got cached with no material context (shouldn't happen in practice, not fatal either).
    pub title: Option<String>,
    pub duration_secs: f32,
    /// Same `data:audio/wav;base64,…` shape `readRecording` returns — this plays back through the exact
    /// same `<audio>` element the UI already drives for the student's own recordings, no separate path.
    pub data_url: String,
}

fn build_reference_dto(state_guard: &state::SharedState) -> Option<ReferenceRecordingDto> {
    let reference = state_guard.reference.as_ref()?;
    let bytes = std::fs::read(&reference.path).ok()?;
    Some(ReferenceRecordingDto {
        title: state_guard.material_title.clone(),
        duration_secs: reference.duration_secs,
        data_url: format!("data:audio/wav;base64,{}", BASE64.encode(bytes)),
    })
}

/// The reference recording currently cached for this student — the teacher's own "модельное произношение"
/// material, captured locally the moment it finished playing. Real, not a stub: `ServerToClient::
/// MaterialPlaying`/`MaterialStopped` (`student::net`'s handling of them) and `recording::save_reference`
/// are all unchanged — this only reads back what that pipeline already produces. `None` before any material
/// has been played to this student this session, or if capturing it failed (best-effort, per that code's
/// own doc comment) — not an error, just nothing to compare against yet.
#[tauri::command]
pub fn read_reference_recording(session: State<StudentSessionState>) -> Result<Option<ReferenceRecordingDto>, String> {
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет подключения к преподавателю")?;
    let state_guard = student.app_state.lock().unwrap();
    Ok(build_reference_dto(&state_guard))
}

/// `pub(crate)` only so `student_session.rs`'s own reference-changed poller can build the identical DTO to
/// emit as a real `"reference-updated"` event — not part of the command surface itself.
pub(crate) fn reference_dto_for_poll(state_guard: &state::SharedState) -> Option<ReferenceRecordingDto> {
    build_reference_dto(state_guard)
}

/// Sends a saved recording to the teacher for real — a real `ClientToServer::FileOffer` over this
/// session's already-encrypted control connection, the same message/channel `set_hand_raised`/
/// `submit_chat_message` already use. `teacher::net`'s existing (unchanged) handler for it saves the bytes
/// to a real file on the teacher's machine and logs who sent it in the real chat log, which
/// `teacher_session.rs`'s own poller already reports as a `"chat-message"` event; `teacher_session.rs`'s
/// `list_received_recordings`/`read_received_recording` (new, alongside this) read that same file back for
/// real playback.
///
/// `FileOffer` has no field of its own for "which student sent this" or "compared against which
/// reference" — both are baked straight into the file's own name instead, human-readably: `"<student> —
/// <recording> (эталон: <title>)"`, the `(эталон: …)` part only present if a reference was actually cached
/// when this was sent. That's also literally the file name the teacher ends up with on disk, so it reads
/// the same way whether browsed through the app or a plain file manager.
#[tauri::command]
pub fn send_recording_to_teacher(session: State<StudentSessionState>, name: String) -> Result<(), String> {
    let entry = find_recording(&name)?;
    let data = std::fs::read(&entry.path).map_err(|e| format!("не удалось прочитать запись: {e}"))?;

    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет подключения к преподавателю")?;
    let state_guard = student.app_state.lock().unwrap();
    let reference_title = if state_guard.reference.is_some() { state_guard.material_title.clone() } else { None };
    let to_server = state_guard.to_server.clone();
    drop(state_guard);

    let tx = to_server.ok_or("подключение к преподавателю ещё не готово")?;
    let stem = name.strip_suffix(".wav").unwrap_or(&name);
    let mut label = format!("{} — {stem}", student.student_name);
    if let Some(title) = reference_title {
        label.push_str(&format!(" (эталон: {title})"));
    }
    label.push_str(".wav");

    tx.send(ClientToServer::FileOffer { name: label, data }).map_err(|_| "соединение с преподавателем разорвано".to_string())
}
