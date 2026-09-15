//! Real network connection to a teacher's session, for the live screen-demo
//! video receiver (step 7 part B — `vocalis_roadmap.md`, section 8). Reuses
//! `student::net::connect_to_teacher` and `student::screen::run_screen_demo_receiver`
//! unchanged — the exact same handshake/session-key derivation and H.264
//! receive/decode path the egui student app uses; nothing about the network
//! protocol is reimplemented here. What's new is only the "poll the decoded
//! frame for changes, JPEG-encode it, emit it to the webview" glue, reusing
//! `screen_frame`'s helper — the same one `screen_demo.rs`'s self-preview
//! uses — so both paths emit the identical `screen-demo-frame` event shape
//! and the frontend needs no changes to tell them apart.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vocalis::student::{net, screen, state};

use super::screen_frame::jpeg_data_url;

/// How often to check `demo_frame_version` for a new decoded frame.
/// Comfortably under the 66.7ms/15fps frame budget `video-bench` measured,
/// so this adds negligible latency on top of decode+JPEG themselves.
const DEMO_POLL_INTERVAL_MS: u64 = 20;
/// `student::net::connect_to_teacher` has no readiness callback of its own
/// (unlike `teacher::mic::start_mic_capture`, which `student_mic.rs` gets a
/// synchronous ready signal from) — it just sets `connected_teacher` once the
/// Hello/Welcome handshake succeeds, then runs its receive loop forever. This
/// bounds how long `connect_student_session` polls for that before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct StudentSession {
    // Kept alive only for its `Arc` refcount — the spawned tasks each hold
    // their own clone and are the only things that ever read from it. Not
    // read directly through this field, but dropping it early would be a
    // real bug if a future change removed one of those clones, so it stays.
    #[allow(dead_code)]
    app_state: state::AppState,
    teacher_name: String,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for StudentSession {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Default)]
pub struct StudentSessionState(pub Mutex<Option<StudentSession>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StudentSessionInfo {
    pub teacher_name: String,
}

/// Connects to a real teacher session over the network — same Hello/Welcome
/// handshake, same session-key derivation, as the egui student app — then
/// starts the always-on decoded-frame receiver (`run_screen_demo_receiver`,
/// idle until the teacher actually starts a demo, exactly like
/// `student::app::StudentApp::new` spawning it unconditionally at startup)
/// plus a poller that JPEG-encodes each new decoded frame and emits it as
/// `screen-demo-frame` — the identical event `screen_demo.rs`'s self-preview
/// already emits, so `StudentConsole.tsx` needs no changes to render it.
#[tauri::command]
pub fn connect_student_session<R: tauri::Runtime>(
    app: AppHandle<R>,
    session: State<StudentSessionState>,
    teacher_ip: String,
    control_port: u16,
    student_name: String,
    pin: String,
) -> Result<StudentSessionInfo, String> {
    let mut guard = session.0.lock().unwrap();
    if let Some(existing) = guard.as_ref() {
        return Ok(StudentSessionInfo { teacher_name: existing.teacher_name.clone() });
    }

    let ip: IpAddr = teacher_ip.parse().map_err(|_| format!("invalid teacher IP: {teacher_ip}"))?;
    let addr = SocketAddr::new(ip, control_port);
    let app_state: state::AppState = Arc::new(Mutex::new(state::SharedState::default()));

    let connect_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let connect_task = {
        let app_state = app_state.clone();
        let connect_error = connect_error.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = net::connect_to_teacher(app_state, addr, student_name, pin).await {
                *connect_error.lock().unwrap() = Some(e.to_string());
            }
        })
    };

    // No readiness channel to wait on (see the module doc comment) — poll
    // the same flag `connect_to_teacher` itself sets right after a
    // successful handshake, same field the egui app's own UI reads to show
    // "connected to <teacher>".
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let teacher_name = loop {
        if let Some(name) = app_state.lock().unwrap().connected_teacher.clone() {
            break name;
        }
        if let Some(e) = connect_error.lock().unwrap().take() {
            return Err(e);
        }
        if Instant::now() > deadline {
            connect_task.abort();
            return Err("не удалось подключиться к преподавателю (таймаут)".to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let mut tasks = vec![connect_task];
    {
        let recv_state = app_state.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = screen::run_screen_demo_receiver(recv_state).await {
                eprintln!("[student_session] screen-demo receiver stopped: {e:#}");
            }
        }));
    }
    {
        let poll_state = app_state.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let mut last_frame_version = 0u64;
            let mut presenting = false;
            let mut interval = tokio::time::interval(Duration::from_millis(DEMO_POLL_INTERVAL_MS));
            loop {
                interval.tick().await;
                let (new_frame, now_presenting) = {
                    let guard = poll_state.lock().unwrap();
                    let now_presenting = guard.demo_presenter.is_some();
                    let new_frame = if guard.demo_frame_version == last_frame_version {
                        None
                    } else {
                        last_frame_version = guard.demo_frame_version;
                        guard.demo_frame.clone()
                    };
                    (new_frame, now_presenting)
                };
                if presenting && !now_presenting {
                    let _ = app.emit("screen-demo-stopped", ());
                }
                presenting = now_presenting;

                let Some(frame) = new_frame else { continue };
                match jpeg_data_url(frame.width, frame.height, &frame.rgba) {
                    Ok(data_url) => {
                        let _ = app.emit(
                            "screen-demo-frame",
                            super::screen_frame::ScreenDemoFrameDto { width: frame.width, height: frame.height, data_url },
                        );
                    }
                    Err(e) => eprintln!("[student_session] JPEG encode failed: {e}"),
                }
            }
        }));
    }

    let info = StudentSessionInfo { teacher_name: teacher_name.clone() };
    *guard = Some(StudentSession { app_state, teacher_name, tasks });
    Ok(info)
}

#[tauri::command]
pub fn disconnect_student_session(session: State<StudentSessionState>) {
    *session.0.lock().unwrap() = None;
}
