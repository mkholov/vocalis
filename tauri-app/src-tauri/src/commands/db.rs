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

/// Creates a class — the same `db::insert_class` call the egui class picker makes
/// (`teacher::class_picker::try_create_class`), with the same trim / empty-name check and the same
/// Russian error texts. The `classes` table has only a name (no description or schedule), so that is
/// all a class is here too.
///
/// One difference from egui, on purpose: a name that is already taken is refused. The new UI
/// identifies the class to start a lesson for by its name (`start_teacher_session` reuses the class
/// with that name), so two classes with one name would be indistinguishable — and the second one
/// would never be reachable.
#[tauri::command]
pub fn create_class(name: String) -> Result<ClassDto, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Введите название класса".to_string());
    }
    let conn = db::open().map_err(|e| format!("Не удалось открыть базу данных: {e}"))?;
    let existing = db::list_classes(&conn).map_err(|e| format!("Не удалось создать класс: {e}"))?;
    if existing.iter().any(|c| c.name == name) {
        return Err(format!("Класс «{name}» уже есть"));
    }
    let id = db::insert_class(&conn, &name).map_err(|e| format!("Не удалось создать класс: {e}"))?;
    Ok(ClassDto { id, name })
}
