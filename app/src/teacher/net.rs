use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use lingua_common::{
    crypto, read_message, read_message_encrypted, write_message, write_message_encrypted,
    ClientToServer, ServerToClient, CONTROL_PORT,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use super::db;
use super::state::{self, AppState, ChatEntry, Student};

pub async fn run_control_server(state: AppState, teacher_name: Arc<str>) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", CONTROL_PORT)).await?;
    info!("teacher control server listening on port {CONTROL_PORT}");
    loop {
        let (socket, peer) = listener.accept().await?;
        let state = state.clone();
        let teacher_name = teacher_name.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_student(socket, peer.ip(), state, teacher_name).await {
                warn!("student connection {peer} ended: {e:#}");
            }
        });
    }
}

async fn handle_student(
    socket: tokio::net::TcpStream,
    ip: std::net::IpAddr,
    state: AppState,
    teacher_name: Arc<str>,
) -> Result<()> {
    let (mut read_half, mut write_half) = socket.into_split();

    let hello: ClientToServer = read_message(&mut read_half).await?;
    let (name, pin) = match hello {
        ClientToServer::Hello { name, pin } => (name, pin),
        _ => anyhow::bail!("expected Hello as first message"),
    };

    let expected_pin = state.lock().unwrap().lesson_pin.clone();
    if pin.trim() != expected_pin {
        write_message(
            &mut write_half,
            &ServerToClient::Rejected {
                reason: "Неверный PIN-код урока".to_string(),
            },
        )
        .await?;
        warn!("student '{name}' from {ip} rejected: wrong PIN");
        {
            let guard = state.lock().unwrap();
            let _ = db::insert_connection_log(&guard.db, guard.lesson_row_id, &name, &ip.to_string(), "rejected_pin");
        }
        return Ok(());
    }

    let student_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerToClient>();

    // Last plaintext message either side sends. From here on every control frame
    // (and every UDP audio packet tied to this student) is encrypted under
    // `session_key`, which both sides can now derive independently from `pin` +
    // this freshly generated salt — see `crypto`'s module doc comment.
    let salt = crypto::generate_salt();
    let session_key = crypto::derive_key(&pin, &salt);

    write_message(
        &mut write_half,
        &ServerToClient::Welcome {
            student_id,
            teacher_name: teacher_name.to_string(),
            salt,
        },
    )
    .await?;

    {
        let mut guard = state.lock().unwrap();
        let seat = guard.assign_seat();
        if guard.mics_locked {
            let _ = tx.send(ServerToClient::SetMicLocked(true));
        }
        let db_id = db::insert_student(&guard.db, guard.lesson_row_id, &name, seat).ok();

        // Roster check is soft record-keeping, never a connection gate (that's
        // what the PIN is for) — an empty roster (nothing configured yet) means
        // there's nothing to flag against, so it's treated as a match too.
        let normalized = db::normalize_name(&name);
        let roster_status = if guard.roster.is_empty()
            || guard.roster.iter().any(|r| db::normalize_name(&r.full_name) == normalized)
        {
            state::RosterStatus::Matched
        } else {
            state::RosterStatus::UnrecognizedPending
        };
        if roster_status == state::RosterStatus::UnrecognizedPending {
            guard.chat_log.push(ChatEntry {
                from: "Система".to_string(),
                text: format!("❓ {name} подключился(-ась), но не найден(а) в списке класса"),
            });
        }

        guard.students.insert(
            student_id,
            Student {
                name: name.clone(),
                ip,
                seat,
                to_client: tx,
                last_frame_jpeg: None,
                frame_version: 0,
                locked: false,
                test_mode: false,
                test_violations: 0,
                group: None,
                needs_help: false,
                last_level: 0,
                last_level_at: None,
                assignments: Vec::new(),
                score: None,
                db_id,
                roster_status,
                salt,
                session_key,
            },
        );
        let _ = db::insert_connection_log(&guard.db, guard.lesson_row_id, &name, &ip.to_string(), "connected");
    }
    info!("student '{name}' connected from {ip}");

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_message_encrypted(&mut write_half, &msg, &session_key).await.is_err() {
                break;
            }
        }
    });

    loop {
        match read_message_encrypted::<ClientToServer, _>(&mut read_half, &session_key).await {
            Ok(ClientToServer::ScreenFrame { jpeg }) => {
                let mut guard = state.lock().unwrap();
                // If this student is the one currently being demoed to the class,
                // relay the same frame on to the audience — there's no student-to-
                // student channel for screenshots (unlike P2P group audio), so the
                // teacher is the only place this can be forwarded from.
                if let Some(demo) = &guard.screen_demo {
                    if demo.source == state::ScreenDemoSource::Student(student_id) {
                        let targets = demo.targets.clone();
                        for id in &targets {
                            if let Some(s) = guard.students.get(id) {
                                let _ = s.to_client.send(ServerToClient::ScreenDemoFrame { jpeg: jpeg.clone() });
                            }
                        }
                    }
                }
                if let Some(student) = guard.students.get_mut(&student_id) {
                    student.last_frame_jpeg = Some(jpeg);
                    student.frame_version += 1;
                }
            }
            Ok(ClientToServer::ChatMessage { text }) => {
                let mut guard = state.lock().unwrap();
                guard.chat_log.push(ChatEntry {
                    from: name.clone(),
                    text,
                });
            }
            Ok(ClientToServer::AudioLevel { millis }) => {
                let mut guard = state.lock().unwrap();
                if let Some(student) = guard.students.get_mut(&student_id) {
                    student.last_level = millis;
                    student.last_level_at = Some(Instant::now());
                }
            }
            Ok(ClientToServer::RequestHelp { needed }) => {
                let mut guard = state.lock().unwrap();
                if let Some(student) = guard.students.get_mut(&student_id) {
                    student.needs_help = needed;
                }
            }
            Ok(ClientToServer::AssignmentDone { id }) => {
                let mut guard = state.lock().unwrap();
                let assignment_db_id = guard.students.get(&student_id).and_then(|student| {
                    student.assignments.iter().find(|a| a.id == id).and_then(|a| a.db_id)
                });
                if let Some(student) = guard.students.get_mut(&student_id) {
                    if let Some(a) = student.assignments.iter_mut().find(|a| a.id == id) {
                        a.done = true;
                    }
                }
                if let Some(assignment_db_id) = assignment_db_id {
                    let _ = db::mark_assignment_done(&guard.db, assignment_db_id);
                }
            }
            Ok(ClientToServer::TestResult { id, correct, total }) => {
                let mut guard = state.lock().unwrap();
                let assignment_db_id = guard.students.get(&student_id).and_then(|student| {
                    student.assignments.iter().find(|a| a.id == id).and_then(|a| a.db_id)
                });
                if let Some(student) = guard.students.get_mut(&student_id) {
                    if let Some(a) = student.assignments.iter_mut().find(|a| a.id == id) {
                        a.done = true;
                        a.test_score = Some((correct, total));
                    }
                }
                if let Some(assignment_db_id) = assignment_db_id {
                    let _ = db::mark_assignment_done(&guard.db, assignment_db_id);
                    let _ = db::insert_test_result(&guard.db, assignment_db_id, correct, total);
                }
            }
            Ok(ClientToServer::FocusLost) => {
                let mut guard = state.lock().unwrap();
                if let Some(student) = guard.students.get_mut(&student_id) {
                    student.test_violations += 1;
                }
                guard.chat_log.push(ChatEntry {
                    from: "Система".to_string(),
                    text: format!("⚠️ {name} переключился(-ась) на другое приложение во время теста"),
                });
            }
            Ok(ClientToServer::FileOffer { name: file_name, data }) => match save_received_file(&file_name, &data) {
                Ok(path) => {
                    let mut guard = state.lock().unwrap();
                    guard.chat_log.push(ChatEntry {
                        from: name.clone(),
                        text: format!("📎 прислал(а) файл: {file_name} (сохранён в {})", path.display()),
                    });
                }
                Err(e) => warn!("failed to save file '{file_name}' from student '{name}': {e:#}"),
            },
            Ok(ClientToServer::Hello { .. }) => {}
            Err(_) => break,
        }
    }

    writer_task.abort();
    {
        let mut guard = state.lock().unwrap();
        guard.leave_group(student_id);
        if guard.listening_to == Some(student_id) {
            guard.listening_to = None;
        }
        if guard.talking_to == Some(student_id) {
            guard.talking_to = None;
        }
        // Nothing left to show if the student being demoed just left.
        if matches!(&guard.screen_demo, Some(demo) if demo.source == state::ScreenDemoSource::Student(student_id)) {
            if let Some(demo) = guard.screen_demo.take() {
                for id in &demo.targets {
                    if let Some(s) = guard.students.get(id) {
                        let _ = s.to_client.send(ServerToClient::StopScreenDemo);
                    }
                }
            }
        }
        guard.students.remove(&student_id);
        let _ = db::insert_connection_log(&guard.db, guard.lesson_row_id, &name, &ip.to_string(), "disconnected");
    }
    info!("student '{name}' disconnected");
    Ok(())
}

/// Saves a file a student pushed to the teacher (e.g. a self-recorded clip sent
/// via "Отправить учителю") — mirrors the student side's own `save_received_file`.
fn save_received_file(name: &str, data: &[u8]) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("VocalisReceivedFromStudents");
    std::fs::create_dir_all(&dir)?;
    let safe_name = name.replace(['/', '\\', ':'], "_");
    let path = dir.join(safe_name);
    std::fs::write(&path, data)?;
    Ok(path)
}
