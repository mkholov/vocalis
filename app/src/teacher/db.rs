//! Local SQLite persistence for lesson history — students, grades, and assignment
//! completion — so it survives a restart of the teacher console. Plain synchronous
//! `rusqlite`: no migrations, no ORM, just idempotent `CREATE TABLE IF NOT EXISTS`
//! and a handful of hand-written queries. Every call site already holds the same
//! `std::sync::Mutex` that guards `SharedState`, so a single `Connection` (not
//! `Sync`, but `Send`) living inside that state is enough — no separate lock needed.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use lingua_common::AssignmentKind;
use rusqlite::{params, Connection};

/// Opens (creating if needed) `~/.local/share/Vocalis/vocalis.sqlite3` on
/// macOS/Linux, or `%APPDATA%\Vocalis\vocalis.sqlite3` on Windows, and ensures the
/// schema exists.
pub fn open() -> Result<Connection> {
    let path = db_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("creating Vocalis data directory")?;
    }
    let conn = Connection::open(&path).context("opening Vocalis database")?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS lessons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            class_name TEXT NOT NULL,
            started_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS students (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            lesson_id INTEGER NOT NULL REFERENCES lessons(id),
            name TEXT NOT NULL,
            seat INTEGER NOT NULL,
            score INTEGER,
            connected_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS assignments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            student_id INTEGER NOT NULL REFERENCES students(id),
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS materials (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            file_path TEXT NOT NULL,
            added_at INTEGER NOT NULL
        );
        ",
    )
    .context("creating Vocalis tables")?;
    Ok(conn)
}

fn db_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
    }
    .unwrap_or_else(std::env::temp_dir);
    base.join("Vocalis").join("vocalis.sqlite3")
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn kind_label(kind: AssignmentKind) -> &'static str {
    match kind {
        AssignmentKind::Listening => "listening",
        AssignmentKind::Test => "test",
        AssignmentKind::Dialogue => "dialogue",
        AssignmentKind::Pronunciation => "pronunciation",
    }
}

/// A snapshot of history from *before* the current lesson, loaded once at startup
/// so the teacher can see it never actually went away between restarts.
#[derive(Default, Clone, Copy)]
pub struct HistorySummary {
    pub lessons_count: i64,
    pub avg_score: Option<f32>,
    pub assignments_done: i64,
}

pub fn load_history_summary(conn: &Connection) -> Result<HistorySummary> {
    let lessons_count = conn.query_row("SELECT COUNT(*) FROM lessons", [], |r| r.get(0))?;
    let avg_score: Option<f64> = conn.query_row(
        "SELECT AVG(score) FROM students WHERE score IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let assignments_done = conn.query_row(
        "SELECT COUNT(*) FROM assignments WHERE done = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(HistorySummary {
        lessons_count,
        avg_score: avg_score.map(|v| v as f32),
        assignments_done,
    })
}

/// Records the start of a new lesson (one row per teacher app launch). Returns the
/// row id, used to tie every student/score/assignment recorded this session back to it.
pub fn insert_lesson(conn: &Connection, class_name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO lessons (class_name, started_at) VALUES (?1, ?2)",
        params![class_name, now_epoch()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Records a student joining the current lesson (this *is* the attendance record —
/// its mere existence for a given lesson/name/seat is the "present" signal).
pub fn insert_student(conn: &Connection, lesson_id: i64, name: &str, seat: usize) -> Result<i64> {
    conn.execute(
        "INSERT INTO students (lesson_id, name, seat, connected_at) VALUES (?1, ?2, ?3, ?4)",
        params![lesson_id, name, seat as i64, now_epoch()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_score(conn: &Connection, student_row_id: i64, score: u32) -> Result<()> {
    conn.execute(
        "UPDATE students SET score = ?1 WHERE id = ?2",
        params![score, student_row_id],
    )?;
    Ok(())
}

pub fn insert_assignment(
    conn: &Connection,
    student_row_id: i64,
    title: &str,
    kind: AssignmentKind,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO assignments (student_id, title, kind, done) VALUES (?1, ?2, ?3, 0)",
        params![student_row_id, title, kind_label(kind)],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn mark_assignment_done(conn: &Connection, assignment_row_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE assignments SET done = 1 WHERE id = ?1",
        params![assignment_row_id],
    )?;
    Ok(())
}

/// One row of the audio materials library — the file itself stays wherever the
/// teacher originally picked it from (just like the existing "send file" flow);
/// only its title and path are persisted, matching what was asked for.
pub struct MaterialRow {
    pub id: i64,
    pub title: String,
    pub file_path: String,
}

pub fn insert_material(conn: &Connection, title: &str, file_path: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO materials (title, file_path, added_at) VALUES (?1, ?2, ?3)",
        params![title, file_path, now_epoch()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_materials(conn: &Connection) -> Result<Vec<MaterialRow>> {
    let mut stmt = conn.prepare("SELECT id, title, file_path FROM materials ORDER BY added_at DESC")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(MaterialRow {
                id: row.get(0)?,
                title: row.get(1)?,
                file_path: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
