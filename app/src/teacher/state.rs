use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lingua_common::{AssignmentId, AssignmentKind, ServerToClient, StudentId};
use rusqlite::Connection;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::db;

/// Generates a random 6-digit lesson PIN (`"000000"`-`"999999"`), used unless the
/// teacher overrides it via `VOCALIS_LESSON_PIN` or edits it in the UI.
pub fn generate_pin() -> String {
    let n = (Uuid::new_v4().as_u128() % 1_000_000) as u32;
    format!("{n:06}")
}

pub struct ChatEntry {
    pub from: String,
    pub text: String,
}

pub struct AssignmentInstance {
    pub id: AssignmentId,
    pub title: String,
    pub kind: AssignmentKind,
    pub done: bool,
    /// Row id in the `assignments` table, if the DB write succeeded — `None` just
    /// means this particular assignment won't be persisted, not a hard failure.
    pub db_id: Option<i64>,
}

/// A student's live status, in the priority order the grid should show it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Empty,
    NeedsHelp,
    Speaking,
    Connected,
}

/// How recent a mic-level reading must be to still count as "currently speaking".
const SPEAKING_TIMEOUT: Duration = Duration::from_millis(900);
/// RMS level (fixed-point *1000) above which we consider a student to be talking.
const SPEAKING_THRESHOLD: i32 = 120;

/// Everything the GUI needs to know about one connected student.
pub struct Student {
    pub name: String,
    pub ip: IpAddr,
    pub seat: usize,
    /// Sends control messages to this student's TCP writer task.
    pub to_client: mpsc::UnboundedSender<ServerToClient>,
    /// Latest screen snapshot, JPEG-encoded, as received over the control channel.
    pub last_frame_jpeg: Option<Vec<u8>>,
    /// Bumped every time `last_frame_jpeg` changes, so the GUI knows to re-upload the texture.
    pub frame_version: u64,
    pub locked: bool,
    /// Whether the current lock (if `locked`) is test mode — drives the focus-loss
    /// monitoring UI on the teacher's side; meaningless while `locked` is false.
    pub test_mode: bool,
    /// How many times this student's client reported losing OS focus during the
    /// current test — reset to 0 each time a new test-mode lock starts.
    pub test_violations: u32,
    pub group: Option<usize>,
    pub needs_help: bool,
    pub last_level: i32,
    pub last_level_at: Option<Instant>,
    pub assignments: Vec<AssignmentInstance>,
    pub score: Option<u32>,
    /// Row id in the `students` table, if the DB write succeeded when they connected.
    pub db_id: Option<i64>,
}

impl Student {
    pub fn presence(&self) -> Presence {
        if self.needs_help {
            Presence::NeedsHelp
        } else if self.last_level >= SPEAKING_THRESHOLD
            && self
                .last_level_at
                .is_some_and(|t| t.elapsed() < SPEAKING_TIMEOUT)
        {
            Presence::Speaking
        } else {
            Presence::Connected
        }
    }

    pub fn assignments_done(&self) -> usize {
        self.assignments.iter().filter(|a| a.done).count()
    }
}

pub struct SharedState {
    pub students: HashMap<StudentId, Student>,
    pub mic_broadcasting: bool,
    /// Conversation groups, keyed by an ever-increasing id (never reused, so a stale
    /// `Student::group` index can never silently point at the wrong group).
    pub groups: HashMap<usize, Vec<StudentId>>,
    pub next_group_id: usize,
    pub listening_to: Option<StudentId>,
    /// The student currently in a private two-way intercom with the teacher, if
    /// any. Always implies `listening_to == talking_to` — you can listen to a
    /// student without talking to them, but not the other way around.
    pub talking_to: Option<StudentId>,
    pub chat_log: Vec<ChatEntry>,
    pub class_name: String,
    pub class_size: usize,
    /// PIN a student's `Hello.pin` must match to be admitted; shown on the teacher's
    /// screen and read out to the class. Editable at runtime from the top bar.
    pub lesson_pin: String,
    pub mics_locked: bool,
    /// Seats that have had at least one student connect this session, for attendance.
    pub ever_connected_seats: HashSet<usize>,
    next_seat: usize,
    pub lesson_started_at: Instant,
    /// Local SQLite connection backing lesson/student/grade/assignment history —
    /// guarded by the same mutex as everything else here, so no separate lock needed.
    pub db: Connection,
    /// Row id of the `lessons` entry created for this run of the teacher console.
    pub lesson_row_id: i64,
    /// Snapshot of history from before this lesson, loaded once at startup.
    pub history: db::HistorySummary,
    /// The audio materials library, loaded once at startup and appended to as the
    /// teacher uploads more — the file itself stays on disk wherever it was picked
    /// from, this is just title + path.
    pub materials: Vec<db::MaterialRow>,
    /// The material currently being broadcast, if any (progress for the "now
    /// playing" bar in the Materials tab).
    pub playing: Option<PlayingMaterial>,
    /// An active full-class screen demonstration (teacher's own screen, or a
    /// relayed student's), if any. Mutually exclusive with itself — starting one
    /// always stops whichever was running before.
    pub screen_demo: Option<ScreenDemo>,
}

/// Live playback progress for the Materials tab, updated a few times a second by
/// the playback task itself.
pub struct PlayingMaterial {
    pub material_id: i64,
    pub title: String,
    pub total_ms: u64,
    pub elapsed_ms: u64,
    /// Who this play-out was actually sent to — needed so both the "Stop" button
    /// and the natural end of the clip can tell the same set of students it ended
    /// (`ServerToClient::MaterialStopped`), regardless of the selection changing
    /// in the meantime.
    pub targets: Vec<StudentId>,
}

/// Whose screen a demo is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenDemoSource {
    Teacher,
    Student(StudentId),
}

/// An active full-class screen demonstration.
pub struct ScreenDemo {
    pub source: ScreenDemoSource,
    pub presenter_name: String,
    /// The audience — everyone it was announced to (`StartScreenDemo`), so the
    /// same set can be told it ended regardless of the selection changing later.
    /// For a student demo this is deliberately "everyone but the presenter", not
    /// `action_targets()` — showing someone's screen to themselves makes no sense.
    pub targets: Vec<StudentId>,
}

pub type AppState = Arc<std::sync::Mutex<SharedState>>;

impl SharedState {
    pub fn new(
        class_name: String,
        class_size: usize,
        lesson_pin: String,
        db: Connection,
        lesson_row_id: i64,
        history: db::HistorySummary,
        materials: Vec<db::MaterialRow>,
    ) -> Self {
        Self {
            students: HashMap::new(),
            mic_broadcasting: false,
            groups: HashMap::new(),
            next_group_id: 0,
            listening_to: None,
            talking_to: None,
            chat_log: Vec::new(),
            class_name,
            class_size,
            lesson_pin,
            mics_locked: false,
            ever_connected_seats: HashSet::new(),
            db,
            lesson_row_id,
            history,
            materials,
            playing: None,
            screen_demo: None,
            next_seat: 1,
            lesson_started_at: Instant::now(),
        }
    }

    pub fn assign_seat(&mut self) -> usize {
        let seat = self.next_seat;
        self.next_seat += 1;
        self.ever_connected_seats.insert(seat);
        seat
    }

    pub fn student_addrs(&self) -> Vec<SocketAddr> {
        self.students
            .values()
            .map(|s| SocketAddr::new(s.ip, lingua_common::MIC_PORT))
            .collect()
    }

    /// Puts `members` into a brand new group, notifying each member of the others.
    pub fn create_group(&mut self, members: &[StudentId]) {
        if members.len() < 2 {
            return;
        }
        // Leave any group a selected student was already in.
        for &id in members {
            self.leave_group(id);
        }
        let group_id = self.next_group_id;
        self.next_group_id += 1;
        self.groups.insert(group_id, members.to_vec());
        for &id in members {
            if let Some(s) = self.students.get_mut(&id) {
                s.group = Some(group_id);
            }
        }
        self.notify_group(group_id);
    }

    /// Removes `id` from whatever group it's in, if any, and tells every remaining
    /// member (and `id` itself) about the change.
    pub fn leave_group(&mut self, id: StudentId) {
        let Some(group_id) = self.students.get(&id).and_then(|s| s.group) else {
            return;
        };
        if let Some(members) = self.groups.get_mut(&group_id) {
            members.retain(|&m| m != id);
            let remaining = members.clone();
            if remaining.len() < 2 {
                for &m in &remaining {
                    if let Some(s) = self.students.get_mut(&m) {
                        s.group = None;
                    }
                    if let Some(s) = self.students.get(&m) {
                        let _ = s.to_client.send(ServerToClient::LeaveGroup);
                    }
                }
                self.groups.remove(&group_id);
            } else {
                self.notify_group(group_id);
            }
        }
        if let Some(s) = self.students.get_mut(&id) {
            s.group = None;
        }
        if let Some(s) = self.students.get(&id) {
            let _ = s.to_client.send(ServerToClient::LeaveGroup);
        }
    }

    /// Moves `a` into whatever group `b` is currently in (creating a fresh pair if `b`
    /// is ungrouped). Used by the drag-and-drop grouping UI.
    pub fn group_with(&mut self, a: StudentId, b: StudentId) {
        if a == b {
            return;
        }
        self.leave_group(a);
        if let Some(group_id) = self.students.get(&b).and_then(|s| s.group) {
            if let Some(members) = self.groups.get_mut(&group_id) {
                if !members.contains(&a) {
                    members.push(a);
                }
            }
            if let Some(s) = self.students.get_mut(&a) {
                s.group = Some(group_id);
            }
            self.notify_group(group_id);
        } else {
            self.create_group(&[a, b]);
        }
    }

    pub fn leave_all_groups(&mut self) {
        let ids: Vec<StudentId> = self.students.keys().copied().collect();
        for id in ids {
            self.leave_group(id);
        }
    }

    fn notify_group(&self, group_id: usize) {
        let Some(members) = self.groups.get(&group_id) else {
            return;
        };
        for &id in members {
            let Some(student) = self.students.get(&id) else {
                continue;
            };
            let peers = members
                .iter()
                .filter(|&&other| other != id)
                .filter_map(|other| self.students.get(other))
                .map(|s| lingua_common::GroupPeer {
                    addr: SocketAddr::new(s.ip, lingua_common::PEER_PORT),
                    name: s.name.clone(),
                })
                .collect();
            let _ = student
                .to_client
                .send(ServerToClient::JoinGroup { peers });
        }
    }

    /// Switches `listening_to` to `id`, telling whichever student was previously
    /// being listened to (if any, and if different) to stop uploading their mic,
    /// and this one to start. No-op if already listening to `id`. Shared by the
    /// plain "listen in" toggle and by starting an intercom (which always implies
    /// listening to the same student).
    pub fn start_listening(&mut self, id: StudentId) {
        if self.listening_to == Some(id) {
            return;
        }
        if let Some(prev) = self.listening_to.take() {
            if let Some(s) = self.students.get(&prev) {
                let _ = s.to_client.send(ServerToClient::StopMicUpload);
            }
        }
        if let Some(s) = self.students.get(&id) {
            let _ = s.to_client.send(ServerToClient::StartMicUpload);
            self.listening_to = Some(id);
        }
    }

    pub fn set_mics_locked(&mut self, locked: bool) {
        self.mics_locked = locked;
        for s in self.students.values() {
            let _ = s.to_client.send(ServerToClient::SetMicLocked(locked));
        }
    }

    /// Average of every manually-entered score (0-100), if any have been entered.
    pub fn average_score(&self) -> Option<f32> {
        let scores: Vec<u32> = self.students.values().filter_map(|s| s.score).collect();
        if scores.is_empty() {
            return None;
        }
        Some(scores.iter().sum::<u32>() as f32 / scores.len() as f32)
    }

    pub fn assignments_completed_total(&self) -> usize {
        self.students.values().map(|s| s.assignments_done()).sum()
    }

    pub fn connected_count(&self) -> usize {
        self.students.len()
    }
}
