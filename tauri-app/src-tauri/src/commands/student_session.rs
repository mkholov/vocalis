//! Real network connection to a teacher's session — the screen-demo video
//! receiver (step 7 part B), the class-wide mic-broadcast and private
//! intercom receivers, and this student's own outbound mic for listen-in/
//! groups/intercom (all step 7.5), `vocalis_roadmap.md` section 8. Reuses
//! `student::net::connect_to_teacher`, `student::screen::
//! run_screen_demo_receiver`, `student::audio::run_mic_broadcast_receiver`,
//! `student::audio::run_intercom_receiver`, and `student::audio::
//! run_outbound_and_group_audio` unchanged — the exact same handshake/
//! session-key derivation and H.264/Opus send/receive/decode paths the egui
//! student app uses; nothing about any of these network protocols is
//! reimplemented here. What's new is only the "poll the decoded frame for
//! changes, JPEG-encode it, emit it to the webview" glue for video, reusing
//! `screen_frame`'s helper — the same one `screen_demo.rs`'s self-preview
//! uses — so both paths emit the identical `screen-demo-frame` event shape
//! and the frontend needs no changes to tell them apart. Audio needs no such
//! glue: mixing/playback (`run_mic_broadcast_receiver`) and mic capture/
//! upload (`run_outbound_and_group_audio`) happen entirely inside
//! `student::audio`/real speakers — this only has to create the shared
//! `SharedMix`, start the real mic capture, and keep the tasks running.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vocalis::student::{audio, mic, net, screen, state};
use lingua_common::ClientToServer;

use super::screen_frame::jpeg_data_url;

/// How often to check `demo_frame_version` for a new decoded frame.
/// Comfortably under the 66.7ms/15fps frame budget `video-bench` measured,
/// so this adds negligible latency on top of decode+JPEG themselves.
const DEMO_POLL_INTERVAL_MS: u64 = 20;
/// Assignments arrive rarely (a teacher click, not a media stream) — nowhere near frame-budget territory,
/// so a much coarser poll than the demo-frame one above is plenty responsive.
const ASSIGNMENT_POLL_INTERVAL_MS: u64 = 200;
/// How often to check whether the connection to the teacher is still up. A dropped TCP connection (the
/// teacher ends the lesson, closes the app, or the network blips) is invisible to this student's own UI
/// otherwise — `connect_student_session`'s promise only ever resolves once, at the start, so nothing tells
/// a still-mounted `StudentConsole` its "подключено к …" header has gone stale. Coarse on purpose: this is
/// "tell the user eventually," not a latency-sensitive media path.
const DISCONNECT_POLL_INTERVAL_MS: u64 = 300;
/// Chat is a person typing, not a media stream — same reasoning as `ASSIGNMENT_POLL_INTERVAL_MS`.
const CHAT_POLL_INTERVAL_MS: u64 = 250;
/// `student::net::connect_to_teacher` has no readiness callback of its own
/// (unlike `teacher::mic::start_mic_capture`, which `student_mic.rs` gets a
/// synchronous ready signal from) — it just sets `connected_teacher` once the
/// Hello/Welcome handshake succeeds, then runs its receive loop forever. This
/// bounds how long `connect_student_session` polls for that before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// This student's own mic, feeding `student::audio::run_outbound_and_group_audio`
/// (listen-in, groups, intercom — step 7.5). `student::mic::MicCapture`
/// wraps a `cpal::Stream`, which isn't `Send` on macOS — same reasoning as
/// `teacher_session.rs`'s `MicBroadcast`, confined to one dedicated OS
/// thread rather than moved into a `tauri::async_runtime` task.
pub struct OutboundMic {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
    /// The capture's native sample rate — what `student_recording.rs` needs to
    /// label a recording with, since the recording tap in
    /// `audio::run_outbound_and_group_audio` stores raw native-rate PCM.
    pub native_rate: u32,
}

impl Drop for OutboundMic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct StudentSession {
    // Kept alive for its `Arc` refcount (the spawned tasks each hold their
    // own clone) and, since step 7.5's groups, also for the real E2E test to
    // read `peer_addrs`/`peer_keys` off directly — real protocol-level proof
    // that a `JoinGroup` control message actually arrived, independent of
    // whether either student has a real mic to also verify audio with.
    #[allow(dead_code)]
    pub app_state: state::AppState,
    /// `pub` (not private) solely so the real two-process E2E test in
    /// `tests/command_bridge.rs` can read `mix.lock().unwrap().broadcast.len()` as proof that
    /// real decoded audio actually arrived — there's no webview event to
    /// observe here the way `screen-demo-frame` lets the video path prove
    /// itself, since mixing/playback happens entirely inside
    /// `student::audio` (real speakers, not something to route through IPC).
    /// Only that test ever reads it outside of the clone already passed into
    /// the receiver task below, hence `#[allow(dead_code)]` for non-test builds.
    #[allow(dead_code)]
    pub mix: audio::SharedMix,
    teacher_name: String,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
    /// `None` if this machine has no usable input device — listen-in/
    /// groups/intercom simply aren't available then, same non-fatal
    /// treatment `student_mic.rs`'s meter already gives a missing mic.
    /// `pub` so the real E2E test can check whether it's `Some`
    /// before asserting that listen-in audio actually arrived — a CI runner
    /// with no input device is an environment limitation, not a bug, same
    /// reasoning as `student_mic_meter_start_stop_does_not_panic`.
    #[allow(dead_code)]
    pub outbound_mic: Option<OutboundMic>,
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

/// One `TestQuestion`, in the exact shape `lib/assignments.ts`'s `AssignmentContent` expects — the
/// counterpart of `teacher_session.rs`'s `TestQuestionDto` (that one `Deserialize`s the same shape from
/// the editor; this one `Serialize`s it for a student to render).
#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestQuestionDto {
    pub text: String,
    pub options: Vec<String>,
    pub correct_index: usize,
}

/// The counterpart of `teacher_session.rs`'s `AssignmentContentDto`, in the same
/// `{ kind: "test" | "listening" | "reading", ... }` shape `lib/assignments.ts` already knows how to
/// render — a real `ServerToClient::AssignmentOffer`'s content reaches the student console with no
/// reshaping on the frontend.
#[derive(Serialize, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssignmentContentDto {
    Test { questions: Vec<TestQuestionDto> },
    Listening { material_title: String, questions: Vec<String> },
    Reading { text: String },
}

impl From<&lingua_common::AssignmentContent> for AssignmentContentDto {
    fn from(c: &lingua_common::AssignmentContent) -> Self {
        match c {
            lingua_common::AssignmentContent::Test { questions } => AssignmentContentDto::Test {
                questions: questions
                    .iter()
                    .map(|q| TestQuestionDto { text: q.text.clone(), options: q.options.clone(), correct_index: q.correct_index })
                    .collect(),
            },
            lingua_common::AssignmentContent::Listening { material_title, questions } => {
                AssignmentContentDto::Listening { material_title: material_title.clone(), questions: questions.clone() }
            }
            lingua_common::AssignmentContent::Reading { text } => AssignmentContentDto::Reading { text: text.clone() },
        }
    }
}

/// `AssignmentEntry::last_score` once a `Test` has been submitted — `(correct, total)` as a named pair
/// instead of a bare tuple, so the JSON reads as `{ "correct": 1, "total": 2 }` rather than `[1, 2]`.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TestScoreDto {
    pub correct: u32,
    pub total: u32,
}

/// One real assignment this student has received, straight from `student::state::AssignmentEntry` —
/// emitted (the whole current list, same "just resend the snapshot" convention `student-levels` uses on
/// the teacher side) as the `"assignments"` event whenever a new one arrives or an answer is submitted for
/// one of them. A `content: None` entry (a bare label-only "quick send", e.g. egui's `Dialogue` — see
/// `AssignmentOffer`'s own doc comment) has nothing here to render yet, so it's left out rather than shown
/// as broken. `done`/`last_score` are this same student's own `AssignmentEntry` fields — real state, kept
/// in step with `submit_test_answers`/`submit_assignment_done` below (which are what set them), so the
/// frontend can tell "already answered" from "still open" without any state of its own to lose track of.
#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentDto {
    pub id: String,
    pub title: String,
    pub content: AssignmentContentDto,
    pub done: bool,
    pub last_score: Option<TestScoreDto>,
}

/// Polls `stop` every 100ms — `run_outbound_and_group_audio` has no
/// cancellation of its own (loops on `mic_rx.recv()` until the channel
/// closes), so this races it inside a `tokio::select!`, same pattern as
/// `teacher_session.rs`'s `wait_for_stop` for the mic broadcast.
async fn wait_for_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn run_outbound_mic_thread(
    state: state::AppState,
    mix: audio::SharedMix,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    ready_tx: std::sync::mpsc::Sender<Result<u32, String>>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture, native_rate) = match mic::start_mic_capture(tx, None) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
    };
    let _ = ready_tx.send(Ok(native_rate));

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[student_session] outbound mic runtime failed to start: {e:#}");
            return;
        }
    };
    rt.block_on(async {
        tokio::select! {
            res = audio::run_outbound_and_group_audio(state, mix, rx, native_rate, output_rate) => {
                if let Err(e) = res {
                    eprintln!("[student_session] outbound/group audio stopped: {e:#}");
                }
            }
            _ = wait_for_stop(stop) => {}
        }
    });
    drop(capture); // explicit: dies on this same thread, where it was created
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
    let mix = audio::new_mix_state();
    let output_rate = audio::default_output_sample_rate();
    {
        // Always-on, idle until the teacher actually broadcasts — exactly
        // like `student::app::StudentApp::new` spawning this unconditionally
        // at startup. `default_output_sample_rate()` queries the real
        // configured output device once, same as the egui app.
        let recv_state = app_state.clone();
        let recv_mix = mix.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = audio::run_mic_broadcast_receiver(recv_state, recv_mix, output_rate).await {
                eprintln!("[student_session] mic-broadcast receiver stopped: {e:#}");
            }
        }));
    }
    {
        // Always-on, idle until the teacher opens a private intercom with
        // this student — same shape as the mic-broadcast receiver above,
        // just mixing into `mix.intercom` instead of `mix.broadcast`.
        let recv_state = app_state.clone();
        let recv_mix = mix.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            if let Err(e) = audio::run_intercom_receiver(recv_state, recv_mix, output_rate).await {
                eprintln!("[student_session] intercom receiver stopped: {e:#}");
            }
        }));
    }

    // This student's own mic, feeding listen-in/groups/intercom (step 7.5) —
    // non-fatal if this machine has no usable input device, same as
    // `student_mic.rs`'s meter: the rest of the session still works.
    let outbound_mic = {
        let mic_state = app_state.clone();
        let mic_mix = mix.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
        let thread = std::thread::spawn(move || run_outbound_mic_thread(mic_state, mic_mix, output_rate, thread_stop, ready_tx));
        match ready_rx.recv() {
            Ok(Ok(native_rate)) => Some(OutboundMic { stop, _thread: thread, native_rate }),
            Ok(Err(e)) => {
                eprintln!("[student_session] no microphone available for listen-in/groups: {e}");
                None
            }
            Err(_) => {
                eprintln!("[student_session] outbound mic thread exited before reporting readiness");
                None
            }
        }
    };
    {
        // Fires exactly once: `connected_teacher` only ever goes `Some` -> `None` (`net::connect_to_teacher`
        // has no reconnect logic — once its read loop exits, this `AppState` is done for good), so there is
        // nothing left to watch once it does. Real detection, not a guess: this is the same field the
        // control connection's own read loop clears on a real socket close (teacher stopped the lesson,
        // quit the app, or the network died) — not a heartbeat/ping this command invents itself.
        let poll_state = app_state.clone();
        let app = app.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(DISCONNECT_POLL_INTERVAL_MS));
            loop {
                interval.tick().await;
                if poll_state.lock().unwrap().connected_teacher.is_none() {
                    let _ = app.emit("teacher-disconnected", ());
                    break;
                }
            }
        }));
    }
    {
        // Always-on, same idle-until-something-happens shape as the receivers above: nothing to show
        // until the teacher actually sends something, real from the first assignment on.
        let poll_state = app_state.clone();
        let app = app.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            // Compares the *whole built snapshot*, not just `assignments.len()`: submitting an answer
            // mutates an existing entry's `done`/`last_score` in place rather than adding a new one, so a
            // length-only check would miss it and the console would never see its own submission confirmed.
            // Starts at `Some(vec![])`, not `None` — matching the real starting state (no assignments yet)
            // exactly, so the first tick doesn't read as "changed" and emit a spurious empty event before
            // anything has actually happened.
            let mut last_sent: Option<Vec<AssignmentDto>> = Some(Vec::new());
            let mut interval = tokio::time::interval(Duration::from_millis(ASSIGNMENT_POLL_INTERVAL_MS));
            loop {
                interval.tick().await;
                let list: Vec<AssignmentDto> = poll_state
                    .lock()
                    .unwrap()
                    .assignments
                    .iter()
                    .filter_map(|a| {
                        a.content.as_ref().map(|c| AssignmentDto {
                            id: a.id.to_string(),
                            title: a.title.clone(),
                            content: c.into(),
                            done: a.done,
                            last_score: a.last_score.map(|(correct, total)| TestScoreDto { correct, total }),
                        })
                    })
                    .collect();
                if last_sent.as_ref() == Some(&list) {
                    continue;
                }
                last_sent = Some(list.clone());
                let _ = app.emit("assignments", list);
            }
        }));
    }
    {
        // Real incoming chat: `student::state::AppState.chat_log` only ever grows (`student::net`'s
        // unchanged read loop pushes to it on a real `ServerToClient::ChatMessage`) — this student's own
        // sent messages never land here (`submit_chat_message` below only ever sends, it never touches
        // `chat_log`), so every entry this sees is a real message from the teacher, safe to just forward.
        let poll_state = app_state.clone();
        let app = app.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let mut last_len = poll_state.lock().unwrap().chat_log.len();
            let mut interval = tokio::time::interval(Duration::from_millis(CHAT_POLL_INTERVAL_MS));
            loop {
                interval.tick().await;
                let guard = poll_state.lock().unwrap();
                if guard.chat_log.len() > last_len {
                    for entry in &guard.chat_log[last_len..] {
                        let _ = app.emit("chat-message", ChatMessageDto { from: entry.from.clone(), text: entry.text.clone() });
                    }
                    last_len = guard.chat_log.len();
                }
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
    *guard = Some(StudentSession { app_state, mix, teacher_name, tasks, outbound_mic });
    Ok(info)
}

#[tauri::command]
pub fn disconnect_student_session(session: State<StudentSessionState>) {
    let mut guard = session.0.lock().unwrap();
    // A recording in progress lives in this session's state — save it rather than
    // letting it vanish with it (leaving the console mid-recording, say).
    if let Some(student) = guard.as_ref() {
        let active = student.app_state.lock().unwrap().recording.take();
        if let Some(active) = active {
            if let Err(e) = super::student_recording::save_active(active) {
                eprintln!("[student_session] couldn't save the in-progress recording on disconnect: {e}");
            }
        }
    }
    *guard = None;
}

/// Real "поднять руку": sends `ClientToServer::RequestHelp` over this session's already-encrypted control
/// connection — the same message and the same `net::connect_to_teacher`-owned channel the egui student
/// app's `toggle_help` uses (`state.to_server`), so the teacher side (`teacher::net`'s handler, unchanged)
/// needs no new plumbing to see it: it already sets `Student::needs_help`, which `emit_levels` in
/// `teacher_session.rs` now reports as `needsHelp` on every connected student.
#[tauri::command]
pub fn set_hand_raised(session: State<StudentSessionState>, raised: bool) -> Result<(), String> {
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет активного подключения к преподавателю")?;
    let to_server = student.app_state.lock().unwrap().to_server.clone();
    let tx = to_server.ok_or("подключение к преподавателю ещё не готово")?;
    tx.send(ClientToServer::RequestHelp { needed: raised }).map_err(|_| "соединение с преподавателем разорвано".to_string())
}

/// Grades a real `Test` assignment client-side and reports the result — the same rule egui's own
/// `submit_test` uses (compare each chosen option's index against `TestQuestion::correct_index`, sent to
/// this student as part of the real `AssignmentOffer` content — see that struct's own doc comment for why
/// the correct answer already being on this machine is by design, not a leak this introduces). Also marks
/// the assignment done, both locally (so `list_assignments`'s next real snapshot already reflects it,
/// before the teacher's own ack even arrives) and over the wire via a real `ClientToServer::TestResult` —
/// `teacher::net`'s existing handler (unchanged) persists it as a real `test_results` row and marks the
/// `assignments` row done, which is what `class_stats` already reads.
///
/// `answers[i]` is the option index chosen for `questions[i]`; refuses if the counts don't match (an
/// unanswered question) or if this assignment was already submitted — a student can send an answer exactly
/// once.
#[tauri::command]
pub fn submit_test_answers(session: State<StudentSessionState>, assignment_id: String, answers: Vec<usize>) -> Result<TestScoreDto, String> {
    let id: lingua_common::AssignmentId = assignment_id.parse().map_err(|_| format!("invalid assignment id: {assignment_id}"))?;
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет активного подключения к преподавателю")?;
    let mut state_guard = student.app_state.lock().unwrap();

    let assignment = state_guard.assignments.iter().find(|a| a.id == id).ok_or("задание не найдено")?;
    if assignment.done {
        return Err("ответ на это задание уже отправлен".to_string());
    }
    let questions = match &assignment.content {
        Some(lingua_common::AssignmentContent::Test { questions }) => questions.clone(),
        _ => return Err("это задание не тест".to_string()),
    };
    if answers.len() != questions.len() {
        return Err("нужно ответить на все вопросы".to_string());
    }

    let correct = questions.iter().zip(answers.iter()).filter(|(q, &a)| q.correct_index == a).count() as u32;
    let total = questions.len() as u32;

    if let Some(a) = state_guard.assignments.iter_mut().find(|a| a.id == id) {
        a.done = true;
        a.last_score = Some((correct, total));
    }
    let to_server = state_guard.to_server.clone();
    drop(state_guard);

    let tx = to_server.ok_or("подключение к преподавателю ещё не готово")?;
    tx.send(ClientToServer::TestResult { id, correct, total }).map_err(|_| "соединение с преподавателем разорвано".to_string())?;
    Ok(TestScoreDto { correct, total })
}

/// Marks a real `Listening`/`Reading` assignment done — these aren't auto-graded (no right answer to check
/// client-side), so there's no score, just a real `ClientToServer::AssignmentDone`, mirroring egui's own
/// `mark_assignment_done` exactly: sets `done` locally first, then sends. Refuses a second submission the
/// same way `submit_test_answers` does.
#[tauri::command]
pub fn submit_assignment_done(session: State<StudentSessionState>, assignment_id: String) -> Result<(), String> {
    let id: lingua_common::AssignmentId = assignment_id.parse().map_err(|_| format!("invalid assignment id: {assignment_id}"))?;
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет активного подключения к преподавателю")?;
    let mut state_guard = student.app_state.lock().unwrap();

    let already_done = state_guard.assignments.iter().find(|a| a.id == id).ok_or("задание не найдено")?.done;
    if already_done {
        return Err("ответ на это задание уже отправлен".to_string());
    }
    if let Some(a) = state_guard.assignments.iter_mut().find(|a| a.id == id) {
        a.done = true;
    }
    let to_server = state_guard.to_server.clone();
    drop(state_guard);

    let tx = to_server.ok_or("подключение к преподавателю ещё не готово")?;
    tx.send(ClientToServer::AssignmentDone { id }).map_err(|_| "соединение с преподавателем разорвано".to_string())
}

/// One real chat message, in or out — same `{from, text}` shape as `student::state::ChatEntry` and the two
/// wire messages themselves (`ServerToClient::ChatMessage`/`ClientToServer::ChatMessage`), so nothing needs
/// reshaping in either direction. A separate type from `teacher_session.rs`'s own `ChatMessageDto` (same
/// shape) purely to keep the two command modules independent, the same reason `AssignmentContentDto` is
/// duplicated between them.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageDto {
    pub from: String,
    pub text: String,
}

/// Sends a real chat message to the teacher — there is only ever one possible recipient from a student's
/// side, unlike the teacher's own `send_chat_message`, so no target to pick. Reuses the real, existing
/// `ClientToServer::ChatMessage` — `teacher::net`'s handling of it (unchanged) already logs it into
/// `SharedState.chat_log` under this student's real name, which is what the teacher's own "Чат" panel and
/// its `chat-message` event read.
#[tauri::command]
pub fn submit_chat_message(session: State<StudentSessionState>, text: String) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Пустое сообщение не отправляется".to_string());
    }
    let guard = session.0.lock().unwrap();
    let student = guard.as_ref().ok_or("нет активного подключения к преподавателю")?;
    let to_server = student.app_state.lock().unwrap().to_server.clone();
    let tx = to_server.ok_or("подключение к преподавателю ещё не готово")?;
    tx.send(ClientToServer::ChatMessage { text }).map_err(|_| "соединение с преподавателем разорвано".to_string())
}
