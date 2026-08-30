use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

/// UDP port the teacher broadcasts `DiscoveryAnnounce` packets on, and students listen on.
pub const DISCOVERY_PORT: u16 = 47990;
/// TCP port the teacher listens on for student control connections.
pub const CONTROL_PORT: u16 = 47991;
/// UDP port each student listens on for the teacher's microphone broadcast.
pub const MIC_PORT: u16 = 47992;
/// UDP port each student listens/sends on for group (peer-to-peer) audio.
pub const PEER_PORT: u16 = 47993;
/// UDP port the teacher listens on when listening in on a single student's mic.
pub const TEACHER_LISTEN_PORT: u16 = 47994;
/// UDP port each student listens on for the teacher's private intercom audio —
/// distinct from `MIC_PORT` because the two can be live at once (a class-wide
/// broadcast in progress plus a private word with one student) and each carries
/// its own independent Opus stream, which needs its own port rather than a
/// discriminator byte grafted onto the shared packet format.
pub const TEACHER_INTERCOM_PORT: u16 = 47995;

pub const DISCOVERY_MAGIC: &[u8; 8] = b"LINGUA1\0";

pub type StudentId = Uuid;
pub type AssignmentId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryAnnounce {
    pub teacher_name: String,
    pub control_port: u16,
}

/// One other member of a conversation group, as seen from a given student's side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPeer {
    pub addr: SocketAddr,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentKind {
    Listening,
    Test,
    Dialogue,
    Pronunciation,
}

impl AssignmentKind {
    pub fn label(self) -> &'static str {
        match self {
            AssignmentKind::Listening => "Аудирование",
            AssignmentKind::Test => "Тест",
            AssignmentKind::Dialogue => "Диалог",
            AssignmentKind::Pronunciation => "Произношение",
        }
    }
}

/// One multiple-choice question in a `Test` assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestQuestion {
    pub text: String,
    pub options: Vec<String>,
    /// Index into `options`. Sent to the student along with the question —
    /// grading happens client-side (see `AssignmentContent::Test`'s doc) — this
    /// app has no encryption or anti-cheat pretensions anywhere else either, so
    /// there's no separate "answer stays server-side" flow to build here.
    pub correct_index: usize,
}

/// The actual content of an assignment, as opposed to just its `title`/`kind`
/// label. `None` (not this type at all) on `AssignmentOffer` means a plain
/// legacy/label-only assignment (currently just `Dialogue` quick-sends) —
/// completed the old way, via a bare "mark done" click.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssignmentContent {
    /// Auto-graded once every question has an answer: the client computes
    /// correct/total locally and reports it via `ClientToServer::TestResult`,
    /// which also counts as completion — no separate "mark done" for a test.
    Test { questions: Vec<TestQuestion> },
    /// An existing library material plus prompts to think about while/after
    /// listening. Not auto-graded — completed via `AssignmentDone` like Reading.
    Listening {
        material_title: String,
        questions: Vec<String>,
    },
    /// A text to read aloud/silently. Not auto-graded.
    Reading { text: String },
}

/// Messages sent from a student client to the teacher console over the TCP control channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientToServer {
    /// `pin` must match the teacher's current lesson PIN or the connection is
    /// rejected — see [`ServerToClient::Rejected`].
    Hello { name: String, pin: String },
    /// A downsized JPEG snapshot of the student's screen, sent periodically for monitoring.
    ScreenFrame { jpeg: Vec<u8> },
    ChatMessage { text: String },
    /// Current mic input level (RMS, fixed-point *1000), sent a few times a second so
    /// the teacher's grid can show who's actually talking right now.
    AudioLevel { millis: i32 },
    /// The student raised (or lowered) their hand to ask for help.
    RequestHelp { needed: bool },
    /// The student marked a received assignment as done.
    AssignmentDone { id: AssignmentId },
    /// A whole file pushed to the teacher (e.g. a self-recorded pronunciation clip),
    /// saved as-is on the teacher's machine — the reverse direction of
    /// [`ServerToClient::FileOffer`], over the same control channel.
    FileOffer { name: String, data: Vec<u8> },
    /// Result of an auto-graded `AssignmentContent::Test`, sent once every
    /// question has been answered. Also counts as completing the assignment.
    TestResult {
        id: AssignmentId,
        correct: u32,
        total: u32,
    },
    /// Sent once each time the student's window loses OS focus while in test
    /// mode (`LockScreen.test_mode`) — i.e. they switched to another app/window.
    FocusLost,
}

/// Messages sent from the teacher console to a student client over the TCP control channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerToClient {
    Welcome {
        student_id: StudentId,
        teacher_name: String,
    },
    /// Sent instead of `Welcome` when `Hello.pin` didn't match the lesson PIN; the
    /// control connection is closed by the teacher right after.
    Rejected {
        reason: String,
    },
    /// Join a conversation group with the given peers: mic audio should be sent to,
    /// and mixed in from, every one of them.
    JoinGroup {
        peers: Vec<GroupPeer>,
    },
    LeaveGroup,
    LockScreen {
        message: String,
        /// Test mode adds two things on top of the plain lock overlay: the
        /// student's client fights to keep OS focus (re-requesting it, re-
        /// asserting fullscreen/always-on-top) whenever it detects focus was
        /// lost, and reports each loss to the teacher via `FocusLost` — "honest
        /// monitoring", not a hard block (a regular desktop app can't actually
        /// prevent Alt+Tab/task-switching without admin-level hooks).
        test_mode: bool,
    },
    UnlockScreen,
    /// Start streaming mic audio to the teacher for real-time listen-in.
    StartMicUpload,
    StopMicUpload,
    /// The teacher opened a private two-way intercom with this student — audio
    /// will start arriving on `TEACHER_INTERCOM_PORT`. Purely a UI signal (to show
    /// a "teacher is talking to you personally" indicator, distinct from the
    /// class-wide broadcast); the receive socket is always bound regardless.
    StartIntercom,
    StopIntercom,
    /// The teacher started playing an audio material to this student — pairs with
    /// the "model pronunciation" feature: the student's UI can prompt "repeat
    /// after the speaker" and starts caching the incoming broadcast locally as a
    /// playable reference (best-effort — nothing breaks if it's missed).
    MaterialPlaying {
        title: String,
    },
    /// That material stopped (finished or was stopped by the teacher) — the
    /// student finalizes whatever reference audio it managed to cache.
    MaterialStopped,
    /// A full-class screen demonstration started — either the teacher's own
    /// screen, or (relayed through the teacher) another student's. `presenter` is
    /// a display name for the "Демонстрация экрана: <кто>" indicator; frames
    /// follow as `ScreenDemoFrame`.
    StartScreenDemo {
        presenter: String,
    },
    StopScreenDemo,
    /// One frame of an active screen demo (see `StartScreenDemo`).
    ScreenDemoFrame {
        jpeg: Vec<u8>,
    },
    /// Sent only to the student whose screen is being demoed to the class: raises
    /// (or restores) their own `ScreenFrame` capture to demo-grade quality/rate —
    /// the passive monitoring cadence is deliberately too light for the rest of
    /// the class to actually read along.
    SetScreenCaptureBoost(bool),
    /// Master mic switch: while locked, the student mustn't transmit mic audio to
    /// anyone (group peers or the teacher), e.g. to keep a test quiet.
    SetMicLocked(bool),
    ChatMessage {
        from: String,
        text: String,
    },
    /// A whole file pushed from the teacher (e.g. a worksheet), saved as-is by the student.
    FileOffer {
        name: String,
        data: Vec<u8>,
    },
    AssignmentOffer {
        id: AssignmentId,
        title: String,
        kind: AssignmentKind,
        content: Option<AssignmentContent>,
    },
}
