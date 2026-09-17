//! The teacher's live session (`vocalis_roadmap.md`, section 8, step 7 and
//! 7.5) — everything that needs a real, currently-running
//! `teacher::state::SharedState` to act on. `start_teacher_session`/
//! `stop_teacher_session` (part A) start the actual network/session stack
//! unchanged: `teacher::net::run_control_server`, `lingua_common::
//! run_teacher_announcer` are exactly what the egui teacher console uses to
//! accept connections and update `Student::last_level` from each student's
//! `ClientToServer::AudioLevel` telemetry — nothing about that pipeline is
//! reimplemented here, this only starts it and periodically reads
//! `SharedState.students` out into a Tauri event. `start_own_screen_demo`/
//! `stop_own_screen_demo` (part B) and `start_mic_broadcast`/
//! `stop_mic_broadcast` (step 7.5) act on that same running session to
//! reuse, respectively, `teacher::screen::run_own_screen_demo` and
//! `teacher::mic::run_mic_broadcast` — again unchanged, same pipelines the
//! egui console's own toggles use.
//!
//! Because the step-3 login screens are still mocked, nothing in the real UI
//! flow ever calls `start_teacher_session` today — `TeacherClassGrid.tsx`
//! calls it itself on mount so its grid has a real, connectable session to
//! show real data for, alongside (not replacing) the local mock simulation
//! it already had. See the step-7 report for how this was exercised with a
//! second, real local process actually connecting and reporting real levels.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingua_common::{ServerToClient, StudentId, TEACHER_INTERCOM_PORT};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vocalis::teacher::{db, listen, mic, net, screen, state};

const LEVEL_EMIT_INTERVAL_MS: u64 = 150;
// A fresh `SharedState`'s `class_size` doesn't matter for this step — nothing
// here renders a seat grid off this state, only mic levels — so a generous
// fixed size just leaves room for any number of test connections.
const TEST_CLASS_SIZE: usize = 30;
/// Display name announced to students in `ServerToClient::StartScreenDemo` —
/// there's no stored teacher profile name in this Tauri prototype yet (see
/// `start_teacher_session`'s doc comment), so this is a fixed placeholder,
/// same spirit as `"Tauri (тест)"` below for discovery announcements.
const SCREEN_DEMO_PRESENTER_NAME: &str = "Преподаватель";

/// The teacher's class-wide mic broadcast (step 7.5 — `vocalis_roadmap.md`,
/// section 8). `teacher::mic::MicCapture` wraps a `cpal::Stream`, which isn't
/// `Send` on macOS (see `student_mic.rs`'s matching comment) — confined here
/// to one dedicated OS thread for its whole lifetime, exactly like
/// `student_mic.rs`'s `MicMeter`, rather than trying to move it into a
/// `tauri::async_runtime` task.
pub struct MicBroadcast {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl Drop for MicBroadcast {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The teacher's private two-way intercom with one student (step 7.5).
/// Structurally identical to `MicBroadcast` — a second, independent `cpal`
/// capture (so it doesn't interfere with a concurrent class-wide broadcast,
/// exactly like `teacher::app::TeacherApp::toggle_intercom`), confined to
/// its own OS thread for the same non-`Send`-stream reason.
pub struct Intercom {
    student_id: StudentId,
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl Drop for Intercom {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct TeacherSession {
    // Read directly by `start_own_screen_demo`/`stop_own_screen_demo` (the
    // student roster, `screen_demo` field) in addition to being cloned into
    // the spawned tasks below.
    app_state: state::AppState,
    pin: String,
    class_name: String,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
    /// The teacher's own-screen broadcast task (step 7 part B), separate
    /// from `tasks` because it starts/stops independently of the session
    /// itself — mirrors `teacher::app::TeacherApp::screen_demo_task`.
    screen_demo_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// The teacher's mic broadcast (step 7.5), same independent start/stop
    /// lifetime as `screen_demo_task` — dropping this (or the whole
    /// `TeacherSession`) stops it via `MicBroadcast`'s own `Drop`.
    mic_broadcast: Option<MicBroadcast>,
    /// The teacher's private intercom, if any (step 7.5) — independent of
    /// `mic_broadcast` (separate `cpal` capture, separate atomic level).
    intercom: Option<Intercom>,
    /// Decoded listen-in audio, ready for real local speaker playback —
    /// `listen::run_listen_receiver` (spawned once below, always-on and idle
    /// until `listening_to` names someone) plays it automatically via its
    /// own `ensure_output_started`. `pub(crate)` (not private) solely so the
    /// real E2E test can read its length as proof real audio arrived — same
    /// reasoning as `student_session.rs`'s `mix` field.
    #[allow(dead_code)]
    pub(crate) listen_queue: listen::ListenQueue,
}

impl Drop for TeacherSession {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(task) = &self.screen_demo_task {
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

    // Always-on, idle until `start_listen` sets `listening_to` — exactly the
    // same shape as `teacher::app::TeacherApp::new` spawning this once at
    // startup (see that function's own listen-in setup).
    let listen_queue = listen::new_listen_queue();
    {
        let app_state = app_state.clone();
        let listen_queue = listen_queue.clone();
        let output_rate = listen::default_output_sample_rate();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = listen::run_listen_receiver(app_state, listen_queue, output_rate).await {
                eprintln!("[teacher_session] listen-in receiver stopped: {e:#}");
            }
        }));
    }

    let info = TeacherSessionInfo { pin: pin.clone(), control_port: lingua_common::CONTROL_PORT, class_name: class_name.clone() };
    *guard = Some(TeacherSession {
        app_state,
        pin,
        class_name,
        tasks,
        screen_demo_task: None,
        mic_broadcast: None,
        intercom: None,
        listen_queue,
    });
    Ok(info)
}

#[tauri::command]
pub fn stop_teacher_session(session: State<TeacherSessionState>) {
    *session.0.lock().unwrap() = None;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnScreenDemoInfo {
    pub target_count: usize,
}

/// Starts the real class-wide broadcast of the teacher's own screen (step 7
/// part B — `vocalis_roadmap.md`, section 8): reuses
/// `teacher::screen::run_own_screen_demo` unchanged, targeting every
/// currently connected real student, exactly mirroring
/// `teacher::app::TeacherApp::toggle_own_screen_demo`'s orchestration
/// (`ServerToClient::StartScreenDemo` to each target, then `screen_demo` set
/// on `SharedState` before the capture/encode task starts — that field is
/// what both `run_own_screen_demo` checks each tick to know whether to keep
/// running, and what a presenting student's relay would check, though this
/// prototype only wires the teacher's-own-screen source).
#[tauri::command]
pub fn start_own_screen_demo(session: State<TeacherSessionState>) -> Result<OwnScreenDemoInfo, String> {
    let mut guard = session.0.lock().unwrap();
    let teacher_session = guard.as_mut().ok_or("нет активной сессии преподавателя")?;

    if teacher_session.screen_demo_task.is_some() {
        let target_count = teacher_session.app_state.lock().unwrap().students.len();
        return Ok(OwnScreenDemoInfo { target_count });
    }

    let app_state = teacher_session.app_state.clone();
    let targets: Vec<StudentId> = {
        let mut state_guard = app_state.lock().unwrap();
        let targets: Vec<StudentId> = state_guard.students.keys().copied().collect();
        if targets.is_empty() {
            return Err("нет подключенных учеников".to_string());
        }
        for id in &targets {
            if let Some(s) = state_guard.students.get(id) {
                let _ = s.to_client.send(ServerToClient::StartScreenDemo { presenter: SCREEN_DEMO_PRESENTER_NAME.to_string() });
            }
        }
        state_guard.screen_demo = Some(state::ScreenDemo {
            source: state::ScreenDemoSource::Teacher,
            presenter_name: SCREEN_DEMO_PRESENTER_NAME.to_string(),
            targets: targets.clone(),
        });
        targets
    };

    let target_count = targets.len();
    let task_state = app_state;
    // `starting_level` 0 (the top of `video::QUALITY_LADDER`) — this
    // prototype has no stored per-teacher video-quality setting
    // (`settings::VideoQuality::ladder_level`) to read yet, unlike the egui
    // app; `AdaptiveQuality` will degrade from there on its own if needed.
    teacher_session.screen_demo_task = Some(tauri::async_runtime::spawn(async move {
        screen::run_own_screen_demo(task_state, targets, 0).await;
    }));

    Ok(OwnScreenDemoInfo { target_count })
}

#[tauri::command]
pub fn stop_own_screen_demo(session: State<TeacherSessionState>) {
    let mut guard = session.0.lock().unwrap();
    let Some(teacher_session) = guard.as_mut() else { return };
    if let Some(task) = teacher_session.screen_demo_task.take() {
        task.abort();
    }
    let mut state_guard = teacher_session.app_state.lock().unwrap();
    if let Some(demo) = state_guard.screen_demo.take() {
        for id in &demo.targets {
            if let Some(s) = state_guard.students.get(id) {
                let _ = s.to_client.send(ServerToClient::StopScreenDemo);
            }
        }
    }
}

/// Polls `stop` every 100ms — `teacher::mic::run_mic_broadcast` has no
/// cancellation of its own (it just loops on `rx.recv()` until the channel
/// closes), so `run_mic_broadcast_thread` races it against this inside a
/// `tokio::select!` to stop promptly when `stop_mic_broadcast` is called,
/// without needing to modify `app/`.
async fn wait_for_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn run_mic_broadcast_thread(state: state::AppState, stop: Arc<AtomicBool>, ready_tx: std::sync::mpsc::Sender<Result<(), String>>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture, native_rate) = match mic::start_mic_capture(tx, &mic::MIC_LEVEL_MILLIS, None) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));

    // `enable_all` (not just `enable_time`, unlike `student_mic.rs`'s mini
    // runtime): `run_mic_broadcast` does real UDP I/O, so this needs the IO
    // driver too, not just timers.
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[teacher_session] mic broadcast runtime failed to start: {e:#}");
            return;
        }
    };
    rt.block_on(async {
        tokio::select! {
            res = mic::run_mic_broadcast(state, rx, native_rate) => {
                if let Err(e) = res {
                    eprintln!("[teacher_session] mic broadcast stopped: {e:#}");
                }
            }
            _ = wait_for_stop(stop) => {}
        }
    });
    drop(capture); // explicit: dies on this same thread, where it was created
}

/// Starts the teacher's mic broadcast to the whole class (step 7.5 —
/// `vocalis_roadmap.md`, section 8): reuses `teacher::mic::start_mic_capture`
/// + `teacher::mic::run_mic_broadcast` unchanged — the same capture/
/// resample/Opus-encode/UDP-fan-out pipeline the egui teacher console's own
/// mic-broadcast toggle uses. Unlike the screen demo, no `ServerToClient`
/// announcement is needed first: `run_mic_broadcast` just reads
/// `SharedState.student_addrs_with_keys()` fresh on every frame, so students
/// who join mid-broadcast are picked up automatically.
#[tauri::command]
pub fn start_mic_broadcast(session: State<TeacherSessionState>) -> Result<(), String> {
    let mut guard = session.0.lock().unwrap();
    let teacher_session = guard.as_mut().ok_or("нет активной сессии преподавателя")?;
    if teacher_session.mic_broadcast.is_some() {
        return Ok(());
    }

    let app_state = teacher_session.app_state.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread = std::thread::spawn(move || run_mic_broadcast_thread(app_state, thread_stop, ready_tx));

    match ready_rx.recv() {
        Ok(Ok(())) => {
            teacher_session.mic_broadcast = Some(MicBroadcast { stop, _thread: thread });
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err("mic broadcast thread exited before reporting readiness".to_string()),
    }
}

#[tauri::command]
pub fn stop_mic_broadcast(session: State<TeacherSessionState>) {
    if let Some(teacher_session) = session.0.lock().unwrap().as_mut() {
        teacher_session.mic_broadcast = None;
    }
}

/// Starts listening in on one connected student's mic in real time (step
/// 7.5 — `vocalis_roadmap.md`, section 8): reuses `SharedState::
/// start_listening` unchanged — the same method `teacher::app::
/// TeacherApp::toggle_listen`'s "start" path calls, which sends the target
/// `ServerToClient::StartMicUpload` (and whoever was previously being
/// listened to, if different, `StopMicUpload`) and updates `listening_to`,
/// which `run_listen_receiver` (already running, spawned in
/// `start_teacher_session`) checks on every incoming packet to know whose
/// session key to decrypt with.
#[tauri::command]
pub fn start_listen(session: State<TeacherSessionState>, student_id: String) -> Result<(), String> {
    let id: StudentId = student_id.parse().map_err(|_| format!("invalid student id: {student_id}"))?;
    let guard = session.0.lock().unwrap();
    let teacher_session = guard.as_ref().ok_or("нет активной сессии преподавателя")?;
    let mut state_guard = teacher_session.app_state.lock().unwrap();
    if !state_guard.students.contains_key(&id) {
        return Err("ученик не подключён".to_string());
    }
    state_guard.start_listening(id);
    Ok(())
}

/// Stops listening in, if anyone's currently being listened to — mirrors
/// `toggle_listen`'s "stop" path (`SharedState` has no symmetric
/// `stop_listening` method of its own, only `start_listening`, so this
/// replicates those same three lines rather than adding one to `app/`).
#[tauri::command]
pub fn stop_listen(session: State<TeacherSessionState>) {
    let guard = session.0.lock().unwrap();
    let Some(teacher_session) = guard.as_ref() else { return };
    let mut state_guard = teacher_session.app_state.lock().unwrap();
    if let Some(id) = state_guard.listening_to.take() {
        if let Some(s) = state_guard.students.get(&id) {
            let _ = s.to_client.send(ServerToClient::StopMicUpload);
        }
    }
}

fn run_intercom_thread(
    target_ip: std::net::IpAddr,
    key: lingua_common::SessionKey,
    stop: Arc<AtomicBool>,
    ready_tx: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture, native_rate) = match mic::start_mic_capture(tx, &mic::INTERCOM_MIC_LEVEL_MILLIS, None) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[teacher_session] intercom runtime failed to start: {e:#}");
            return;
        }
    };
    let target = std::net::SocketAddr::new(target_ip, TEACHER_INTERCOM_PORT);
    rt.block_on(async {
        tokio::select! {
            res = mic::run_intercom_send(rx, native_rate, target, key) => {
                if let Err(e) = res {
                    eprintln!("[teacher_session] intercom send stopped: {e:#}");
                }
            }
            _ = wait_for_stop(stop) => {}
        }
    });
    drop(capture); // explicit: dies on this same thread, where it was created
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntercomInfo {
    pub student_name: String,
}

/// Opens (or switches to a different student) a private two-way intercom
/// (step 7.5): reuses `teacher::mic::run_intercom_send` unchanged for the
/// teacher's own voice going to just this student — a second, independent
/// mic capture, so a concurrent class-wide broadcast (if running) is
/// untouched — and `SharedState::start_listening` (the same method plain
/// listen-in uses) so the teacher hears them back. Mirrors
/// `TeacherApp::toggle_intercom`'s orchestration exactly, including
/// stopping any previous intercom first.
#[tauri::command]
pub fn start_intercom(session: State<TeacherSessionState>, student_id: String) -> Result<IntercomInfo, String> {
    let id: StudentId = student_id.parse().map_err(|_| format!("invalid student id: {student_id}"))?;
    let mut guard = session.0.lock().unwrap();
    let teacher_session = guard.as_mut().ok_or("нет активной сессии преподавателя")?;

    if let Some(prev) = teacher_session.intercom.take() {
        let mut state_guard = teacher_session.app_state.lock().unwrap();
        if state_guard.talking_to == Some(prev.student_id) {
            state_guard.talking_to = None;
        }
        if let Some(s) = state_guard.students.get(&prev.student_id) {
            let _ = s.to_client.send(ServerToClient::StopIntercom);
        }
        drop(state_guard);
        drop(prev); // stops the previous mic capture/send task
    }

    let (name, ip, key) = {
        let state_guard = teacher_session.app_state.lock().unwrap();
        let s = state_guard.students.get(&id).ok_or("ученик не подключён")?;
        (s.name.clone(), s.ip, s.session_key)
    };
    teacher_session.app_state.lock().unwrap().start_listening(id);

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread = std::thread::spawn(move || run_intercom_thread(ip, key, thread_stop, ready_tx));

    match ready_rx.recv() {
        Ok(Ok(())) => {
            teacher_session.intercom = Some(Intercom { student_id: id, stop, _thread: thread });
            let mut state_guard = teacher_session.app_state.lock().unwrap();
            state_guard.talking_to = Some(id);
            if let Some(s) = state_guard.students.get(&id) {
                let _ = s.to_client.send(ServerToClient::StartIntercom);
            }
            Ok(IntercomInfo { student_name: name })
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err("intercom thread exited before reporting readiness".to_string()),
    }
}

/// Closes the intercom, if one's open. Leaves plain listen-in
/// (`listening_to`) alone — mirrors `TeacherApp::stop_intercom`'s own doc
/// comment on why: the two are independent once intercom has started.
#[tauri::command]
pub fn stop_intercom(session: State<TeacherSessionState>) {
    let mut guard = session.0.lock().unwrap();
    let Some(teacher_session) = guard.as_mut() else { return };
    let Some(intercom) = teacher_session.intercom.take() else { return };
    let mut state_guard = teacher_session.app_state.lock().unwrap();
    if state_guard.talking_to == Some(intercom.student_id) {
        state_guard.talking_to = None;
    }
    if let Some(s) = state_guard.students.get(&intercom.student_id) {
        let _ = s.to_client.send(ServerToClient::StopIntercom);
    }
}
