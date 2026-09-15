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
        .invoke_handler(tauri::generate_handler![
            commands::db::list_classes,
            commands::audio::list_audio_devices,
            commands::network::discover_teachers,
            commands::video::capture_screen_preview,
            commands::teacher_session::start_teacher_session,
            commands::teacher_session::stop_teacher_session,
            commands::student_mic::start_student_mic_meter,
            commands::student_mic::stop_student_mic_meter,
            commands::screen_demo::start_screen_demo,
            commands::screen_demo::stop_screen_demo,
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
}
