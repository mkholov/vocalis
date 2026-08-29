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
}

pub type AppState = Arc<Mutex<SharedState>>;
