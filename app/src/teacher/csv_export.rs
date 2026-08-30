//! Manual CSV generation for the student-history export. Deliberately no CSV
//! crate: RFC4180 quoting (wrap a field containing a comma/quote/newline in
//! quotes, doubling any embedded quote) is a few lines, and this app has
//! consistently avoided pulling in a dependency for something that small.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use super::app::format_epoch_date;
use super::db::LessonHistoryEntry;

fn escape_field(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn push_row(out: &mut String, fields: &[String]) {
    out.push_str(&fields.iter().map(|f| escape_field(f)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
}

fn score_cell(score: Option<u32>) -> String {
    score.map(|v| format!("{v}%")).unwrap_or_default()
}

/// Multiple tests in one lesson collapse into one cell, semicolon-separated
/// (comma is the CSV delimiter) — matches how `history_card_view` already shows
/// them on screen, just as one line instead of several.
fn test_results_cell(entry: &LessonHistoryEntry) -> String {
    entry
        .test_results
        .iter()
        .map(|tr| format!("{}: {}/{}", tr.title, tr.correct, tr.total))
        .collect::<Vec<_>>()
        .join("; ")
}

fn write_with_bom(path: &Path, content: &str) -> Result<()> {
    let mut file = std::fs::File::create(path).context("creating CSV file")?;
    // UTF-8 BOM — without it, Excel on Windows guesses the system codepage
    // instead of UTF-8 and mangles Cyrillic.
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// One student's cross-lesson history (see `db::student_history`), for the
/// "Экспорт" button on the history card.
pub fn write_student_history(path: &Path, history: &[LessonHistoryEntry]) -> Result<()> {
    let mut out = String::new();
    push_row(&mut out, &["Дата".into(), "Класс/урок".into(), "Оценка".into(), "Результаты тестов".into()]);
    for entry in history {
        push_row(
            &mut out,
            &[
                format_epoch_date(entry.started_at),
                entry.class_name.clone(),
                score_cell(entry.score),
                test_results_cell(entry),
            ],
        );
    }
    write_with_bom(path, &out)
}

/// Every roster student's history in one file — same columns plus a leading name
/// column, for the "Экспорт всего класса" button.
pub fn write_class_history(path: &Path, students: &[(String, Vec<LessonHistoryEntry>)]) -> Result<()> {
    let mut out = String::new();
    push_row(
        &mut out,
        &["Ученик".into(), "Дата".into(), "Класс/урок".into(), "Оценка".into(), "Результаты тестов".into()],
    );
    for (name, history) in students {
        if history.is_empty() {
            push_row(&mut out, &[name.clone(), String::new(), String::new(), String::new(), String::new()]);
            continue;
        }
        for entry in history {
            push_row(
                &mut out,
                &[
                    name.clone(),
                    format_epoch_date(entry.started_at),
                    entry.class_name.clone(),
                    score_cell(entry.score),
                    test_results_cell(entry),
                ],
            );
        }
    }
    write_with_bom(path, &out)
}
