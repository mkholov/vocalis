//! Small building blocks shared across the teacher and student UIs — currently
//! just [`section_header`], factored out after an audit turned up four
//! different ad hoc ways of labeling a section (bare label, label+separator,
//! a bordered `ui.group()`, bare `ui.strong()`) scattered across the teacher
//! tabs and the student settings window.

use eframe::egui;

use crate::theme;

/// A section header: a muted label followed by a full-width separator. Use
/// this instead of a bare `ui.strong()`/`ui.colored_label()` or a bordered
/// `ui.group()` anywhere a screen groups related controls under a caption —
/// the goal is one consistent look for "here's a labeled group of controls"
/// everywhere it appears, not a per-screen judgment call.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.colored_label(theme::muted(), text);
    ui.separator();
}
