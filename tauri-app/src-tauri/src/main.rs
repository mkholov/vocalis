// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // "Режим кабинета" (`device_role.rs`): a real `--set-role=...`/`--get-role` CLI mode, checked *before*
    // Tauri or any window starts — IT scripts this against 15 machines, or a reference image before
    // cloning it, without ever needing the GUI to come up at all.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit_code) = tauri_app_lib::device_role::handle_cli(&args) {
        std::process::exit(exit_code);
    }
    tauri_app_lib::run()
}
