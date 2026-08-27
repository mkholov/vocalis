use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::Result;
use lingua_common::{read_message, write_message, ClientToServer, ServerToClient};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::state::{AppState, AssignmentEntry, ChatEntry, DiscoveredTeacher, ReceivedFile};

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
        &ClientToServer::Hello { name: student_name, pin },
    )
    .await?;
    let welcome: ServerToClient = read_message(&mut read_half).await?;
    let teacher_name = match welcome {
        ServerToClient::Welcome { teacher_name, .. } => teacher_name,
        ServerToClient::Rejected { reason } => anyhow::bail!(reason),
        _ => anyhow::bail!("expected Welcome as first server message"),
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<ClientToServer>();
    {
        let mut guard = state.lock().unwrap();
        guard.connected_teacher = Some(teacher_name.clone());
        guard.teacher_addr = Some(addr.ip());
        guard.connecting = false;
        guard.to_server = Some(tx.clone());
    }
    info!("connected to teacher '{teacher_name}'");

    let _screen_task = AbortOnDrop(tokio::spawn(super::screen::run_screen_capture(tx)));

    let _writer_task = AbortOnDrop(tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_message(&mut write_half, &msg).await.is_err() {
                break;
            }
        }
    }));

    loop {
        match read_message::<ServerToClient, _>(&mut read_half).await {
            Ok(ServerToClient::Welcome { .. }) => {}
            // Only ever sent as the very first reply, handled above during the
            // handshake — present here only so this match stays exhaustive.
            Ok(ServerToClient::Rejected { .. }) => {}
            Ok(ServerToClient::JoinGroup { peers }) => {
                let mut guard = state.lock().unwrap();
                guard.peer_addrs = peers.iter().map(|p| p.addr).collect();
                guard.peer_names = peers.iter().map(|p| p.name.clone()).collect();
            }
            Ok(ServerToClient::LeaveGroup) => {
                let mut guard = state.lock().unwrap();
                guard.peer_addrs.clear();
                guard.peer_names.clear();
            }
            Ok(ServerToClient::LockScreen { message }) => {
                state.lock().unwrap().locked_message = Some(message);
            }
            Ok(ServerToClient::UnlockScreen) => {
                state.lock().unwrap().locked_message = None;
            }
            Ok(ServerToClient::StartMicUpload) => {
                state.lock().unwrap().uploading_to_teacher = true;
            }
            Ok(ServerToClient::StopMicUpload) => {
                state.lock().unwrap().uploading_to_teacher = false;
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
            Ok(ServerToClient::AssignmentOffer { id, title, kind }) => {
                state.lock().unwrap().assignments.push(AssignmentEntry {
                    id,
                    title,
                    kind,
                    done: false,
                });
            }
            Err(_) => break,
        }
    }

    let mut guard = state.lock().unwrap();
    guard.connected_teacher = None;
    guard.teacher_addr = None;
    guard.to_server = None;
    guard.peer_addrs.clear();
    guard.peer_names.clear();
    guard.uploading_to_teacher = false;
    guard.locked_message = None;
    guard.mic_locked = false;
    guard.needs_help = false;
    guard.assignments.clear();
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
