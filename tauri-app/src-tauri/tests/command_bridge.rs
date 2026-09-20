//! Real-IPC / real-network integration tests for the Tauri command bridge.
//!
//! These live in `tests/` (not in a `#[cfg(test)] mod tests` inside `src/lib.rs`) on
//! purpose: on Windows the test executable needs an application manifest requesting
//! Common Controls v6 (tao/wry/tauri-runtime-wry import `SetWindowSubclass`/
//! `DefSubclassProc`/`TaskDialogIndirect`, which the default v5 comctl32 doesn't
//! export by name), and `build.rs` can only attach one to *integration-test*
//! executables (`cargo:rustc-link-arg-tests`) — a lib's own unit-test executable
//! never receives `-tests` link args. Without it the exe dies at startup with
//! `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`. See `build.rs`.

//! Exercises each command through Tauri's actual IPC dispatch (command-name
//! routing + serde (de)serialization of args/return value) via
//! `tauri::test`'s `MockRuntime` — the same path the webview's
//! `invoke("list_classes")` goes through, just without a real window. Every
//! assertion here compares against the real, independently-obtained result
//! (e.g. calling `vocalis::audio_devices` directly) rather than a hardcoded
//! stub, so a command that silently stopped doing real work would fail.

use tauri::test::mock_builder;
use tauri_app_lib::build_app;
use tauri::webview::InvokeRequest;
use tauri::ipc::{CallbackFn, InvokeBody};

fn invoke(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
    let app = build_app(mock_builder());
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
    // Its own throwaway database, seeded here: that keeps this test off the developer's real one *and*
    // means it compares the command against rows that certainly exist (against a real database that
    // happened to be empty it would have passed without checking anything).
    let _db = ScratchDb::new("list_classes");
    let expected = {
        let conn = vocalis::teacher::db::open().expect("open the scratch db");
        vocalis::teacher::db::insert_class(&conn, "E2E класс (список A)").expect("insert a class");
        vocalis::teacher::db::insert_class(&conn, "E2E класс (список Б)").expect("insert another class");
        vocalis::teacher::db::list_classes(&conn).expect("query the classes table")
    };
    assert!(expected.len() >= 2, "the seeded classes should be there");
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

/// `start_teacher_session` inserts a real row into `db::open()`'s database, so it runs against a
/// throwaway one (`ScratchDb` — see there for how, and why it needs `--test-threads=1`).
#[test]
fn start_teacher_session_creates_a_real_class_and_pin() {
    let _db = ScratchDb::new("session");

    // Built once and reused for both calls below (unlike the plain
    // `invoke` helper, which builds a fresh app — and so a fresh,
    // independent `TeacherSessionState` — every time): idempotency is a
    // property of *one* running app seeing two calls, e.g. a React
    // effect double-invoked under StrictMode, not of two unrelated apps.
    let app = build_app(tauri::test::mock_builder());
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

    let app = build_app(tauri::test::mock_builder());
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
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `start_teacher_session` writes a class and a lesson (and the student's connection a row): keep that
    // out of the developer's real database.
    let _db = ScratchDb::new("screen_demo");

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info = teacher_session::start_teacher_session(
        teacher_app.handle().clone(),
        teacher_state.clone(),
        "E2E класс".to_string(),
    )
    .expect("start_teacher_session should succeed");

    let student_app = build_app(tauri::test::mock_builder());
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
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `start_teacher_session` writes a class and a lesson (and the student's connection a row): keep that
    // out of the developer's real database.
    let _db = ScratchDb::new("mic_broadcast");

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info =
        teacher_session::start_teacher_session(teacher_app.handle().clone(), teacher_state.clone(), "E2E класс (аудио)".to_string())
            .expect("start_teacher_session should succeed");

    let student_app = build_app(tauri::test::mock_builder());
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
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `start_teacher_session` writes a class and a lesson (and the student's connection a row): keep that
    // out of the developer's real database.
    let _db = ScratchDb::new("listen_in");

    let teacher_app = build_app(tauri::test::mock_builder());
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

    let student_app = build_app(tauri::test::mock_builder());
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
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `start_teacher_session` writes a class and a lesson (and the student's connection a row): keep that
    // out of the developer's real database.
    let _db = ScratchDb::new("intercom");

    let teacher_app = build_app(tauri::test::mock_builder());
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

    let student_app = build_app(tauri::test::mock_builder());
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

/// Real end-to-end scenario for step 7.5's groups/pairs: three real
/// independently-driven Tauri apps this time — one teacher, two
/// students — since grouping is inherently peer-to-peer, not
/// teacher-mediated. Verifies the protocol level: `create_group` really
/// sent a `ServerToClient::JoinGroup` to each student, and
/// `student::net::connect_to_teacher` (unchanged) really derived each
/// peer's session key from it — checked by reading `peer_addrs`/
/// `peer_keys` directly off each student's real `SharedState` (see
/// `StudentSession.app_state`'s doc comment for why that field is
/// reachable from here). Deliberately stops there rather than also
/// verifying real peer-to-peer *audio* — see the comment further down,
/// after `create_group` succeeds, for why that specifically can't be
/// simulated with two students on one test machine.
#[test]
fn creating_a_group_relays_real_peer_info_to_both_real_students() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tauri::{Listener, Manager};
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `start_teacher_session` writes a class and a lesson (and the student's connection a row): keep that
    // out of the developer's real database.
    let _db = ScratchDb::new("groups");

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info = teacher_session::start_teacher_session(
        teacher_app.handle().clone(),
        teacher_state.clone(),
        "E2E класс (группы)".to_string(),
    )
    .expect("start_teacher_session should succeed");

    let (ids_tx, ids_rx) = mpsc::channel::<Vec<String>>();
    teacher_app.listen("student-levels", move |event| {
        if let Ok(levels) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(arr) = levels.as_array() {
                if arr.len() >= 2 {
                    let ids: Vec<String> = arr.iter().filter_map(|s| s["id"].as_str().map(str::to_string)).collect();
                    let _ = ids_tx.send(ids);
                }
            }
        }
    });

    let student_a_app = build_app(tauri::test::mock_builder());
    let student_a_state = student_a_app.state::<student_session::StudentSessionState>();
    student_session::connect_student_session(
        student_a_app.handle().clone(),
        student_a_state.clone(),
        "127.0.0.1".to_string(),
        lingua_common::CONTROL_PORT,
        "E2E ученик A (группы)".to_string(),
        session_info.pin.clone(),
    )
    .expect("student A should connect to the real running control server");

    let student_b_app = build_app(tauri::test::mock_builder());
    let student_b_state = student_b_app.state::<student_session::StudentSessionState>();
    student_session::connect_student_session(
        student_b_app.handle().clone(),
        student_b_state.clone(),
        "127.0.0.1".to_string(),
        lingua_common::CONTROL_PORT,
        "E2E ученик B (группы)".to_string(),
        session_info.pin.clone(),
    )
    .expect("student B should connect to the real running control server");

    let ids = ids_rx.recv_timeout(Duration::from_secs(5)).expect("a real student-levels event naming both students should arrive");
    assert_eq!(ids.len(), 2, "expected exactly the two real connected students");

    let group_info = teacher_session::create_group(teacher_state.clone(), ids).expect("create_group should succeed for two real connected students");
    assert_eq!(group_info.member_count, 2);

    // Give the real JoinGroup control messages time to arrive and be
    // processed by each student's own `connect_to_teacher` message loop.
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut a_peers, mut b_peers) = (0, 0);
    while Instant::now() < deadline {
        a_peers = student_a_state.0.lock().unwrap().as_ref().map(|s| s.app_state.lock().unwrap().peer_addrs.len()).unwrap_or(0);
        b_peers = student_b_state.0.lock().unwrap().as_ref().map(|s| s.app_state.lock().unwrap().peer_addrs.len()).unwrap_or(0);
        if a_peers > 0 && b_peers > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(a_peers, 1, "student A should have real peer info for exactly one peer (student B)");
    assert_eq!(b_peers, 1, "student B should have real peer info for exactly one peer (student A)");
    println!("[e2e] real group: JoinGroup delivered to both students, each with 1 real peer");

    // Deliberately no audio-level verification here, unlike the other
    // step 7.5 tests: `run_outbound_and_group_audio` binds a *fixed*
    // `PEER_PORT` per student, which is fine in real use (each student
    // is a separate machine) but means two real students on *one* test
    // machine can't both bind it — confirmed: the second one's task
    // fails immediately with "Address already in use", logged but not
    // fatal (the rest of that student's session — receiving group audio,
    // teacher audio, etc. — keeps working, only *sending* group audio to
    // peers is unavailable). Simulating real two-way peer audio here
    // would need two actually separate machines, not two processes on
    // one — so this test stops at the protocol-level proof above, which
    // needs no such assumption.
    teacher_session::stop_teacher_session(teacher_state);
    student_session::disconnect_student_session(student_a_state);
    student_session::disconnect_student_session(student_b_state);
}

/// Points `vocalis::teacher::db::open()` (and `student::recording`'s Recordings folder) at a throwaway
/// directory for as long as the guard lives, then restores the environment and deletes it.
///
/// **Every test in this file that could write — anything that calls `start_teacher_session`, saves a
/// recording or uploads a material — must start with `let _db = ScratchDb::new("…");`**, otherwise it
/// leaves rows in the developer's real `~/.local/share/Vocalis` on every run (that is how 40+ junk
/// "E2E класс…" classes got there). Both paths resolve from `HOME` (unix) / `APPDATA` (Windows) on every
/// call, so overriding those is enough. Process-global env, so only sound because this file is run
/// with `--test-threads=1` (as `tauri-windows-build.yml` does) — the same constraint that already
/// applies to the fixed ports these tests bind.
struct ScratchDb {
    dir: std::path::PathBuf,
    prev_home: Option<std::ffi::OsString>,
    prev_appdata: Option<std::ffi::OsString>,
}

impl ScratchDb {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vocalis_tauri_scratch_db_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch db dir");
        let guard = ScratchDb { dir: dir.clone(), prev_home: std::env::var_os("HOME"), prev_appdata: std::env::var_os("APPDATA") };
        std::env::set_var("HOME", &dir);
        std::env::set_var("APPDATA", &dir);
        guard
    }
}

impl Drop for ScratchDb {
    fn drop(&mut self) {
        match &self.prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match &self.prev_appdata {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
        // A background task (e.g. the control server recording a student's disconnect) can still be
        // writing to the SQLite file — creating a `-journal` next to it — when this runs, which makes a
        // single `remove_dir_all` fail with "directory not empty" and leaves the tree behind. Retry
        // briefly; the connection is already bound to this directory, so none of that can reach the
        // real database, this is only about not leaving temp litter.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.dir).is_ok() || !self.dir.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// A minimal, valid mono 16-bit PCM WAV file — just enough for
/// `symphonia`'s probe (which `teacher::materials::decode_to_mono_pcm`
/// uses unchanged) to recognize and decode it. Unlike the mic/listen-in/
/// intercom E2E tests, this one needs no real microphone or speaker — playing
/// a library file is pure file-decode + network, so it's the one test
/// in this group that never has an environment-dependent skip path.
fn write_test_wav(path: &std::path::Path) {
    let sample_rate = 8_000u32;
    let samples: Vec<i16> = (0..sample_rate) // 1 second
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (0.2 * (2.0 * std::f32::consts::PI * 440.0 * t).sin() * i16::MAX as f32) as i16
        })
        .collect();
    let data_bytes = samples.len() * 2;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, buf).expect("write test WAV file");
}

/// Real end-to-end scenario for step 7.5's audio materials library: a
/// real WAV file, really decoded by `teacher::materials::
/// decode_to_mono_pcm`, really streamed over `MIC_PORT` by
/// `teacher::materials::run_playback` to a real connected student — who
/// receives it through the *same* always-on `run_mic_broadcast_receiver`
/// task already wired for the live mic broadcast (step 7.5's other
/// feature), needing no separate receive-side plumbing at all.
#[test]
fn playing_a_material_reaches_a_real_connected_student() {
    use std::time::{Duration, Instant};
    use tauri::Manager;
    use tauri_app_lib::commands::{student_session, teacher_session};

    // `upload_material` inserts a real library row (and `start_teacher_session` a class): keep both out of
    // the developer's real database.
    let _db = ScratchDb::new("materials");

    let wav_path = std::env::temp_dir().join(format!("vocalis_e2e_material_{}.wav", std::process::id()));
    write_test_wav(&wav_path);

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info = teacher_session::start_teacher_session(
        teacher_app.handle().clone(),
        teacher_state.clone(),
        "E2E класс (материалы)".to_string(),
    )
    .expect("start_teacher_session should succeed");

    let student_app = build_app(tauri::test::mock_builder());
    let student_state = student_app.state::<student_session::StudentSessionState>();
    student_session::connect_student_session(
        student_app.handle().clone(),
        student_state.clone(),
        "127.0.0.1".to_string(),
        lingua_common::CONTROL_PORT,
        "E2E ученик (материалы)".to_string(),
        session_info.pin.clone(),
    )
    .expect("connect_student_session should succeed against a real running control server");

    let material = teacher_session::upload_material(teacher_state.clone(), wav_path.to_string_lossy().to_string(), "E2E материал".to_string())
        .expect("upload_material should succeed with a real decodable WAV file");

    let listed = teacher_session::list_materials(teacher_state.clone()).expect("list_materials should succeed");
    assert!(listed.iter().any(|m| m.id == material.id && m.title == "E2E материал"));

    // Empty student_ids -> "everyone currently connected".
    let playback =
        teacher_session::play_material(teacher_state.clone(), material.id, Vec::new()).expect("play_material should succeed");
    assert_eq!(playback.target_count, 1, "the one real connected student");

    // Give the real decode -> resample -> Opus encode -> UDP -> decrypt
    // -> decode -> resample chain time to deliver real samples into the
    // student's mix (the same `mix.broadcast` queue the mic-broadcast
    // test reads, since materials playback reuses MIC_PORT).
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut queued_samples = 0usize;
    while Instant::now() < deadline {
        queued_samples = student_state.0.lock().unwrap().as_ref().map(|s| s.mix.lock().unwrap().broadcast.len()).unwrap_or(0);
        if queued_samples > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    teacher_session::stop_playback(teacher_state.clone());
    teacher_session::stop_teacher_session(teacher_state);
    student_session::disconnect_student_session(student_state);
    std::fs::remove_file(&wav_path).ok();

    assert!(queued_samples > 0, "expected real decoded material audio to reach the student's mix queue within 8s");
    println!("[e2e] real material playback: {queued_samples} samples queued for the student's playback");
}

/// Checks `bytes` really is a canonical mono 16-bit PCM WAV — the only shape `student::recording`
/// ever writes — and returns `(sample_rate, sample_count)`.
fn parse_wav_mono16(bytes: &[u8]) -> (u32, usize) {
    assert!(bytes.len() >= 44, "shorter than a WAV header: {} bytes", bytes.len());
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1, "mono");
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16, "16-bit");
    let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 44 + data_len, "the data chunk must account for every remaining byte");
    (sample_rate, data_len / 2)
}

/// Step 7.5 item 6 — everything about student recordings that does NOT need a microphone, on any
/// machine including a CI runner. The capture itself is real code we don't touch: the tap in
/// `student::audio::run_outbound_and_group_audio` appends the mic's PCM to `SharedState.recording`. Here
/// that PCM is a synthetic sine written straight into the same field, so the real save / list /
/// read-back / delete / disconnect-saves code runs against real files (in a throwaway HOME).
#[test]
fn student_recordings_save_list_play_back_and_delete() {
    use std::time::Duration;
    use tauri::Manager;
    use tauri_app_lib::commands::{student_recording, student_session, teacher_session};
    use vocalis::student::state::ActiveRecording;

    let _home = ScratchDb::new("recordings");

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info = teacher_session::start_teacher_session(
        teacher_app.handle().clone(),
        teacher_state.clone(),
        "E2E класс (записи)".to_string(),
    )
    .expect("start_teacher_session should succeed");

    let student_app = build_app(tauri::test::mock_builder());
    let student_state = student_app.state::<student_session::StudentSessionState>();
    student_session::connect_student_session(
        student_app.handle().clone(),
        student_state.clone(),
        "127.0.0.1".to_string(),
        lingua_common::CONTROL_PORT,
        "E2E ученик (записи)".to_string(),
        session_info.pin.clone(),
    )
    .expect("connect_student_session should succeed against a real running control server");

    let inject = |samples: Vec<i16>, sample_rate: u32| {
        let guard = student_state.0.lock().unwrap();
        guard.as_ref().unwrap().app_state.lock().unwrap().recording = Some(ActiveRecording { samples, sample_rate });
    };
    let sine = |secs: f32, rate: u32| -> Vec<i16> {
        (0..(rate as f32 * secs) as u32).map(|i| (0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * i16::MAX as f32) as i16).collect()
    };

    assert!(student_recording::list_recordings().is_empty(), "a throwaway HOME starts with no recordings");

    // save: 1.5s at 16 kHz.
    inject(sine(1.5, 16_000), 16_000);
    let saved = student_recording::stop_recording(student_state.clone())
        .expect("stop_recording should succeed")
        .expect("something was captured, so a recording should come back");
    assert!((saved.duration_secs - 1.5).abs() < 0.01, "duration should be samples/rate, got {}", saved.duration_secs);
    assert!(saved.name.starts_with("recording_") && saved.name.ends_with(".wav"), "unexpected name {}", saved.name);
    assert!(saved.recorded_at_epoch.is_some(), "the epoch in the file name should parse back out");

    // list: it's there.
    let listed = student_recording::list_recordings();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, saved.name);

    // play back: the exact WAV that was saved, as a data URL the webview's <audio> can play.
    let url = student_recording::read_recording(saved.name.clone()).expect("read_recording should succeed");
    let b64 = url.strip_prefix("data:audio/wav;base64,").expect("should be a WAV data URL");
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).expect("valid base64");
    let (rate, samples) = parse_wav_mono16(&bytes);
    assert_eq!((rate, samples), (16_000, 24_000), "the WAV should carry exactly what was captured");

    // The webview must not be able to reach outside the recordings directory by name.
    for evil in ["../../../etc/passwd", "..\\..\\Windows\\win.ini", "/etc/passwd", "recording_1.wav", ""] {
        assert!(student_recording::read_recording(evil.to_string()).is_err(), "read_recording({evil:?}) must be refused");
        assert!(student_recording::delete_recording(evil.to_string()).is_err(), "delete_recording({evil:?}) must be refused");
    }
    assert_eq!(student_recording::list_recordings().len(), 1, "refused deletes must not have removed anything");

    // stopping an empty recording leaves nothing behind (and stopping with none is a no-op).
    inject(Vec::new(), 16_000);
    assert!(student_recording::stop_recording(student_state.clone()).unwrap().is_none());
    assert!(student_recording::stop_recording(student_state.clone()).unwrap().is_none());
    assert_eq!(student_recording::list_recordings().len(), 1);

    // delete.
    student_recording::delete_recording(saved.name.clone()).expect("delete_recording should succeed");
    assert!(student_recording::list_recordings().is_empty());
    assert!(student_recording::read_recording(saved.name).is_err(), "a deleted recording is gone");

    // leaving mid-recording must not lose it: disconnect saves what's been captured so far.
    // (Sleep past the second boundary — `recording::save` names files by epoch seconds.)
    std::thread::sleep(Duration::from_millis(1100));
    inject(sine(0.5, 16_000), 16_000);
    student_session::disconnect_student_session(student_state);
    let after = student_recording::list_recordings();
    assert_eq!(after.len(), 1, "the in-progress recording should have been saved on disconnect");
    assert!((after[0].duration_secs - 0.5).abs() < 0.01);

    teacher_session::stop_teacher_session(teacher_state);
}

/// Step 7.5 item 6 with the real thing: the recording tap fed by this machine's actual microphone. Like
/// the other mic tests it can't assert success where there's no input device — `start_recording` says so
/// ("микрофон недоступен") and the test reports and returns — but wherever there is one, it records for
/// real and checks the WAV against wall-clock time.
#[test]
fn student_recording_captures_the_real_microphone() {
    use std::time::{Duration, Instant};
    use tauri::Manager;
    use tauri_app_lib::commands::{student_recording, student_session, teacher_session};

    let _home = ScratchDb::new("recordings_mic");

    let teacher_app = build_app(tauri::test::mock_builder());
    let teacher_state = teacher_app.state::<teacher_session::TeacherSessionState>();
    let session_info = teacher_session::start_teacher_session(
        teacher_app.handle().clone(),
        teacher_state.clone(),
        "E2E класс (запись микрофона)".to_string(),
    )
    .expect("start_teacher_session should succeed");

    let student_app = build_app(tauri::test::mock_builder());
    let student_state = student_app.state::<student_session::StudentSessionState>();
    student_session::connect_student_session(
        student_app.handle().clone(),
        student_state.clone(),
        "127.0.0.1".to_string(),
        lingua_common::CONTROL_PORT,
        "E2E ученик (запись микрофона)".to_string(),
        session_info.pin.clone(),
    )
    .expect("connect_student_session should succeed");

    if let Err(e) = student_recording::start_recording(student_state.clone()) {
        println!("[e2e] skipping real-microphone recording: {e}");
        student_session::disconnect_student_session(student_state);
        teacher_session::stop_teacher_session(teacher_state);
        return;
    }
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(1500));
    let saved = student_recording::stop_recording(student_state.clone())
        .expect("stop_recording should succeed")
        .expect("a real microphone should have produced samples in 1.5s");
    let wall = started.elapsed().as_secs_f32();

    let url = student_recording::read_recording(saved.name.clone()).expect("read_recording");
    let b64 = url.strip_prefix("data:audio/wav;base64,").unwrap();
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap();
    let (rate, samples) = parse_wav_mono16(&bytes);
    let recorded = samples as f32 / rate as f32;
    println!("[e2e] real microphone recording: {recorded:.2}s of audio at {rate} Hz for {wall:.2}s of wall time");

    student_session::disconnect_student_session(student_state);
    teacher_session::stop_teacher_session(teacher_state);

    assert!((recorded - saved.duration_secs).abs() < 0.01, "DTO and file disagree: {} vs {recorded}", saved.duration_secs);
    assert!(recorded > 1.0 && recorded < wall + 0.2, "recorded {recorded:.2}s over {wall:.2}s of wall time");
}
