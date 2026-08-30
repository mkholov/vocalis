use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use lingua_common::{AssignmentId, AssignmentKind, ClientToServer};
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
}

pub type AppState = Arc<Mutex<SharedState>>;
