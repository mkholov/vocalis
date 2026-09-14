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
        ]),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build codegen");
}
