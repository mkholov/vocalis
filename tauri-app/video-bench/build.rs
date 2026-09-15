fn main() {
    // Same reason as tauri-app/src-tauri's build.rs: Tauri v2 denies every
    // command by default, even our own — this generates the `allow-*`
    // identifiers `capabilities/default.json` references.
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["ack", "start_channel_bench", "debug"]));
    tauri_build::try_build(attributes).expect("failed to run tauri-build codegen");
}
