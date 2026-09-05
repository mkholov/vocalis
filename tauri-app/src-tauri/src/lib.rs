// Minimal skeleton on purpose — no commands, no business logic yet. Just the
// window shell; see ../../README or the repo root for what this crate is for.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
