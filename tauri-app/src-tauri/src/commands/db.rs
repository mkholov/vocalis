//! Database-backed commands — thin wrappers around `vocalis::teacher::db`,
//! the same SQLite module the egui teacher console uses (same file on disk:
//! `~/.local/share/Vocalis/vocalis.sqlite3` / `%APPDATA%\Vocalis\...`, no
//! separate database for this prototype). Listing and creating go through that
//! module's own functions. Renaming and deleting a class have no counterpart there
//! (the egui console never offered them, and `app/` is off limits for this
//! migration), so those two run their few statements directly on the same
//! connection `db::open()` hands out.

use serde::Serialize;
use tauri::State;
use vocalis::teacher::db;

use super::teacher_session::{active_class_name, TeacherSessionState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassDto {
    pub id: i64,
    pub name: String,
    /// Lessons held for this class so far — what deleting it would erase along with it.
    pub lessons: i64,
    /// Names on this class's roster.
    pub roster: i64,
}

fn class_dto(conn: &rusqlite::Connection, id: i64, name: String) -> Result<ClassDto, String> {
    let n = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [id], |r| r.get(0)).map_err(|e| e.to_string()) };
    Ok(ClassDto {
        id,
        name,
        lessons: n("SELECT COUNT(*) FROM lessons WHERE class_id = ?1")?,
        roster: n("SELECT COUNT(*) FROM roster WHERE class_id = ?1")?,
    })
}

/// Lists every class the teacher has ever created — real rows from the real
/// database, not a stub. Empty on a fresh install, not an error.
#[tauri::command]
pub fn list_classes() -> Result<Vec<ClassDto>, String> {
    let conn = db::open().map_err(|e| e.to_string())?;
    let rows = db::list_classes(&conn).map_err(|e| e.to_string())?;
    rows.into_iter().map(|c| class_dto(&conn, c.id, c.name)).collect()
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
    class_dto(&conn, id, name)
}

/// Renames a class. Same trim / empty checks and duplicate refusal as `create_class` (the new UI finds a
/// class by name, so names stay unique); renaming to the name it already has is a harmless no-op.
/// The lesson and roster rows keep a copy of the class name, so those follow the rename — otherwise the
/// old name would linger in the history.
#[tauri::command]
pub fn rename_class(session: State<TeacherSessionState>, id: i64, name: String) -> Result<ClassDto, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Введите название класса".to_string());
    }
    let conn = db::open().map_err(|e| format!("Не удалось открыть базу данных: {e}"))?;
    let classes = db::list_classes(&conn).map_err(|e| format!("Не удалось переименовать класс: {e}"))?;
    let current = classes.iter().find(|c| c.id == id).ok_or("Класс не найден — возможно, его уже удалили")?;
    if active_class_name(&session).as_deref() == Some(current.name.as_str()) {
        return Err("Урок этого класса сейчас идёт — переименовать можно после его завершения".to_string());
    }
    if classes.iter().any(|c| c.id != id && c.name == name) {
        return Err(format!("Класс «{name}» уже есть"));
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for sql in [
        "UPDATE classes SET name = ?2 WHERE id = ?1",
        "UPDATE lessons SET class_name = ?2 WHERE class_id = ?1",
        "UPDATE roster SET class_name = ?2 WHERE class_id = ?1",
    ] {
        tx.execute(sql, rusqlite::params![id, name]).map_err(|e| format!("Не удалось переименовать класс: {e}"))?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    class_dto(&conn, id, name)
}

/// Deletes a class together with everything that belongs to it: its lessons, the students/assignments/
/// results/connection log of those lessons, and its roster — all in one transaction, so a failure leaves
/// the class exactly as it was. Leaving them behind would strand rows pointing at a class that no longer
/// exists. Shared data (the materials library, assignment templates) is not class-bound and is untouched.
/// The UI asks for confirmation first and shows the counts (`ClassDto.lessons`/`roster`) it is about to erase.
#[tauri::command]
pub fn delete_class(session: State<TeacherSessionState>, id: i64) -> Result<(), String> {
    let conn = db::open().map_err(|e| format!("Не удалось открыть базу данных: {e}"))?;
    let classes = db::list_classes(&conn).map_err(|e| format!("Не удалось удалить класс: {e}"))?;
    let current = classes.iter().find(|c| c.id == id).ok_or("Класс не найден — возможно, его уже удалили")?;
    if active_class_name(&session).as_deref() == Some(current.name.as_str()) {
        return Err("Урок этого класса сейчас идёт — удалить можно после его завершения".to_string());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for sql in [
        "DELETE FROM test_results WHERE assignment_id IN (
             SELECT a.id FROM assignments a
             JOIN students s ON s.id = a.student_id
             JOIN lessons l ON l.id = s.lesson_id WHERE l.class_id = ?1)",
        "DELETE FROM assignments WHERE student_id IN (
             SELECT s.id FROM students s JOIN lessons l ON l.id = s.lesson_id WHERE l.class_id = ?1)",
        "DELETE FROM connection_log WHERE lesson_id IN (SELECT id FROM lessons WHERE class_id = ?1)",
        "DELETE FROM students WHERE lesson_id IN (SELECT id FROM lessons WHERE class_id = ?1)",
        "DELETE FROM lessons WHERE class_id = ?1",
        "DELETE FROM roster WHERE class_id = ?1",
        "DELETE FROM classes WHERE id = ?1",
    ] {
        tx.execute(sql, [id]).map_err(|e| format!("Не удалось удалить класс: {e}"))?;
    }
    tx.commit().map_err(|e| e.to_string())
}
