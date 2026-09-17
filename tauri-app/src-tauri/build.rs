fn main() {
    // Auto-generates `allow-<command>`/`deny-<command>` permissions for our own
    // (non-plugin) commands — Tauri v2's ACL denies everything by default, even
    // app-defined commands, so `capabilities/default.json` can reference these
    // identifiers. See `list_classes` etc. in `src/commands/`.
    let attributes = tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "list_classes",
            "list_audio_devices",
            "discover_teachers",
            "capture_screen_preview",
            "start_teacher_session",
            "stop_teacher_session",
            "start_student_mic_meter",
            "stop_student_mic_meter",
            "start_screen_demo",
            "stop_screen_demo",
            "start_own_screen_demo",
            "stop_own_screen_demo",
            "start_mic_broadcast",
            "stop_mic_broadcast",
            "start_listen",
            "stop_listen",
            "start_intercom",
            "stop_intercom",
            "create_group",
            "leave_group",
            "connect_student_session",
            "disconnect_student_session",
        ]),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build codegen");
}
