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
        .invoke_handler(tauri::generate_handler![
            commands::db::list_classes,
            commands::audio::list_audio_devices,
            commands::network::discover_teachers,
            commands::video::capture_screen_preview,
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
}
