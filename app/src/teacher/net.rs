use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use lingua_common::{read_message, write_message, ClientToServer, ServerToClient, CONTROL_PORT};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use super::state::{AppState, ChatEntry, Student};

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
        return Ok(());
    }

    let student_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerToClient>();

    write_message(
        &mut write_half,
        &ServerToClient::Welcome {
            student_id,
            teacher_name: teacher_name.to_string(),
        },
    )
    .await?;

    {
        let mut guard = state.lock().unwrap();
        let seat = guard.assign_seat();
        if guard.mics_locked {
            let _ = tx.send(ServerToClient::SetMicLocked(true));
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
                group: None,
                needs_help: false,
                last_level: 0,
                last_level_at: None,
                assignments: Vec::new(),
                score: None,
            },
        );
    }
    info!("student '{name}' connected from {ip}");

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_message(&mut write_half, &msg).await.is_err() {
                break;
            }
        }
    });

    loop {
        match read_message::<ClientToServer, _>(&mut read_half).await {
            Ok(ClientToServer::ScreenFrame { jpeg }) => {
                let mut guard = state.lock().unwrap();
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
                if let Some(student) = guard.students.get_mut(&student_id) {
                    if let Some(a) = student.assignments.iter_mut().find(|a| a.id == id) {
                        a.done = true;
                    }
                }
            }
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
        guard.students.remove(&student_id);
    }
    info!("student '{name}' disconnected");
    Ok(())
}
