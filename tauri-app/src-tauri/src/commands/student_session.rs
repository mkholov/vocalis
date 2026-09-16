//! Real network connection to a teacher's session — the screen-demo video
//! receiver (step 7 part B), the class-wide mic-broadcast receiver (step
//! 7.5), and this student's own outbound mic for listen-in/groups/intercom
//! (also step 7.5), `vocalis_roadmap.md` section 8. Reuses `student::net::
//! connect_to_teacher`, `student::screen::run_screen_demo_receiver`,
//! `student::audio::run_mic_broadcast_receiver`, and `student::audio::
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

/// This student's own mic, feeding `student::audio::run_outbound_and_group_audio`
/// (listen-in, groups, intercom — step 7.5). `student::mic::MicCapture`
/// wraps a `cpal::Stream`, which isn't `Send` on macOS — same reasoning as
/// `teacher_session.rs`'s `MicBroadcast`, confined to one dedicated OS
/// thread rather than moved into a `tauri::async_runtime` task.
pub(crate) struct OutboundMic {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl Drop for OutboundMic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct StudentSession {
    // Kept alive only for its `Arc` refcount — the spawned tasks each hold
    // their own clone and are the only things that ever read from it. Not
    // read directly through this field, but dropping it early would be a
    // real bug if a future change removed one of those clones, so it stays.
    #[allow(dead_code)]
    app_state: state::AppState,
    /// `pub(crate)` (not private) solely so the real two-process E2E test in
    /// `lib.rs` can read `mix.lock().unwrap().broadcast.len()` as proof that
    /// real decoded audio actually arrived — there's no webview event to
    /// observe here the way `screen-demo-frame` lets the video path prove
    /// itself, since mixing/playback happens entirely inside
    /// `student::audio` (real speakers, not something to route through IPC).
    /// Only that test ever reads it outside of the clone already passed into
    /// the receiver task below, hence `#[allow(dead_code)]` for non-test builds.
    #[allow(dead_code)]
    pub(crate) mix: audio::SharedMix,
    teacher_name: String,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
    /// `None` if this machine has no usable input device — listen-in/
    /// groups/intercom simply aren't available then, same non-fatal
    /// treatment `student_mic.rs`'s meter already gives a missing mic.
    /// `pub(crate)` so the real E2E test can check whether it's `Some`
    /// before asserting that listen-in audio actually arrived — a CI runner
    /// with no input device is an environment limitation, not a bug, same
    /// reasoning as `student_mic_meter_start_stop_does_not_panic`.
    #[allow(dead_code)]
    pub(crate) outbound_mic: Option<OutboundMic>,
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
    ready_tx: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture, native_rate) = match mic::start_mic_capture(tx, None) {
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

    // This student's own mic, feeding listen-in/groups/intercom (step 7.5) —
    // non-fatal if this machine has no usable input device, same as
    // `student_mic.rs`'s meter: the rest of the session still works.
    let outbound_mic = {
        let mic_state = app_state.clone();
        let mic_mix = mix.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = std::thread::spawn(move || run_outbound_mic_thread(mic_state, mic_mix, output_rate, thread_stop, ready_tx));
        match ready_rx.recv() {
            Ok(Ok(())) => Some(OutboundMic { stop, _thread: thread }),
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
    *session.0.lock().unwrap() = None;
}
