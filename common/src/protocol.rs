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
    },
}
