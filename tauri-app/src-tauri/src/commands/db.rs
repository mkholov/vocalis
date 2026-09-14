//! Database-backed commands — thin wrappers around `vocalis::teacher::db`,
//! the same SQLite module the egui teacher console uses (same file on disk:
//! `~/.local/share/Vocalis/vocalis.sqlite3` / `%APPDATA%\Vocalis\...`, no
//! separate database for this prototype). No query logic lives here, only
//! the mapping from its plain structs to `Serialize`-able DTOs for IPC.

use serde::Serialize;
use vocalis::teacher::db;

#[derive(Serialize)]
pub struct ClassDto {
    pub id: i64,
    pub name: String,
}

/// Lists every class the teacher has ever created — real rows from the real
/// database, not a stub. Empty on a fresh install, not an error.
#[tauri::command]
pub fn list_classes() -> Result<Vec<ClassDto>, String> {
    let conn = db::open().map_err(|e| e.to_string())?;
    db::list_classes(&conn)
        .map(|rows| rows.into_iter().map(|c| ClassDto { id: c.id, name: c.name }).collect())
        .map_err(|e| e.to_string())
}
