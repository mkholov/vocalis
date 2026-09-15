//! Live "my own mic" level for the student console (step 7, part A —
//! `vocalis_roadmap.md`, section 8). Reuses `teacher::mic::start_mic_capture`
//! unchanged — despite the module name, it's a plain `cpal` capture + RMS
//! function with no teacher-specific logic, and it already takes the level
//! atomic to report into as a parameter, so no student-specific variant is
//! needed. Only the "hold the stream open, drain samples, emit an event
//! periodically" wiring here is new.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vocalis::teacher::mic::start_mic_capture;

const LEVEL_EMIT_INTERVAL_MS: u64 = 120;

/// Not `teacher::mic::MIC_LEVEL_MILLIS` — that one is documented as backing
/// the *teacher's* broadcast VU meter specifically; this is the student
/// console's own, so it gets its own atomic even though the underlying
/// capture/RMS function is shared.
static STUDENT_MIC_LEVEL_MILLIS: AtomicI32 = AtomicI32::new(0);

/// `cpal::Stream` (inside `MicCapture`) isn't `Send` on macOS — its
/// CoreAudio property-listener callback is a boxed `dyn FnMut()`, which the
/// platform APIs don't guarantee is safe to hand to another thread. Rather
/// than wrapping it in an `unsafe impl Send` (the way `screen_capture.rs`'s
/// `MonitorCapture` does for a type it could actually reason was safe to
/// move), this keeps the capture confined to the one dedicated OS thread
/// that creates it for its entire lifetime — the `Mutex` that `tauri::State`
/// requires `Send + Sync` only ever holds a stop flag and a `JoinHandle`,
/// never the capture itself.
pub struct MicMeter {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl Drop for MicMeter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct MicMeterState(pub Mutex<Option<MicMeter>>);

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct MicLevelDto {
    pub level: i32,
}

#[tauri::command]
pub fn start_student_mic_meter<R: tauri::Runtime>(app: AppHandle<R>, meter: State<MicMeterState>) -> Result<(), String> {
    let mut guard = meter.0.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    // `mpsc::channel` (not a oneshot) purely to get the capture's `Result`
    // back out of the thread before returning from this command — the
    // channel itself is dropped right after.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();

    let thread = std::thread::spawn(move || {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let capture = match start_mic_capture(tx, &STUDENT_MIC_LEVEL_MILLIS, None) {
            Ok((capture, _sample_rate)) => capture,
            Err(e) => {
                let _ = ready_tx.send(Err(e.to_string()));
                return;
            }
        };
        let _ = ready_tx.send(Ok(()));

        // A tiny single-threaded runtime, local to this thread, just to drain
        // `rx` (the capture callback already stores the level itself — see
        // `teacher::mic::start_mic_capture`'s doc comment — this only exists
        // so the channel doesn't grow unbounded) and to emit on a timer.
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("build mic-meter runtime");
        rt.block_on(async {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(LEVEL_EMIT_INTERVAL_MS));
            loop {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                    _ = interval.tick() => {
                        let level = STUDENT_MIC_LEVEL_MILLIS.load(Ordering::Relaxed);
                        let _ = app.emit("mic-level", MicLevelDto { level });
                    }
                    chunk = rx.recv() => {
                        if chunk.is_none() {
                            break;
                        }
                    }
                }
            }
        });
        drop(capture); // explicit: dies on this same thread, where it was created
    });

    match ready_rx.recv() {
        Ok(Ok(())) => {
            *guard = Some(MicMeter { stop, _thread: thread });
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err("mic capture thread exited before reporting readiness".to_string()),
    }
}

#[tauri::command]
pub fn stop_student_mic_meter(meter: State<MicMeterState>) {
    *meter.0.lock().unwrap() = None;
    STUDENT_MIC_LEVEL_MILLIS.store(0, Ordering::Relaxed);
}
