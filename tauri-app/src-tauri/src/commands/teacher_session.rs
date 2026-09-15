//! Live per-student mic levels for the teacher's class grid (step 7, part A —
//! `vocalis_roadmap.md`, section 8). Reuses the real network/session stack
//! unchanged: `teacher::state::SharedState`, `teacher::net::run_control_server`,
//! `lingua_common::run_teacher_announcer` are exactly what the egui teacher
//! console uses to accept connections and update `Student::last_level` from
//! each student's `ClientToServer::AudioLevel` telemetry (see
//! `student::audio::run_level_telemetry`) — nothing about that pipeline is
//! reimplemented here, this module only starts it and periodically reads
//! `SharedState.students` out into a Tauri event.
//!
//! Because the step-3 login screens are still mocked, nothing in the real UI
//! flow ever calls `start_teacher_session` today — `TeacherClassGrid.tsx`
//! calls it itself on mount so its grid has a real, connectable session to
//! show real data for, alongside (not replacing) the local mock simulation
//! it already had. See the step-7 report for how this was exercised with a
//! second, real local process actually connecting and reporting real levels.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vocalis::teacher::{db, net, state};

const LEVEL_EMIT_INTERVAL_MS: u64 = 150;
// A fresh `SharedState`'s `class_size` doesn't matter for this step — nothing
// here renders a seat grid off this state, only mic levels — so a generous
// fixed size just leaves room for any number of test connections.
const TEST_CLASS_SIZE: usize = 30;

pub struct TeacherSession {
    // Kept alive only for its `Arc` refcount — the spawned tasks each hold
    // their own clone and are the only things that ever read from it. Not
    // read directly through this field, but dropping it early would be a
    // real bug if a future change removed one of those clones, so it stays.
    #[allow(dead_code)]
    app_state: state::AppState,
    pin: String,
    class_name: String,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for TeacherSession {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Default)]
pub struct TeacherSessionState(pub Mutex<Option<TeacherSession>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeacherSessionInfo {
    pub pin: String,
    pub control_port: u16,
    pub class_name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StudentLevelDto {
    pub id: String,
    pub name: String,
    /// RMS level, fixed-point *1000 — same convention as the egui app's
    /// `Student::last_level` / `theme::wave_meter`'s `level_millis` param.
    pub level: i32,
    /// Seconds since this student's last level report — lets the frontend
    /// treat a stale reading as silence the same way `Student::presence()`
    /// does with `last_level_at`, without needing its own clock synced to
    /// the backend's.
    pub seconds_since_report: f32,
}

fn emit_levels<R: tauri::Runtime>(app: &AppHandle<R>, app_state: &state::AppState) {
    let guard = app_state.lock().unwrap();
    let levels: Vec<StudentLevelDto> = guard
        .students
        .iter()
        .map(|(id, s)| StudentLevelDto {
            id: id.to_string(),
            name: s.name.clone(),
            level: s.last_level,
            seconds_since_report: s.last_level_at.map(|at| at.elapsed().as_secs_f32()).unwrap_or(f32::MAX),
        })
        .collect();
    drop(guard);
    let _ = app.emit("student-levels", levels);
}

#[tauri::command]
pub fn start_teacher_session<R: tauri::Runtime>(
    app: AppHandle<R>,
    session: State<TeacherSessionState>,
    class_name: String,
) -> Result<TeacherSessionInfo, String> {
    let mut guard = session.0.lock().unwrap();
    if let Some(existing) = guard.as_ref() {
        return Ok(TeacherSessionInfo {
            pin: existing.pin.clone(),
            control_port: lingua_common::CONTROL_PORT,
            class_name: existing.class_name.clone(),
        });
    }

    let conn = db::open().map_err(|e| e.to_string())?;
    let class_id = db::insert_class(&conn, &class_name).map_err(|e| e.to_string())?;
    let lesson_row_id = db::insert_lesson(&conn, class_id, &class_name).map_err(|e| e.to_string())?;
    let history = db::load_history_summary(&conn, class_id).unwrap_or_default();
    let pin = state::generate_pin();

    let app_state: state::AppState = Arc::new(Mutex::new(state::SharedState::new(
        class_id,
        class_name.clone(),
        TEST_CLASS_SIZE,
        pin.clone(),
        conn,
        lesson_row_id,
        history,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )));

    let mut tasks = Vec::new();
    let teacher_name: Arc<str> = Arc::from("Tauri (тест)");

    {
        let name = teacher_name.to_string();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = lingua_common::run_teacher_announcer(name, lingua_common::CONTROL_PORT).await {
                eprintln!("[teacher_session] discovery announcer stopped: {e:#}");
            }
        }));
    }
    {
        let app_state = app_state.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = net::run_control_server(app_state, teacher_name).await {
                eprintln!("[teacher_session] control server stopped: {e:#}");
            }
        }));
    }
    {
        let app = app.clone();
        let app_state = app_state.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(LEVEL_EMIT_INTERVAL_MS));
            loop {
                interval.tick().await;
                emit_levels(&app, &app_state);
            }
        }));
    }

    let info = TeacherSessionInfo { pin: pin.clone(), control_port: lingua_common::CONTROL_PORT, class_name: class_name.clone() };
    *guard = Some(TeacherSession { app_state, pin, class_name, tasks });
    Ok(info)
}

#[tauri::command]
pub fn stop_teacher_session(session: State<TeacherSessionState>) {
    *session.0.lock().unwrap() = None;
}
