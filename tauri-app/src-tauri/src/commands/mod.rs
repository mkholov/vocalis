//! Tauri commands — the bridge to the existing Rust core (`vocalis`,
//! `lingua-common`). Step 2 of the Tauri migration (see
//! `vocalis_roadmap.md`, section 8): backend only, no screen calls any of
//! this yet — exercise each command from the webview's dev console instead,
//! e.g. `await window.__TAURI__.core.invoke("list_classes")`.

pub mod audio;
pub mod db;
pub mod network;
pub mod video;
