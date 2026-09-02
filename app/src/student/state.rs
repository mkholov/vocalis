use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use lingua_common::{AssignmentContent, AssignmentId, AssignmentKind, ClientToServer, SessionKey};
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

/// One decoded frame of the live screen-demo video stream, ready to upload as
/// an egui texture. `rgba` is `Arc`-wrapped (rather than a plain `Vec<u8>`) so
/// the UI thread's per-frame check in `StudentApp::update` — clone if the
/// version changed, skip otherwise — never has to memcpy a multi-megabyte
/// buffer just to look at it; cloning an `Arc` is a refcount bump regardless
/// of how big the frame is.
#[derive(Clone)]
pub struct DemoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
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
    /// Each current group peer's own session key, derived from their relayed salt
    /// (`GroupPeer::salt`) plus `pin` — see `net::connect_to_teacher`'s handling of
    /// `JoinGroup`. Keyed by the same address `peer_addrs` uses, so a received
    /// packet's source address looks its sender's key up here directly.
    pub peer_keys: HashMap<SocketAddr, SessionKey>,
    pub uploading_to_teacher: bool,
    pub to_server: Option<mpsc::UnboundedSender<ClientToServer>>,
    /// The PIN this student connected with, kept around (not just used once for
    /// the Hello handshake) because deriving a group peer's key later requires it
    /// again — see `peer_keys`.
    pub pin: String,
    /// This connection's session key, derived from `(pin, salt)` once the teacher
    /// accepts the Hello handshake (see `net::connect_to_teacher`). `None` before
    /// that point and after a disconnect — every encrypted audio receiver treats a
    /// missing key as "nothing legitimate to decrypt yet" and just drops packets.
    pub session_key: Option<SessionKey>,
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
    /// of the class — drives a small "your screen is being shown" notice, and
    /// gates the H.264 upload task started/stopped by `ServerToClient::Start`/
    /// `StopVideoUpload` (see `net::connect_to_teacher`). Entirely independent
    /// of the passive `ClientToServer::ScreenFrame` monitoring upload, which
    /// keeps running unchanged regardless of this flag.
    pub screen_boosted: bool,
    /// Display name of whoever is presenting, while a screen demo (teacher's own
    /// or a relayed student's) is being shown to this student. `None` = no demo.
    pub demo_presenter: Option<String>,
    /// Latest decoded frame of the active demo, and a version counter bumped on
    /// each new frame — same "poll and diff" pattern the teacher's grid uses for
    /// student screen thumbnails, so the GUI only re-uploads the texture when it
    /// changes. Decoding happens off the UI thread, in
    /// `screen::run_screen_demo_receiver`, so a 15fps video stream never blocks
    /// a frame render.
    pub demo_frame: Option<DemoFrame>,
    pub demo_frame_version: u64,
    /// This student's own configured video quality ceiling (see
    /// `settings::VideoQuality`), set once at launch from `Settings::load()`.
    /// Lives here (not just on `StudentApp`) because `screen::run_video_upload`
    /// is started from inside `net::connect_to_teacher`'s message loop, which
    /// only has this shared state to read from, not the GUI struct.
    pub video_quality: crate::settings::VideoQuality,
}

pub type AppState = Arc<Mutex<SharedState>>;
