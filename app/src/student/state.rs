use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use lingua_common::{AssignmentContent, AssignmentId, AssignmentKind, ClientToServer};
use tokio::sync::mpsc;

pub struct DiscoveredTeacher {
    pub name: String,
    pub last_seen: Instant,
}

pub struct ChatEntry {
    pub from: String,
    pub text: String,
}

pub struct ReceivedFile {
    pub name: String,
    pub path: std::path::PathBuf,
}

pub struct AssignmentEntry {
    pub id: AssignmentId,
    pub title: String,
    pub kind: AssignmentKind,
    pub done: bool,
    /// `None` for a plain legacy/label-only assignment (currently just Dialogue
    /// quick-sends) — shown and completed the old way, a bare "mark done" click.
    pub content: Option<AssignmentContent>,
    /// (correct, total) right after finishing a `Test`, for immediate feedback —
    /// the teacher gets the same numbers via `ClientToServer::TestResult`.
    pub last_score: Option<(u32, u32)>,
}

/// A recording of the student's own mic in progress — filled in by
/// `audio::run_outbound_and_group_audio` as it taps the same raw PCM chunks it
/// already consumes for the network pipeline, at the mic's native sample rate
/// (no need to downsample to the Opus voice rate for a local file).
pub struct ActiveRecording {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

/// A recording saved to disk (`recording::save`), ready to play back, delete, or
/// send to the teacher.
pub struct RecordingEntry {
    pub path: std::path::PathBuf,
    pub duration_secs: f32,
}

#[derive(Default)]
pub struct SharedState {
    /// Keyed by the teacher's TCP control address.
    pub discovered: HashMap<SocketAddr, DiscoveredTeacher>,
    pub connected_teacher: Option<String>,
    pub teacher_addr: Option<IpAddr>,
    pub connecting: bool,
    pub locked_message: Option<String>,
    /// Set alongside `locked_message` when the current lock is test mode —
    /// drives the focus-loss monitoring/resistance loop in `StudentApp::update`.
    pub test_mode_active: bool,
    pub peer_addrs: Vec<SocketAddr>,
    pub peer_names: Vec<String>,
    pub uploading_to_teacher: bool,
    pub to_server: Option<mpsc::UnboundedSender<ClientToServer>>,
    pub chat_log: Vec<ChatEntry>,
    pub received_files: Vec<ReceivedFile>,
    pub last_error: Option<String>,
    pub mic_locked: bool,
    pub needs_help: bool,
    pub assignments: Vec<AssignmentEntry>,
    /// Set while the teacher has opened a private two-way intercom with this
    /// student specifically — drives a distinct "teacher is talking to you
    /// personally" banner, separate from the general class broadcast.
    pub intercom_active: bool,
    /// Set while the mic is being recorded to a local WAV file; `None` otherwise.
    pub recording: Option<ActiveRecording>,
    /// Recordings saved to disk — loaded from disk at startup, appended to as new
    /// ones are saved.
    pub saved_recordings: Vec<RecordingEntry>,
    /// "Model pronunciation" feature: title of the material the teacher most
    /// recently played (or is currently playing) to this student — drives the
    /// "Повторите за диктором" prompt. Kept until a new material starts or the
    /// student disconnects (not cleared just because playback ended, since the
    /// prompt is meant to stick around for "recently played" too).
    pub material_title: Option<String>,
    /// Whether `material_title`'s clip is still actively playing right now (vs.
    /// having already finished) — only affects prompt wording.
    pub material_playing: bool,
    /// While a material is playing, the incoming broadcast is opportunistically
    /// cached here (same tap idea as `recording`, just on the receive side) so it
    /// can be offered back as a locally-playable reference. Best-effort: absent if
    /// the student wasn't connected for the whole clip, or nothing has played yet.
    pub reference_capture: Option<ActiveRecording>,
    /// The finalized reference recording for `material_title`, once caching
    /// completes (`ServerToClient::MaterialStopped` arrives).
    pub reference: Option<RecordingEntry>,
    /// Set while *this* student's own screen is the one being demoed to the rest
    /// of the class — tells `screen::run_screen_capture` to switch to demo-grade
    /// quality/rate, and drives a small "your screen is being shown" notice.
    pub screen_boosted: bool,
    /// Display name of whoever is presenting, while a screen demo (teacher's own
    /// or a relayed student's) is being shown to this student. `None` = no demo.
    pub demo_presenter: Option<String>,
    /// Latest frame of the active demo, and a version counter bumped on each new
    /// frame — same "poll and diff" pattern the teacher's grid uses for student
    /// screen thumbnails, so the GUI only re-uploads the texture when it changes.
    pub last_demo_frame_jpeg: Option<Vec<u8>>,
    pub demo_frame_version: u64,
}

pub type AppState = Arc<Mutex<SharedState>>;
