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
use rusqlite::{params, Connection, OptionalExtension};

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
        CREATE TABLE IF NOT EXISTS classes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS lessons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            class_name TEXT NOT NULL,
            class_id INTEGER REFERENCES classes(id),
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
        CREATE TABLE IF NOT EXISTS test_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            assignment_id INTEGER NOT NULL REFERENCES assignments(id),
            correct INTEGER NOT NULL,
            total INTEGER NOT NULL,
            submitted_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS assignment_templates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            reading_text TEXT,
            material_id INTEGER REFERENCES materials(id),
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS assignment_questions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            template_id INTEGER NOT NULL REFERENCES assignment_templates(id),
            position INTEGER NOT NULL,
            text TEXT NOT NULL,
            correct_index INTEGER
        );
        CREATE TABLE IF NOT EXISTS assignment_question_options (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            question_id INTEGER NOT NULL REFERENCES assignment_questions(id),
            position INTEGER NOT NULL,
            text TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS roster (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            class_name TEXT NOT NULL,
            class_id INTEGER REFERENCES classes(id),
            full_name TEXT NOT NULL,
            added_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS teacher_profile (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            name TEXT NOT NULL,
            salt TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS connection_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            lesson_id INTEGER NOT NULL REFERENCES lessons(id),
            at INTEGER NOT NULL,
            name_raw TEXT NOT NULL,
            name_normalized TEXT NOT NULL,
            ip TEXT NOT NULL,
            event TEXT NOT NULL
        );
        ",
    )
    .context("creating Vocalis tables")?;
    migrate_class_ids(&conn).context("migrating pre-multi-class data")?;
    Ok(conn)
}

/// Adds `class_id` to `lessons`/`roster` for databases created before multi-class
/// support existed, and buckets any pre-existing (now-orphaned) rows into one
/// default class — cheap and fully idempotent, so it's safe to just run on every
/// `open()` rather than tracking "did this already run" separately: once
/// migrated, both checks below are no-ops.
fn migrate_class_ids(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "lessons", "class_id")? {
        conn.execute("ALTER TABLE lessons ADD COLUMN class_id INTEGER REFERENCES classes(id)", [])?;
    }
    if !column_exists(conn, "roster", "class_id")? {
        conn.execute("ALTER TABLE roster ADD COLUMN class_id INTEGER REFERENCES classes(id)", [])?;
    }

    let has_orphans: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM lessons WHERE class_id IS NULL)
             OR EXISTS(SELECT 1 FROM roster WHERE class_id IS NULL)",
        [],
        |row| row.get(0),
    )?;
    if has_orphans {
        let default_class_id = insert_class(conn, "Класс 1")?;
        conn.execute("UPDATE lessons SET class_id = ?1 WHERE class_id IS NULL", params![default_class_id])?;
        conn.execute("UPDATE roster SET class_id = ?1 WHERE class_id IS NULL", params![default_class_id])?;
    }
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    // PRAGMA doesn't take bound parameters for identifiers; `table` is always one
    // of this module's own hardcoded literals, never external input.
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|n| n == column))
}

/// One class the teacher has set up (e.g. "9А английский") — its own roster and
/// its own lesson history, never mixed with another class's.
pub struct ClassRow {
    pub id: i64,
    pub name: String,
}

pub fn insert_class(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute("INSERT INTO classes (name, created_at) VALUES (?1, ?2)", params![name, now_epoch()])?;
    Ok(conn.last_insert_rowid())
}

pub fn list_classes(conn: &Connection) -> Result<Vec<ClassRow>> {
    let mut stmt = conn.prepare("SELECT id, name FROM classes ORDER BY created_at")?;
    let rows = stmt
        .query_map([], |row| Ok(ClassRow { id: row.get(0)?, name: row.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
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

fn kind_from_label(s: &str) -> AssignmentKind {
    match s {
        "listening" => AssignmentKind::Listening,
        "test" => AssignmentKind::Test,
        "pronunciation" => AssignmentKind::Pronunciation,
        _ => AssignmentKind::Dialogue,
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

/// Scoped to one class — with multiple classes now possible, an unscoped total
/// would blend unrelated classes' history into one misleading number.
pub fn load_history_summary(conn: &Connection, class_id: i64) -> Result<HistorySummary> {
    let lessons_count = conn.query_row(
        "SELECT COUNT(*) FROM lessons WHERE class_id = ?1",
        params![class_id],
        |r| r.get(0),
    )?;
    let avg_score: Option<f64> = conn.query_row(
        "SELECT AVG(s.score) FROM students s JOIN lessons l ON l.id = s.lesson_id
         WHERE s.score IS NOT NULL AND l.class_id = ?1",
        params![class_id],
        |r| r.get(0),
    )?;
    let assignments_done = conn.query_row(
        "SELECT COUNT(*) FROM assignments a
         JOIN students s ON s.id = a.student_id
         JOIN lessons l ON l.id = s.lesson_id
         WHERE a.done = 1 AND l.class_id = ?1",
        params![class_id],
        |r| r.get(0),
    )?;
    Ok(HistorySummary {
        lessons_count,
        avg_score: avg_score.map(|v| v as f32),
        assignments_done,
    })
}

/// Records the start of a new lesson (one row per teacher app launch, for the
/// class chosen on the class-picker screen). Returns the row id, used to tie
/// every student/score/assignment recorded this session back to it.
pub fn insert_lesson(conn: &Connection, class_id: i64, class_name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO lessons (class_name, class_id, started_at) VALUES (?1, ?2, ?3)",
        params![class_name, class_id, now_epoch()],
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

/// Records an auto-graded test result against the `assignments` row for that
/// student's copy of the assignment (that row's own `done` flag is set separately
/// via the existing `mark_assignment_done`).
pub fn insert_test_result(conn: &Connection, assignment_row_id: i64, correct: u32, total: u32) -> Result<()> {
    conn.execute(
        "INSERT INTO test_results (assignment_id, correct, total, submitted_at) VALUES (?1, ?2, ?3, ?4)",
        params![assignment_row_id, correct, total, now_epoch()],
    )?;
    Ok(())
}

/// One question of an authored assignment template. `options` is empty and
/// `correct_index` is `None` for a `Listening` prompt (just a text question, not
/// auto-graded); a `Test` question always has both.
pub struct TemplateQuestion {
    pub text: String,
    pub options: Vec<String>,
    pub correct_index: Option<usize>,
}

/// A reusable assignment blueprint the teacher authored — the "Задания" tab's
/// library, analogous to `MaterialRow` for the materials library. `kind` decides
/// which of `reading_text` / `material_id` / `questions` is actually populated.
pub struct AssignmentTemplate {
    pub id: i64,
    pub kind: AssignmentKind,
    pub title: String,
    pub reading_text: Option<String>,
    pub material_id: Option<i64>,
    pub questions: Vec<TemplateQuestion>,
}

/// A question being authored, before it has a row id — the input side of
/// `insert_assignment_template`, mirroring `TemplateQuestion` minus the id.
pub struct NewQuestion {
    pub text: String,
    pub options: Vec<String>,
    pub correct_index: Option<usize>,
}

/// Saves a new assignment template (and all its questions/options) in one
/// transaction. Takes `&mut Connection` (unlike everything else in this module)
/// because `rusqlite`'s transactions require exclusive access to the connection —
/// callers already hold `&mut SharedState`'s lock, so `&mut guard.db` is at hand.
pub fn insert_assignment_template(
    conn: &mut Connection,
    kind: AssignmentKind,
    title: &str,
    reading_text: Option<&str>,
    material_id: Option<i64>,
    questions: &[NewQuestion],
) -> Result<i64> {
    let tx = conn.transaction().context("starting transaction")?;
    tx.execute(
        "INSERT INTO assignment_templates (kind, title, reading_text, material_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind_label(kind), title, reading_text, material_id, now_epoch()],
    )?;
    let template_id = tx.last_insert_rowid();

    for (position, question) in questions.iter().enumerate() {
        tx.execute(
            "INSERT INTO assignment_questions (template_id, position, text, correct_index) VALUES (?1, ?2, ?3, ?4)",
            params![template_id, position as i64, question.text, question.correct_index.map(|v| v as i64)],
        )?;
        let question_id = tx.last_insert_rowid();
        for (opt_position, option_text) in question.options.iter().enumerate() {
            tx.execute(
                "INSERT INTO assignment_question_options (question_id, position, text) VALUES (?1, ?2, ?3)",
                params![question_id, opt_position as i64, option_text],
            )?;
        }
    }

    tx.commit().context("committing assignment template")?;
    Ok(template_id)
}

pub fn list_assignment_templates(conn: &Connection) -> Result<Vec<AssignmentTemplate>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, reading_text, material_id FROM assignment_templates ORDER BY created_at DESC",
    )?;
    let mut templates: Vec<AssignmentTemplate> = stmt
        .query_map([], |row| {
            let kind_str: String = row.get(1)?;
            Ok(AssignmentTemplate {
                id: row.get(0)?,
                kind: kind_from_label(&kind_str),
                title: row.get(2)?,
                reading_text: row.get(3)?,
                material_id: row.get(4)?,
                questions: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Small, local, rarely-loaded data (once at startup, or after authoring a new
    // template) — an N+1 query pattern here is simplicity, not a real cost.
    for template in &mut templates {
        let mut q_stmt = conn.prepare(
            "SELECT id, text, correct_index FROM assignment_questions WHERE template_id = ?1 ORDER BY position",
        )?;
        let question_rows: Vec<(i64, String, Option<i64>)> = q_stmt
            .query_map(params![template.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (question_id, text, correct_index) in question_rows {
            let mut o_stmt = conn
                .prepare("SELECT text FROM assignment_question_options WHERE question_id = ?1 ORDER BY position")?;
            let options: Vec<String> = o_stmt
                .query_map(params![question_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            template.questions.push(TemplateQuestion {
                text,
                options,
                correct_index: correct_index.map(|v| v as usize),
            });
        }
    }

    Ok(templates)
}

/// One student on the pre-set class roster (see `roster` table doc comment above)
/// — just a name to check connecting students against, not an account.
pub struct RosterEntry {
    pub id: i64,
    pub full_name: String,
}

/// Case/whitespace-insensitive comparison key — `"  Иванов Иван "` and
/// `"иванов иван"` are the same student for roster-matching purposes.
pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

pub fn insert_roster_student(conn: &Connection, class_id: i64, class_name: &str, full_name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO roster (class_name, class_id, full_name, added_at) VALUES (?1, ?2, ?3, ?4)",
        params![class_name, class_id, full_name, now_epoch()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_roster(conn: &Connection, class_id: i64) -> Result<Vec<RosterEntry>> {
    let mut stmt = conn.prepare("SELECT id, full_name FROM roster WHERE class_id = ?1 ORDER BY added_at")?;
    let rows = stmt
        .query_map(params![class_id], |row| {
            Ok(RosterEntry { id: row.get(0)?, full_name: row.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn rename_roster_student(conn: &Connection, roster_row_id: i64, full_name: &str) -> Result<()> {
    conn.execute(
        "UPDATE roster SET full_name = ?1 WHERE id = ?2",
        params![full_name, roster_row_id],
    )?;
    Ok(())
}

pub fn delete_roster_student(conn: &Connection, roster_row_id: i64) -> Result<()> {
    conn.execute("DELETE FROM roster WHERE id = ?1", params![roster_row_id])?;
    Ok(())
}

/// One test result within a `LessonHistoryEntry`.
pub struct TestResultEntry {
    pub title: String,
    pub correct: u32,
    pub total: u32,
}

/// One lesson a student (by normalized name) was present for — the row's mere
/// existence in the `students` table already *is* the attendance record, same as
/// everywhere else in this app; there's no separate present/absent flag.
pub struct LessonHistoryEntry {
    pub class_name: String,
    pub started_at: i64,
    pub score: Option<u32>,
    pub test_results: Vec<TestResultEntry>,
}

/// Every lesson a student attended *within one class*, most recent first, matched
/// by normalized name — the only identity a student has in this schema, since
/// there's no login/account system. Scoped to `class_id` so two different
/// students who happen to share a name in two different classes never get
/// blended together. Filtering by name happens in Rust rather than SQL (`WHERE
/// LOWER(name) = ...`) because SQLite's built-in `LOWER()` only folds ASCII and
/// would silently miss Cyrillic names — `normalize_name` (the same function the
/// roster check uses) handles that correctly. Fine to just scan the class's
/// `students` rows for this: it's a small local classroom database, not
/// something that needs a name index.
pub fn student_history(conn: &Connection, normalized_name: &str, class_id: i64) -> Result<Vec<LessonHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.score, l.class_name, l.started_at
         FROM students s
         JOIN lessons l ON l.id = s.lesson_id
         WHERE l.class_id = ?1
         ORDER BY l.started_at DESC",
    )?;
    let rows: Vec<(i64, String, Option<u32>, String, i64)> = stmt
        .query_map(params![class_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut entries = Vec::new();
    for (student_row_id, name, score, class_name, started_at) in rows {
        if normalize_name(&name) != normalized_name {
            continue;
        }
        let mut tr_stmt = conn.prepare(
            "SELECT a.title, tr.correct, tr.total
             FROM test_results tr
             JOIN assignments a ON a.id = tr.assignment_id
             WHERE a.student_id = ?1
             ORDER BY tr.submitted_at",
        )?;
        let test_results = tr_stmt
            .query_map(params![student_row_id], |row| {
                Ok(TestResultEntry { title: row.get(0)?, correct: row.get(1)?, total: row.get(2)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        entries.push(LessonHistoryEntry { class_name, started_at, score, test_results });
    }
    Ok(entries)
}

/// The local, offline "who's allowed to open this teacher console" profile — a
/// single row (`id` is `CHECK`-constrained to 1), not a multi-user account system.
/// `salt`/`password_hash` are hex-encoded byte strings; see `teacher::auth` for
/// how they're produced and checked.
pub struct TeacherProfile {
    pub name: String,
    pub salt: String,
    pub password_hash: String,
}

pub fn load_teacher_profile(conn: &Connection) -> Result<Option<TeacherProfile>> {
    conn.query_row("SELECT name, salt, password_hash FROM teacher_profile WHERE id = 1", [], |row| {
        Ok(TeacherProfile { name: row.get(0)?, salt: row.get(1)?, password_hash: row.get(2)? })
    })
    .optional()
    .context("loading teacher profile")
}

/// Creates or overwrites the (single) teacher profile row.
pub fn save_teacher_profile(conn: &Connection, name: &str, salt: &str, password_hash: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO teacher_profile (id, name, salt, password_hash, created_at) VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, salt = excluded.salt,
             password_hash = excluded.password_hash, created_at = excluded.created_at",
        params![name, salt, password_hash, now_epoch()],
    )?;
    Ok(())
}

/// "Сбросить" — deletes only the profile (name + password), never touches
/// lessons/students/scores/etc., which are lesson data, not profile data.
pub fn delete_teacher_profile(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM teacher_profile", [])?;
    Ok(())
}

/// A connection-related event, for incident review — who tried to connect (or
/// disconnected), from where, and whether it actually succeeded. Rejected-PIN
/// attempts are the important case: repeated ones from the same IP/name are what
/// a brute-force guess at the lesson PIN would look like in this log.
pub struct ConnectionLogEntry {
    pub at: i64,
    pub name_raw: String,
    pub ip: String,
    pub event: String,
}

pub fn insert_connection_log(conn: &Connection, lesson_id: i64, name_raw: &str, ip: &str, event: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO connection_log (lesson_id, at, name_raw, name_normalized, ip, event) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![lesson_id, now_epoch(), name_raw, normalize_name(name_raw), ip, event],
    )?;
    Ok(())
}

/// Most recent `limit` entries, newest first — optionally restricted to one
/// lesson (the "Только текущий урок" filter). Two separate queries rather than
/// one with an optional parameter: simpler than building a dynamic WHERE clause
/// for what's just two fixed shapes.
pub fn list_connection_log(conn: &Connection, lesson_filter: Option<i64>, limit: u32) -> Result<Vec<ConnectionLogEntry>> {
    fn collect(mut stmt: rusqlite::Statement, params: impl rusqlite::Params) -> Result<Vec<ConnectionLogEntry>> {
        let rows = stmt
            .query_map(params, |row| {
                Ok(ConnectionLogEntry {
                    at: row.get(0)?,
                    name_raw: row.get(1)?,
                    ip: row.get(2)?,
                    event: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    match lesson_filter {
        Some(lesson_id) => collect(
            conn.prepare("SELECT at, name_raw, ip, event FROM connection_log WHERE lesson_id = ?1 ORDER BY at DESC LIMIT ?2")?,
            params![lesson_id, limit],
        ),
        None => collect(
            conn.prepare("SELECT at, name_raw, ip, event FROM connection_log ORDER BY at DESC LIMIT ?1")?,
            params![limit],
        ),
    }
}
