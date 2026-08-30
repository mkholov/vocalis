//! Local WAV storage for the student's own mic recordings (self-review of
//! pronunciation, or a homework deliverable) — a plain hand-written PCM WAV writer,
//! since it's a local file rather than a network stream and doesn't need Opus.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use super::state::RecordingEntry;

/// `~/.local/share/Vocalis/Recordings` on macOS/Linux, `%APPDATA%\Vocalis\Recordings`
/// on Windows — the same app-data convention the teacher console's SQLite database
/// uses, under its own subfolder. Deliberately not the temp dir the "files received
/// from teacher" flow uses (`std::env::temp_dir`) — those are transient by nature,
/// but a student's own recordings are exactly the kind of thing meant to stick around.
fn recordings_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
    }
    .unwrap_or_else(std::env::temp_dir);
    base.join("Vocalis").join("Recordings")
}

/// Writes `samples` (mono, 16-bit, at `sample_rate`) as a WAV file and returns the
/// saved entry.
pub fn save(samples: &[i16], sample_rate: u32) -> Result<RecordingEntry> {
    let dir = recordings_dir();
    std::fs::create_dir_all(&dir).context("creating recordings directory")?;

    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("recording_{epoch}.wav"));
    write_wav_mono(&path, samples, sample_rate)?;

    Ok(RecordingEntry {
        duration_secs: samples.len() as f32 / sample_rate.max(1) as f32,
        path,
    })
}

/// Scans the recordings directory for `.wav` files, newest first, so the list
/// survives an app restart — they're just files already sitting on disk, nothing
/// extra needs to be persisted.
pub fn list_existing() -> Vec<RecordingEntry> {
    let Ok(entries) = std::fs::read_dir(recordings_dir()) else {
        return Vec::new();
    };
    let mut recordings: Vec<(SystemTime, RecordingEntry)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            let duration_secs = wav_duration_secs(&path).unwrap_or(0.0);
            Some((modified, RecordingEntry { path, duration_secs }))
        })
        .collect();
    recordings.sort_by(|a, b| b.0.cmp(&a.0));
    recordings.into_iter().map(|(_, r)| r).collect()
}

pub fn delete(path: &Path) -> Result<()> {
    std::fs::remove_file(path).context("deleting recording")
}

fn write_wav_mono(path: &Path, samples: &[i16], sample_rate: u32) -> Result<()> {
    let mut file = std::fs::File::create(path).context("creating WAV file")?;
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2; // mono, 16-bit
    let block_align: u16 = 2;

    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&1u16.to_le_bytes())?; // mono
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?; // bits per sample
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        file.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

/// Reads just the 44-byte canonical header (which is all `write_wav_mono` ever
/// produces) to recover a recording's duration when restoring the list from disk.
fn wav_duration_secs(path: &Path) -> Option<f32> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 44];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let channels = u16::from_le_bytes(header[22..24].try_into().ok()?);
    let sample_rate = u32::from_le_bytes(header[24..28].try_into().ok()?);
    let bits_per_sample = u16::from_le_bytes(header[34..36].try_into().ok()?);
    let data_len = u32::from_le_bytes(header[40..44].try_into().ok()?);
    let bytes_per_frame = (bits_per_sample / 8).max(1) as u32 * (channels.max(1) as u32);
    Some(data_len as f32 / bytes_per_frame as f32 / sample_rate.max(1) as f32)
}
