use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::Result;
use lingua_common::{
    crypto, read_message, read_message_encrypted, write_message, write_message_encrypted,
    ClientToServer, ServerToClient, OPUS_SAMPLE_RATE,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::recording;
use super::state::{ActiveRecording, AppState, AssignmentEntry, ChatEntry, DiscoveredTeacher, ReceivedFile};

const STALE_AFTER: Duration = Duration::from_secs(6);

/// Aborts the wrapped task when dropped. `tokio::spawn` detaches by default — a
/// spawned task keeps running even after whoever spawned it is gone — so without
/// this, cancelling `connect_to_teacher` (e.g. the "Disconnect" button aborting its
/// `JoinHandle`) only ever stopped the read loop itself. The writer task and the
/// screen-capture task kept running in the background, each still holding its half
/// of the split `TcpStream`; since both halves share the same underlying socket, it
/// never actually closed, no FIN was ever sent, and the teacher's blocked read just
/// hung — showing the student as "connected" forever. Holding these guards as locals
/// in `connect_to_teacher` ties their lifetime to the connection itself, so every
/// exit path (clean disconnect, read error, or external abort) tears down all three
/// tasks together and the socket actually closes.
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Listens for teacher announcements and keeps `state.discovered` up to date.
pub async fn run_discovery(state: AppState) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        if let Err(e) = lingua_common::run_discovery_listener(tx).await {
            warn!("discovery listener stopped: {e:#}");
        }
    });

    while let Some((from, announce)) = rx.recv().await {
        let control_addr = SocketAddr::new(from.ip(), announce.control_port);
        let mut guard = state.lock().unwrap();
        guard.discovered.insert(
            control_addr,
            DiscoveredTeacher {
                name: announce.teacher_name,
                last_seen: Instant::now(),
            },
        );
    }
    Ok(())
}

/// Removes teachers we haven't heard an announcement from in a while.
pub async fn run_discovery_pruner(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut guard = state.lock().unwrap();
        guard
            .discovered
            .retain(|_, t| t.last_seen.elapsed() < STALE_AFTER);
    }
}

/// Connects to a teacher's control channel, performs the handshake, and spawns the
/// reader loop that reacts to `ServerToClient` messages (grouping, lock/unlock, chat,
/// files, listen-in). Also starts the screen-capture task for as long as the
/// connection is alive.
pub async fn connect_to_teacher(
    state: AppState,
    addr: SocketAddr,
    student_name: String,
    pin: String,
) -> Result<()> {
    let socket = tokio::net::TcpStream::connect(addr).await?;
    let (mut read_half, mut write_half) = socket.into_split();

    write_message(
        &mut write_half,
        &ClientToServer::Hello { name: student_name, pin: pin.clone() },
    )
    .await?;
    let welcome: ServerToClient = read_message(&mut read_half).await?;
    let (teacher_name, salt) = match welcome {
        ServerToClient::Welcome { teacher_name, salt, .. } => (teacher_name, salt),
        ServerToClient::Rejected { reason } => anyhow::bail!(reason),
        _ => anyhow::bail!("expected Welcome as first server message"),
    };
    // Last plaintext message either side sends — see `crypto`'s module doc
    // comment. Both sides now independently hold the same session key, derived
    // from the PIN we just successfully connected with and the salt the teacher
    // just sent, so everything from here on (this control channel, and every UDP
    // audio port) is encrypted under it.
    let session_key = crypto::derive_key(&pin, &salt);

    let (tx, mut rx) = mpsc::unbounded_channel::<ClientToServer>();
    {
        let mut guard = state.lock().unwrap();
        guard.connected_teacher = Some(teacher_name.clone());
        guard.teacher_addr = Some(addr.ip());
        guard.connecting = false;
        guard.to_server = Some(tx.clone());
        guard.pin = pin;
        guard.session_key = Some(session_key);
    }
    info!("connected to teacher '{teacher_name}'");

    let _screen_task = AbortOnDrop(tokio::spawn(super::screen::run_screen_capture(state.clone(), tx)));

    let _writer_task = AbortOnDrop(tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_message_encrypted(&mut write_half, &msg, &session_key).await.is_err() {
                break;
            }
        }
    }));

    loop {
        match read_message_encrypted::<ServerToClient, _>(&mut read_half, &session_key).await {
            Ok(ServerToClient::Welcome { .. }) => {}
            // Only ever sent as the very first reply, handled above during the
            // handshake — present here only so this match stays exhaustive.
            Ok(ServerToClient::Rejected { .. }) => {}
            Ok(ServerToClient::JoinGroup { peers }) => {
                let mut guard = state.lock().unwrap();
                // Peer-to-peer audio never goes through the teacher, so there's no
                // shared per-pair key the way there is for teacher<->student
                // traffic — each peer's key is instead derived right here from
                // their relayed salt plus the same class PIN we ourselves typed in
                // to connect, and used to decrypt whatever arrives from them.
                let pin = guard.pin.clone();
                guard.peer_keys = peers.iter().map(|p| (p.addr, crypto::derive_key(&pin, &p.salt))).collect();
                guard.peer_addrs = peers.iter().map(|p| p.addr).collect();
                guard.peer_names = peers.iter().map(|p| p.name.clone()).collect();
            }
            Ok(ServerToClient::LeaveGroup) => {
                let mut guard = state.lock().unwrap();
                guard.peer_addrs.clear();
                guard.peer_names.clear();
                guard.peer_keys.clear();
            }
            Ok(ServerToClient::LockScreen { message, test_mode }) => {
                let mut guard = state.lock().unwrap();
                guard.locked_message = Some(message);
                guard.test_mode_active = test_mode;
            }
            Ok(ServerToClient::UnlockScreen) => {
                let mut guard = state.lock().unwrap();
                guard.locked_message = None;
                guard.test_mode_active = false;
            }
            Ok(ServerToClient::StartMicUpload) => {
                state.lock().unwrap().uploading_to_teacher = true;
            }
            Ok(ServerToClient::StopMicUpload) => {
                state.lock().unwrap().uploading_to_teacher = false;
            }
            Ok(ServerToClient::StartIntercom) => {
                state.lock().unwrap().intercom_active = true;
            }
            Ok(ServerToClient::StopIntercom) => {
                state.lock().unwrap().intercom_active = false;
            }
            Ok(ServerToClient::MaterialPlaying { title }) => {
                let mut guard = state.lock().unwrap();
                guard.material_title = Some(title);
                guard.material_playing = true;
                guard.reference_capture = Some(ActiveRecording {
                    samples: Vec::new(),
                    sample_rate: OPUS_SAMPLE_RATE,
                });
            }
            Ok(ServerToClient::MaterialStopped) => {
                let active = {
                    let mut guard = state.lock().unwrap();
                    guard.material_playing = false;
                    guard.reference_capture.take()
                };
                if let Some(active) = active {
                    match recording::save_reference(&active.samples, active.sample_rate) {
                        Ok(entry) => state.lock().unwrap().reference = Some(entry),
                        Err(e) => warn!("failed to save reference recording: {e:#}"),
                    }
                }
            }
            Ok(ServerToClient::StartScreenDemo { presenter }) => {
                let mut guard = state.lock().unwrap();
                guard.demo_presenter = Some(presenter);
                guard.last_demo_frame_jpeg = None;
                guard.demo_frame_version = 0;
            }
            Ok(ServerToClient::StopScreenDemo) => {
                let mut guard = state.lock().unwrap();
                guard.demo_presenter = None;
                guard.last_demo_frame_jpeg = None;
            }
            Ok(ServerToClient::ScreenDemoFrame { jpeg }) => {
                let mut guard = state.lock().unwrap();
                guard.last_demo_frame_jpeg = Some(jpeg);
                guard.demo_frame_version += 1;
            }
            Ok(ServerToClient::SetScreenCaptureBoost(boosted)) => {
                state.lock().unwrap().screen_boosted = boosted;
            }
            Ok(ServerToClient::ChatMessage { from, text }) => {
                state.lock().unwrap().chat_log.push(ChatEntry { from, text });
            }
            Ok(ServerToClient::FileOffer { name, data }) => match save_received_file(&name, &data) {
                Ok(path) => state
                    .lock()
                    .unwrap()
                    .received_files
                    .push(ReceivedFile { name, path }),
                Err(e) => warn!("failed to save received file '{name}': {e:#}"),
            },
            Ok(ServerToClient::SetMicLocked(locked)) => {
                state.lock().unwrap().mic_locked = locked;
            }
            Ok(ServerToClient::AssignmentOffer { id, title, kind, content }) => {
                state.lock().unwrap().assignments.push(AssignmentEntry {
                    id,
                    title,
                    kind,
                    done: false,
                    content,
                    last_score: None,
                });
            }
            Err(_) => break,
        }
    }

    let mut guard = state.lock().unwrap();
    guard.connected_teacher = None;
    guard.teacher_addr = None;
    guard.to_server = None;
    guard.session_key = None;
    guard.pin.clear();
    guard.peer_addrs.clear();
    guard.peer_names.clear();
    guard.peer_keys.clear();
    guard.uploading_to_teacher = false;
    guard.locked_message = None;
    guard.test_mode_active = false;
    guard.mic_locked = false;
    guard.needs_help = false;
    guard.assignments.clear();
    guard.intercom_active = false;
    guard.material_title = None;
    guard.material_playing = false;
    guard.reference_capture = None;
    guard.reference = None;
    guard.screen_boosted = false;
    guard.demo_presenter = None;
    guard.last_demo_frame_jpeg = None;
    info!("disconnected from teacher '{teacher_name}'");
    Ok(())
}

fn save_received_file(name: &str, data: &[u8]) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("VocalisReceived");
    std::fs::create_dir_all(&dir)?;
    let safe_name = name.replace(['/', '\\', ':'], "_");
    let path = dir.join(safe_name);
    std::fs::write(&path, data)?;
    Ok(path)
}
