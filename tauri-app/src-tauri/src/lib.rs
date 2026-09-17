//! Step 2 of the Tauri migration (`vocalis_roadmap.md`, section 8): the
//! existing Rust core (`vocalis`, `lingua-common` — DB/network/audio/video,
//! none of it egui-specific) wired in as Tauri commands. Still no screen
//! calls any of this — see `commands` for how to exercise it from the
//! webview's dev console, and see the `tests` module below for how it's
//! exercised through the real IPC dispatch path (command-name lookup +
//! argument/return serialization) without needing a real window.

mod commands;

/// Shared between the real entry point and the test harness below so both
/// register the exact same commands the same way — generic over `Runtime` so
/// tests can build this with Tauri's `MockRuntime` instead of a real window.
fn build_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::App<R> {
    builder
        .plugin(tauri_plugin_opener::init())
        .manage(commands::teacher_session::TeacherSessionState::default())
        .manage(commands::student_mic::MicMeterState::default())
        .manage(commands::screen_demo::ScreenDemoState::default())
        .manage(commands::student_session::StudentSessionState::default())
        .invoke_handler(tauri::generate_handler![
            commands::db::list_classes,
            commands::audio::list_audio_devices,
            commands::network::discover_teachers,
            commands::video::capture_screen_preview,
            commands::teacher_session::start_teacher_session,
            commands::teacher_session::stop_teacher_session,
            commands::teacher_session::start_own_screen_demo,
            commands::teacher_session::stop_own_screen_demo,
            commands::teacher_session::start_mic_broadcast,
            commands::teacher_session::stop_mic_broadcast,
            commands::teacher_session::start_listen,
            commands::teacher_session::stop_listen,
            commands::teacher_session::start_intercom,
            commands::teacher_session::stop_intercom,
            commands::student_mic::start_student_mic_meter,
            commands::student_mic::stop_student_mic_meter,
            commands::screen_demo::start_screen_demo,
            commands::screen_demo::stop_screen_demo,
            commands::student_session::connect_student_session,
            commands::student_session::disconnect_student_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    build_app(tauri::Builder::default()).run(|_app_handle, _event| {});
}

#[cfg(test)]
mod tests {
    //! Exercises each command through Tauri's actual IPC dispatch (command-name
    //! routing + serde (de)serialization of args/return value) via
    //! `tauri::test`'s `MockRuntime` — the same path the webview's
    //! `invoke("list_classes")` goes through, just without a real window. Every
    //! assertion here compares against the real, independently-obtained result
    //! (e.g. calling `vocalis::audio_devices` directly) rather than a hardcoded
    //! stub, so a command that silently stopped doing real work would fail.

    use tauri::test::mock_builder;
    use tauri::webview::InvokeRequest;
    use tauri::ipc::{CallbackFn, InvokeBody};

    fn invoke(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
        let app = super::build_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let body = match args {
            serde_json::Value::Null => InvokeBody::default(),
            other => InvokeBody::Json(other),
        };
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // Must match the scheme Tauri's ACL treats as "local" on this platform
            // (see the `tauri::test` module doc example) — the wrong one here
            // isn't a real IPC failure, just an artifact of this scheme check.
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body,
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };
        tauri::test::get_ipc_response(&webview, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
    }

    #[test]
    fn list_classes_returns_real_db_rows() {
        let expected = {
            let conn = vocalis::teacher::db::open().expect("open real vocalis db");
            vocalis::teacher::db::list_classes(&conn).expect("query real classes table")
        };
        let response = invoke("list_classes", serde_json::Value::Null).expect("command should succeed");
        let got = response.as_array().expect("expected a JSON array");
        assert_eq!(got.len(), expected.len(), "row count should match a direct DB query");
        for (row, class) in got.iter().zip(expected.iter()) {
            assert_eq!(row["id"], class.id);
            assert_eq!(row["name"], class.name);
        }
    }

    #[test]
    fn list_audio_devices_matches_real_cpal_enumeration() {
        let expected_inputs = vocalis::audio_devices::list_input_device_names();
        let expected_outputs = vocalis::audio_devices::list_output_device_names();

        let response = invoke("list_audio_devices", serde_json::Value::Null).expect("command should succeed");
        assert_eq!(response["inputDevices"], serde_json::json!(expected_inputs));
        assert_eq!(response["outputDevices"], serde_json::json!(expected_outputs));
    }

    #[test]
    fn capture_screen_preview_returns_a_real_jpeg() {
        let response = invoke("capture_screen_preview", serde_json::Value::Null).expect("command should succeed");
        let bytes: Vec<u8> = serde_json::from_value(response).expect("expected a byte array");
        assert!(bytes.len() > 100, "a real captured/encoded frame should be more than a few bytes");
        assert_eq!(&bytes[0..2], &[0xFF, 0xD8], "should start with the JPEG magic bytes");
    }

    #[test]
    fn discover_teachers_completes_a_real_udp_listen_within_its_timeout() {
        // No teacher is broadcasting in this test run, so an empty list is the
        // correct answer — the point is that a real socket bind + timed listen
        // actually completes and round-trips through IPC, not that it finds
        // anything.
        let response = invoke("discover_teachers", serde_json::json!({"timeoutMs": 200})).expect("command should succeed");
        assert!(response.as_array().is_some(), "expected a JSON array (possibly empty)");
    }

    /// `start_teacher_session` inserts a real row into `db::open()`'s database,
    /// same as `list_classes_returns_real_db_rows` reads one — but that one is
    /// read-only, and this one writes, so it must not land in the developer's
    /// real `~/.local/share/Vocalis` the way the read-only tests deliberately
    /// do. `HOME` is process-global, so this test — and only this one — needs
    /// `--test-threads=1` (see `tauri-windows-build.yml`) to not race whichever
    /// other test happens to call `db::open()` at the same moment.
    #[test]
    fn start_teacher_session_creates_a_real_class_and_pin() {
        let scratch_home = std::env::temp_dir().join(format!("vocalis_tauri_session_test_{}", std::process::id()));
        std::fs::create_dir_all(&scratch_home).unwrap();
        std::env::set_var("HOME", &scratch_home);
        // Windows reads `%APPDATA%`, not `HOME` — see `db::db_path`'s doc
        // comment — so both need overriding for this test to actually land in
        // the scratch dir on every platform this CI matrix runs.
        std::env::set_var("APPDATA", &scratch_home);

        // Built once and reused for both calls below (unlike the plain
        // `invoke` helper, which builds a fresh app — and so a fresh,
        // independent `TeacherSessionState` — every time): idempotency is a
        // property of *one* running app seeing two calls, e.g. a React
        // effect double-invoked under StrictMode, not of two unrelated apps.
        let app = super::build_app(tauri::test::mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let call = |cmd: &str, args: serde_json::Value| -> Result<serde_json::Value, serde_json::Value> {
            let body = match args {
                serde_json::Value::Null => InvokeBody::default(),
                other => InvokeBody::Json(other),
            };
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) { "http://tauri.localhost" } else { "tauri://localhost" }
                    .parse()
                    .unwrap(),
                body,
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            tauri::test::get_ipc_response(&webview, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
        };

        let response = call("start_teacher_session", serde_json::json!({"className": "Тестовый класс"}))
            .expect("start_teacher_session should succeed");
        let pin = response["pin"].as_str().expect("expected a pin string");
        assert_eq!(pin.len(), 6, "generate_pin() always produces a 6-digit string");
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(response["className"], "Тестовый класс");

        // Idempotent: a second call on the *same* running app while a session
        // is already active returns that session's existing info rather than
        // erroring or trying to bind the control port a second time.
        let second = call("start_teacher_session", serde_json::json!({"className": "Другое имя"})).expect("should succeed");
        assert_eq!(second["pin"], response["pin"]);

        call("stop_teacher_session", serde_json::Value::Null).expect("stop_teacher_session should succeed");

        let real_class_exists = {
            let conn = vocalis::teacher::db::open().expect("open the scratch db");
            vocalis::teacher::db::list_classes(&conn)
                .unwrap()
                .iter()
                .any(|c| c.name == "Тестовый класс")
        };
        assert!(real_class_exists, "the command should have inserted a real row via db::insert_class, not a stub");

        std::fs::remove_dir_all(&scratch_home).ok();
    }

    /// CI runners (especially Windows ones) may have no real input device at
    /// all — that's a legitimate environment fact, not a bug, so this doesn't
    /// assert success. What it does assert: the command never panics, and
    /// `stop` is always safe to call, including when `start` failed or was
    /// never called.
    #[test]
    fn student_mic_meter_start_stop_does_not_panic() {
        let _ = invoke("start_student_mic_meter", serde_json::Value::Null);
        invoke("stop_student_mic_meter", serde_json::Value::Null).expect("stop should always succeed");
        invoke("stop_student_mic_meter", serde_json::Value::Null).expect("stop should be idempotent");
    }

    /// Uses `app.listen` (via `tauri::Listener`) to capture a real emitted
    /// event payload directly, the same technique step 7 part A's manual E2E
    /// test used — no real webview/JS needed to check the plumbing actually
    /// carries real values. A CI runner always has *some* primary monitor to
    /// capture (headless or not), so unlike the mic meter this asserts success.
    #[test]
    fn start_screen_demo_emits_a_real_decoded_jpeg_frame() {
        use std::sync::mpsc;
        use tauri::Listener;

        let app = super::build_app(tauri::test::mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let call = |cmd: &str| -> Result<serde_json::Value, serde_json::Value> {
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(any(windows, target_os = "android")) { "http://tauri.localhost" } else { "tauri://localhost" }
                    .parse()
                    .unwrap(),
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            };
            tauri::test::get_ipc_response(&webview, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
        };

        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        app.listen("screen-demo-frame", move |event| {
            if let Ok(payload) = serde_json::from_str(event.payload()) {
                let _ = tx.send(payload);
            }
        });

        call("start_screen_demo").expect("start_screen_demo should succeed on a CI runner's real primary monitor");

        let frame = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("a real frame should arrive within 10s");
        let data_url = frame["dataUrl"].as_str().expect("expected a dataUrl string");
        assert!(data_url.starts_with("data:image/jpeg;base64,"), "should be a real JPEG data URL, got: {data_url:.60}");
        let b64 = data_url.strip_prefix("data:image/jpeg;base64,").unwrap();
        let jpeg_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).expect("should be valid base64");
        assert_eq!(&jpeg_bytes[0..2], &[0xFF, 0xD8], "decoded payload should start with the JPEG magic bytes");
        assert!(frame["width"].as_u64().unwrap() > 0 && frame["height"].as_u64().unwrap() > 0);

        call("stop_screen_demo").expect("stop_screen_demo should succeed");
    }

    /// The real end-to-end scenario for step 7 part B's network broadcast:
    /// two independent `App`s (one hosting a real teacher session, one a
    /// real connecting student), talking over real localhost TCP/UDP
    /// sockets — same "two real, independently-driven sides" spirit as part
    /// A's manual E2E test (that one used two separate tokio runtimes; here
    /// each side's whole Tauri app, including its own `tauri::async_runtime`
    /// background tasks, stands in for that). Calls the command functions
    /// directly rather than through `get_ipc_response` — this test is about
    /// the real session/network plumbing, not re-proving IPC dispatch (which
    /// the other tests in this file already cover) — so no webview is built
    /// for either side.
    ///
    /// Requires a real screen to capture, like `start_screen_demo_emits_a_
    /// real_decoded_jpeg_frame` above — same CI assumption, not `#[ignore]`.
    #[test]
    fn teacher_own_screen_demo_reaches_a_real_connected_student_as_decoded_frames() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use tauri::{Listener, Manager};
        use crate::commands::{student_session, teacher_session};

        let teacher_app = super::build_app(tauri::test::mock_builder());
        let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
        let session_info = teacher_session::start_teacher_session(
            teacher_app.handle().clone(),
            teacher_state.clone(),
            "E2E класс".to_string(),
        )
        .expect("start_teacher_session should succeed");

        let student_app = super::build_app(tauri::test::mock_builder());
        let student_state = student_app.state::<student_session::StudentSessionState>();

        let (frame_tx, frame_rx) = mpsc::channel::<Instant>();
        student_app.listen("screen-demo-frame", move |_event| {
            let _ = frame_tx.send(Instant::now());
        });

        let connected = student_session::connect_student_session(
            student_app.handle().clone(),
            student_state.clone(),
            "127.0.0.1".to_string(),
            lingua_common::CONTROL_PORT,
            "E2E ученик".to_string(),
            session_info.pin.clone(),
        )
        .expect("connect_student_session should succeed against a real running control server");
        assert_eq!(connected.teacher_name, "Tauri (тест)", "should be the real teacher_name run_control_server was given");

        // Retry rather than a fixed sleep: the student's Hello/Welcome
        // handshake and this thread's own polling race independently, so
        // `start_own_screen_demo` may legitimately see an empty roster on
        // its first attempt or two even though `connect_student_session`
        // already returned (that only waits for *this* student's handshake,
        // not for the teacher's roster update to have landed).
        let demo_deadline = Instant::now() + Duration::from_secs(5);
        let demo_started_at;
        let demo_info = loop {
            match teacher_session::start_own_screen_demo(teacher_state.clone()) {
                Ok(info) => {
                    demo_started_at = Instant::now();
                    break info;
                }
                Err(e) if Instant::now() < demo_deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                    let _ = e;
                }
                Err(e) => panic!("start_own_screen_demo never succeeded: {e}"),
            }
        };
        assert_eq!(demo_info.target_count, 1, "exactly the one real connected student");

        // Collect real frames. The window is generous (15s) because this
        // whole pipeline (capture, H.264 encode, decode, JPEG re-encode) runs
        // *unoptimized* under `cargo test` — measured locally in `--release`,
        // the same pipeline sustains ~14-15fps (see the roadmap report), but
        // debug-mode CPU cost alone made 3-frames-in-6s flaky in practice
        // (confirmed: failed 2 of 3 local debug runs at that threshold). The
        // assertion below only checks *correctness* (a real frame arrives at
        // all) — that's what CI can reliably verify without `--release`;
        // throughput/fps numbers are reported for information only.
        let collect_until = Instant::now() + Duration::from_secs(15);
        let mut frame_times = Vec::new();
        while Instant::now() < collect_until {
            if let Ok(t) = frame_rx.recv_timeout(Duration::from_millis(200)) {
                frame_times.push(t);
            }
        }

        teacher_session::stop_own_screen_demo(teacher_state.clone());
        student_session::disconnect_student_session(student_state);
        teacher_session::stop_teacher_session(teacher_state);

        assert!(!frame_times.is_empty(), "expected at least one real decoded frame to reach the student within 15s");
        let first_frame_latency = frame_times[0].saturating_duration_since(demo_started_at);
        if frame_times.len() >= 2 {
            let span = frame_times.last().unwrap().saturating_duration_since(frame_times[0]);
            let achieved_fps = (frame_times.len() - 1) as f64 / span.as_secs_f64();
            println!(
                "[e2e] real teacher->student screen demo: {} frames in {:.2}s (~{:.1} fps achieved), first frame after {:.0}ms",
                frame_times.len(),
                span.as_secs_f64(),
                achieved_fps,
                first_frame_latency.as_secs_f64() * 1000.0,
            );
        } else {
            println!(
                "[e2e] real teacher->student screen demo: only 1 frame arrived in the collection window (expected under an \
                 unoptimized debug build — see `../../video-bench/`'s report for release-mode numbers), first frame after {:.0}ms",
                first_frame_latency.as_secs_f64() * 1000.0,
            );
        }
    }

    /// Real end-to-end scenario for step 7.5's mic broadcast: two independent
    /// `App`s again (same shape as the screen-demo E2E test above), this time
    /// verifying real captured audio — resampled, Opus-encoded, UDP-sent,
    /// decrypted, decoded, resampled again — lands in the student's real
    /// output mix queue. There's no webview event to observe here (mixing/
    /// playback happens entirely in `student::audio`, not something routed
    /// through IPC — see `StudentSession.mix`'s doc comment), so this reads
    /// `mix.broadcast`'s length directly.
    ///
    /// Unlike the screen-demo tests, this does NOT assert success: a CI
    /// runner may genuinely have no default input device at all (same
    /// well-established fact `student_mic_meter_start_stop_does_not_panic`
    /// already works around) — `start_mic_broadcast` failing for that reason
    /// is a real environment limitation, not a bug, so the test just reports
    /// it and returns early instead of failing.
    #[test]
    fn teacher_mic_broadcast_reaches_a_real_connected_student() {
        use std::time::{Duration, Instant};
        use tauri::Manager;
        use crate::commands::{student_session, teacher_session};

        let teacher_app = super::build_app(tauri::test::mock_builder());
        let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
        let session_info =
            teacher_session::start_teacher_session(teacher_app.handle().clone(), teacher_state.clone(), "E2E класс (аудио)".to_string())
                .expect("start_teacher_session should succeed");

        let student_app = super::build_app(tauri::test::mock_builder());
        let student_state = student_app.state::<student_session::StudentSessionState>();
        let connected = student_session::connect_student_session(
            student_app.handle().clone(),
            student_state.clone(),
            "127.0.0.1".to_string(),
            lingua_common::CONTROL_PORT,
            "E2E ученик (аудио)".to_string(),
            session_info.pin.clone(),
        )
        .expect("connect_student_session should succeed against a real running control server");
        assert_eq!(connected.teacher_name, "Tauri (тест)");

        if let Err(e) = teacher_session::start_mic_broadcast(teacher_state.clone()) {
            println!("[e2e] skipping mic-broadcast verification: no real input device on this runner ({e})");
            teacher_session::stop_teacher_session(teacher_state);
            student_session::disconnect_student_session(student_state);
            return;
        }

        // Give real audio real time to flow through the whole chain (capture
        // -> resample -> Opus encode -> UDP -> decrypt -> decode -> resample
        // -> mixed into the student's output queue) before checking.
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut queued_samples = 0usize;
        while Instant::now() < deadline {
            queued_samples = student_state.0.lock().unwrap().as_ref().map(|s| s.mix.lock().unwrap().broadcast.len()).unwrap_or(0);
            if queued_samples > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        teacher_session::stop_mic_broadcast(teacher_state.clone());
        teacher_session::stop_teacher_session(teacher_state);
        student_session::disconnect_student_session(student_state);

        assert!(queued_samples > 0, "expected real decoded audio samples to reach the student's mix queue within 8s");
        println!("[e2e] real teacher->student mic broadcast: {queued_samples} samples queued for playback");
    }

    /// Real end-to-end scenario for step 7.5's listen-in: the teacher picks
    /// the (real) connected student out of a real `student-levels` event
    /// (same technique as reading any other real event in this file — no
    /// need to reach into `TeacherSession`'s private `app_state` just to
    /// find a student id), tells them to start uploading via
    /// `start_listen`, and verifies real captured/Opus-encoded/decoded audio
    /// lands in the teacher's own real listen-in queue.
    ///
    /// Depends on the *student's* mic this time (not the teacher's, unlike
    /// the broadcast test) — `connect_student_session` already treats a
    /// missing input device as non-fatal (logs and continues), so this
    /// checks `StudentSession.outbound_mic` directly to tell "no real mic on
    /// this runner" apart from "the pipeline is actually broken" before
    /// asserting anything.
    #[test]
    fn teacher_listens_in_on_a_real_connected_student() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use tauri::{Listener, Manager};
        use crate::commands::{student_session, teacher_session};

        let teacher_app = super::build_app(tauri::test::mock_builder());
        let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
        let session_info = teacher_session::start_teacher_session(
            teacher_app.handle().clone(),
            teacher_state.clone(),
            "E2E класс (прослушка)".to_string(),
        )
        .expect("start_teacher_session should succeed");

        let (id_tx, id_rx) = mpsc::channel::<String>();
        teacher_app.listen("student-levels", move |event| {
            if let Ok(levels) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                if let Some(id) = levels.as_array().and_then(|arr| arr.first()).and_then(|s| s["id"].as_str()) {
                    let _ = id_tx.send(id.to_string());
                }
            }
        });

        let student_app = super::build_app(tauri::test::mock_builder());
        let student_state = student_app.state::<student_session::StudentSessionState>();
        let connected = student_session::connect_student_session(
            student_app.handle().clone(),
            student_state.clone(),
            "127.0.0.1".to_string(),
            lingua_common::CONTROL_PORT,
            "E2E ученик (прослушка)".to_string(),
            session_info.pin.clone(),
        )
        .expect("connect_student_session should succeed against a real running control server");
        assert_eq!(connected.teacher_name, "Tauri (тест)");

        let has_mic = student_state.0.lock().unwrap().as_ref().map(|s| s.outbound_mic.is_some()).unwrap_or(false);
        if !has_mic {
            println!("[e2e] skipping listen-in verification: no real input device on this runner (student side)");
            teacher_session::stop_teacher_session(teacher_state);
            student_session::disconnect_student_session(student_state);
            return;
        }

        let student_id = id_rx.recv_timeout(Duration::from_secs(5)).expect("a real student-levels event naming this student should arrive");

        teacher_session::start_listen(teacher_state.clone(), student_id).expect("start_listen should succeed for a real connected student");

        // Give real audio real time to flow (capture -> resample -> Opus
        // encode -> UDP -> decrypt -> decode -> resample -> queued for
        // playback) before checking.
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut queued_samples = 0usize;
        while Instant::now() < deadline {
            queued_samples = teacher_state.0.lock().unwrap().as_ref().map(|s| s.listen_queue.lock().unwrap().len()).unwrap_or(0);
            if queued_samples > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        teacher_session::stop_listen(teacher_state.clone());
        teacher_session::stop_teacher_session(teacher_state);
        student_session::disconnect_student_session(student_state);

        assert!(queued_samples > 0, "expected real decoded listen-in audio to reach the teacher's queue within 8s");
        println!("[e2e] real listen-in: {queued_samples} samples queued for the teacher's playback");
    }

    /// Real end-to-end scenario for step 7.5's private intercom: verifies
    /// both legs independently — the teacher's own voice reaching the
    /// student (`mix.intercom`, needs the *teacher's* mic, which
    /// `start_intercom` itself requires to succeed at all) and the
    /// student's voice reaching the teacher (`listen_queue`, needs the
    /// *student's* mic — checked via `outbound_mic` before asserting, same
    /// as the plain listen-in test, since a CI runner might have one real
    /// input device but not two independently-addressable ones).
    #[test]
    fn teacher_and_student_hear_each_other_over_a_real_intercom() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use tauri::{Listener, Manager};
        use crate::commands::{student_session, teacher_session};

        let teacher_app = super::build_app(tauri::test::mock_builder());
        let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
        let session_info = teacher_session::start_teacher_session(
            teacher_app.handle().clone(),
            teacher_state.clone(),
            "E2E класс (интерком)".to_string(),
        )
        .expect("start_teacher_session should succeed");

        let (id_tx, id_rx) = mpsc::channel::<String>();
        teacher_app.listen("student-levels", move |event| {
            if let Ok(levels) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                if let Some(id) = levels.as_array().and_then(|arr| arr.first()).and_then(|s| s["id"].as_str()) {
                    let _ = id_tx.send(id.to_string());
                }
            }
        });

        let student_app = super::build_app(tauri::test::mock_builder());
        let student_state = student_app.state::<student_session::StudentSessionState>();
        let connected = student_session::connect_student_session(
            student_app.handle().clone(),
            student_state.clone(),
            "127.0.0.1".to_string(),
            lingua_common::CONTROL_PORT,
            "E2E ученик (интерком)".to_string(),
            session_info.pin.clone(),
        )
        .expect("connect_student_session should succeed against a real running control server");
        assert_eq!(connected.teacher_name, "Tauri (тест)");

        let student_id = id_rx.recv_timeout(Duration::from_secs(5)).expect("a real student-levels event naming this student should arrive");

        let intercom_info = match teacher_session::start_intercom(teacher_state.clone(), student_id) {
            Ok(info) => info,
            Err(e) => {
                println!("[e2e] skipping intercom verification: no real input device on this runner (teacher side) ({e})");
                teacher_session::stop_teacher_session(teacher_state);
                student_session::disconnect_student_session(student_state);
                return;
            }
        };
        assert_eq!(intercom_info.student_name, "E2E ученик (интерком)");

        let student_has_mic = student_state.0.lock().unwrap().as_ref().map(|s| s.outbound_mic.is_some()).unwrap_or(false);

        let deadline = Instant::now() + Duration::from_secs(8);
        let mut teacher_hears_student = 0usize;
        let mut student_hears_teacher = 0usize;
        while Instant::now() < deadline {
            student_hears_teacher = student_state.0.lock().unwrap().as_ref().map(|s| s.mix.lock().unwrap().intercom.len()).unwrap_or(0);
            if student_has_mic {
                teacher_hears_student = teacher_state.0.lock().unwrap().as_ref().map(|s| s.listen_queue.lock().unwrap().len()).unwrap_or(0);
            }
            let done = student_hears_teacher > 0 && (!student_has_mic || teacher_hears_student > 0);
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        teacher_session::stop_intercom(teacher_state.clone());
        teacher_session::stop_teacher_session(teacher_state);
        student_session::disconnect_student_session(student_state);

        assert!(student_hears_teacher > 0, "expected the student to receive real decoded intercom audio from the teacher within 8s");
        println!("[e2e] real intercom, teacher -> student: {student_hears_teacher} samples queued for the student's playback");
        if student_has_mic {
            assert!(teacher_hears_student > 0, "expected the teacher to receive real decoded intercom audio from the student within 8s");
            println!("[e2e] real intercom, student -> teacher: {teacher_hears_student} samples queued for the teacher's playback");
        } else {
            println!("[e2e] skipping student -> teacher leg: no real input device on this runner (student side)");
        }
    }
}
