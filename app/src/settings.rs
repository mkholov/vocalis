//! Local, per-machine app preferences: audio device choices, the screen-demo
//! video quality ceiling, dark/light theme, and (for now, a placeholder for)
//! UI language.
//!
//! Deliberately *not* part of the teacher's lesson SQLite database — these
//! are "how this install is configured" facts, unrelated to any particular
//! class or lesson, and apply identically to whichever of the two roles
//! (teacher/student) happens to run on this machine. A small JSON file next
//! to the database (not inside it) means the student binary — which
//! otherwise never touches a database at all — doesn't need to pull in
//! SQLite just to remember a microphone choice, and the existing lesson DB's
//! schema doesn't need to grow a table that has nothing to do with lessons.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Scaffold for a future real localization system. Deliberately just the one
/// variant for now: every string in the app is still hardcoded Russian, and
/// building a full translation framework to back a single settings-menu
/// dropdown isn't worth it yet. Adding a language later means adding a
/// variant here (and the actual translated strings wherever they end up
/// living) — it does not mean redesigning this type or the settings format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    Russian,
}

impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Language::Russian => "Русский",
        }
    }
}

/// Manual ceiling for the screen-demo video pipeline's capture width/fps —
/// the same three steps `video::AdaptiveQuality` degrades through
/// automatically under load (see `video::QUALITY_LADDER`). Picking a lower
/// ceiling here doesn't disable that automatic degradation, it just starts
/// the ladder further down: a teacher who already knows their machine is
/// weak can skip past the several seconds it'd otherwise take
/// `AdaptiveQuality` to detect that for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoQuality {
    #[default]
    High,
    Medium,
    Low,
}

impl VideoQuality {
    pub const ALL: [VideoQuality; 3] = [VideoQuality::High, VideoQuality::Medium, VideoQuality::Low];

    pub fn label(self) -> &'static str {
        match self {
            VideoQuality::High => "Высокое (1280 px, 15 fps)",
            VideoQuality::Medium => "Среднее (1280 px, 10 fps)",
            VideoQuality::Low => "Низкое (960 px, 10 fps)",
        }
    }

    /// Index into `video::QUALITY_LADDER` this ceiling starts at.
    pub fn ladder_level(self) -> usize {
        match self {
            VideoQuality::High => 0,
            VideoQuality::Medium => 1,
            VideoQuality::Low => 2,
        }
    }
}

/// Dark is the app's original, primary theme (see `theme`'s doc comment) —
/// light is available as a toggle, applied immediately on change (no
/// restart) and used from launch onward via `theme::apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Тёмная",
            Theme::Light => "Светлая",
        }
    }

    pub fn is_light(self) -> bool {
        matches!(self, Theme::Light)
    }
}

/// `None` on either device field means "system default" — resolved fresh
/// each time capture/playback actually starts (see `audio_devices`), so a
/// device that's since been unplugged or renamed never hard-fails a launch,
/// it just falls back.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub video_quality: VideoQuality,
    pub theme: Theme,
    pub language: Language,
    /// Whether the teacher has stepped through (or skipped) the first-run
    /// onboarding walkthrough (see `teacher::onboarding`) — `false` on a fresh
    /// install (missing/default `Settings`), flipped to `true` once, and never
    /// reset automatically. "Показать введение ещё раз" in Settings re-opens
    /// the same walkthrough without touching this flag, so it still only
    /// *auto*-shows once.
    #[serde(default)]
    pub first_run_completed: bool,
}

/// `~/.local/share/Vocalis/settings.json` on macOS/Linux, or
/// `%APPDATA%\Vocalis\settings.json` on Windows — the same directory
/// `teacher::db`'s `vocalis.sqlite3` lives in, just a separate file (see this
/// module's doc comment for why it isn't a table in that database instead).
fn settings_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
    }
    .unwrap_or_else(std::env::temp_dir);
    base.join("Vocalis").join("settings.json")
}

impl Settings {
    /// Loads settings from disk, or the defaults if the file doesn't exist
    /// yet (first run) or fails to parse (e.g. left over from an
    /// incompatible future version) — either way the app should still start
    /// normally, just unconfigured, rather than refuse to launch over it.
    pub fn load() -> Self {
        match std::fs::read_to_string(settings_path()) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).context("creating Vocalis settings directory")?;
        }
        let json = serde_json::to_string_pretty(self).context("serializing settings")?;
        std::fs::write(&path, json).context("writing settings file")?;
        Ok(())
    }
}
