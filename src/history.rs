//! A log of everything sift has moved, so it can be undone.
//!
//! One JSON object per line ("JSON Lines") in
//! `~/Library/Application Support/sift/history.jsonl`: adding an entry is just
//! appending a line, and a half-written last line can't spoil the ones before it.
//! The files waiting for `sift review` live next to it, in `pending.json`.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `Serialize` and `Deserialize` let serde_json turn this to and from JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub time: DateTime<Local>,
    pub rule: String,
    pub from: PathBuf,
    /// Where it went; `None` means the Trash.
    pub to: Option<PathBuf>,
    #[serde(default)]
    pub undone: bool,
}

pub fn folder() -> PathBuf {
    dirs::data_dir().expect("no Application Support folder").join("sift")
}

fn log_path() -> PathBuf {
    folder().join("history.jsonl")
}

pub fn record(entry: &Entry) -> Result<()> {
    std::fs::create_dir_all(folder())?;
    let mut file = OpenOptions::new().create(true).append(true).open(log_path())?;
    writeln!(file, "{}", serde_json::to_string(entry)?)?;
    Ok(())
}

pub fn read() -> Vec<Entry> {
    read_from(&log_path())
}

fn read_from(path: &Path) -> Vec<Entry> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // `filter_map` with `.ok()` skips any line that doesn't parse.
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

fn write_all(entries: &[Entry]) -> Result<()> {
    std::fs::create_dir_all(folder())?;
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry)?);
        text.push('\n');
    }
    std::fs::write(log_path(), text)?;
    Ok(())
}

/// Puts back the last `count` moves (newest first) and marks them undone.
/// Files sent to the Trash are listed instead: Finder's Put Back returns them.
pub fn undo(count: usize) -> Result<Vec<String>> {
    let mut entries = read();
    let mut report = vec![];
    // `iter_mut` lets us flip `undone` in place, `rev()` starts from the newest,
    // `filter` skips ones already undone, and `take` stops after `count`.
    for entry in entries.iter_mut().rev().filter(|e| !e.undone).take(count) {
        report.push(reverse(entry)?);
    }
    write_all(&entries)?;
    Ok(report)
}

/// Puts back one move, by its position in `read()` (the app's per-row Undo).
pub fn undo_at(index: usize) -> Result<String> {
    let mut entries = read();
    let entry = entries.get_mut(index).ok_or_else(|| anyhow::anyhow!("no move number {index}"))?;
    if entry.undone {
        return Ok("Already undone.".into());
    }
    let report = reverse(entry)?;
    write_all(&entries)?;
    Ok(report)
}

/// Moves one entry's file back and marks it undone; says what happened.
fn reverse(entry: &mut Entry) -> Result<String> {
    let report = match &entry.to {
        Some(to) if to.exists() => {
            let back = crate::actions::move_file(to, &entry.from)?;
            format!("Put back {}", crate::config::tilde(&back))
        }
        Some(to) => format!("Skipped {}: it's not there any more", crate::config::tilde(to)),
        None => format!(
            "{} is in the Trash: select it in Finder and choose File \u{25B8} Put Back",
            entry.from.file_name().unwrap_or_default().to_string_lossy()
        ),
    };
    entry.undone = true;
    Ok(report)
}

// ---- Files waiting for review (suggest mode) ----

fn pending_path() -> PathBuf {
    folder().join("pending.json")
}

/// Only paths are kept: the decision is worked out again at review time, in
/// case the file or the rules changed in between.
pub fn pending() -> Vec<PathBuf> {
    std::fs::read_to_string(pending_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn set_pending(paths: &[PathBuf]) -> Result<()> {
    std::fs::create_dir_all(folder())?;
    std::fs::write(pending_path(), serde_json::to_string_pretty(paths)?)?;
    Ok(())
}

/// Adds a path to the review list; true if it wasn't there already.
pub fn add_pending(path: &Path) -> Result<bool> {
    let mut list = pending();
    if list.iter().any(|p| p == path) {
        return Ok(false);
    }
    list.push(path.to_path_buf());
    set_pending(&list)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_bad_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let entry = Entry { time: Local::now(), rule: "Receipts".into(), from: "/a.pdf".into(), to: None, undone: false };
        std::fs::write(&path, format!("{}\nnot json\n", serde_json::to_string(&entry).unwrap())).unwrap();
        assert_eq!(read_from(&path), vec![entry]);
    }
}
