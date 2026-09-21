//! Step 2 of the Tauri migration (`vocalis_roadmap.md`, section 8): the
//! existing Rust core (`vocalis`, `lingua-common` — DB/network/audio/video,
//! none of it egui-specific) wired in as Tauri commands. Still no screen
//! calls any of this — see `commands` for how to exercise it from the
//! webview's dev console; it's exercised through the real IPC dispatch path (command-name lookup +
//! argument/return serialization) without needing a real window — see
//! `tests/command_bridge.rs` (an integration test, not an in-file `mod tests`,
//! for a Windows-specific reason explained there and in `build.rs`).

// `pub` (with `build_app` below) so `tests/command_bridge.rs` — an integration test,
// which is the only kind of test executable `build.rs` can attach a Windows
// manifest to — can reach the commands and the real `build_app` wiring.
pub mod commands;

/// Shared between the real entry point and the integration tests so both
/// register the exact same commands the same way — generic over `Runtime` so
/// tests can build this with Tauri's `MockRuntime` instead of a real window.
pub fn build_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::App<R> {
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::teacher_session::TeacherSessionState::default())
        .manage(commands::student_mic::MicMeterState::default())
        .manage(commands::screen_demo::ScreenDemoState::default())
        .manage(commands::student_session::StudentSessionState::default())
        .invoke_handler(tauri::generate_handler![
            commands::db::list_classes,
            commands::db::create_class,
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
            commands::teacher_session::create_group,
            commands::teacher_session::leave_group,
            commands::teacher_session::list_materials,
            commands::teacher_session::upload_material,
            commands::teacher_session::play_material,
            commands::teacher_session::stop_playback,
            commands::student_mic::start_student_mic_meter,
            commands::student_mic::stop_student_mic_meter,
            commands::screen_demo::start_screen_demo,
            commands::screen_demo::stop_screen_demo,
            commands::student_recording::start_recording,
            commands::student_recording::stop_recording,
            commands::student_recording::list_recordings,
            commands::student_recording::read_recording,
            commands::student_recording::delete_recording,
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
