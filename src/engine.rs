//! The steps that tie the pieces together, shared by the command line and the app.
//!
//! Nothing here prints. Each step hands back data (`Suggestion`, `Explanation`)
//! and lets the caller show it: the command line as text, the app as a page.
//! `#[derive(Serialize)]` on those structs is what lets the app send them to
//! its web page as JSON.

use crate::config::{tilde, Config, Mode};
use crate::facts::{canvas_course_number, Facts};
use crate::rules::{self, Action, Decision};
use crate::{actions, history};
use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Files sift should never touch: hidden files, folders, and downloads still in
/// progress (browsers write those with their own extensions).
pub fn skip(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let partial = ["crdownload", "download", "part", "partial", "tmp"];
    name.starts_with('.')
        || name.starts_with("Icon")
        || !path.is_file()
        || path.extension().is_some_and(|e| partial.iter().any(|p| e.eq_ignore_ascii_case(p)))
}

/// What the rules would do with a file right now, or `None` to leave it.
pub fn plan(config: &Config, path: &Path) -> Option<Decision> {
    if skip(path) {
        return None;
    }
    let facts = Facts::of(path).ok()?;
    rules::decide(config, &facts, Local::now())
}

/// Every file in the watched folders that a rule would sort, with what it
/// would do. This is a dry run: nothing changes.
pub fn scan(config: &Config) -> Vec<(PathBuf, Decision)> {
    let mut found = vec![];
    for folder in config.watched() {
        // A folder that's missing or can't be read is skipped, not fatal.
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(decision) = plan(config, &path) {
                found.push((path, decision));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

#[derive(Debug, PartialEq)]
pub enum Sorted {
    Done,
    Pending,
}

/// Sorts one file as the rules and the mode say: in suggest mode it joins the
/// review list, in move mode it's moved. Errors are printed rather than
/// returned, so one stubborn file never stops the rest.
pub fn sort_one(config: &Config, path: &Path) -> Option<Sorted> {
    let decision = plan(config, path)?;
    match config.mode {
        Mode::Suggest => match history::add_pending(path) {
            Ok(true) => Some(Sorted::Pending),
            Ok(false) => None, // already waiting
            Err(error) => {
                eprintln!("sift: {error:#}");
                None
            }
        },
        Mode::Move => match carry_out(path, &decision) {
            Ok(_) => Some(Sorted::Done),
            Err(error) => {
                eprintln!("sift: {error:#}");
                None
            }
        },
    }
}

/// Sorts everything in the watched folders. Returns how many files were newly
/// lined up for review (in suggest mode), and sends a notification about them.
pub fn sort_all(config: &Config) -> usize {
    let lined_up = scan(config)
        .iter()
        .filter(|(path, _)| sort_one(config, path) == Some(Sorted::Pending))
        .count();
    if lined_up > 0 {
        notify(&format!("{lined_up} file{} ready to sort. Open sift to review.", if lined_up == 1 { "" } else { "s" }));
    }
    lined_up
}

/// Does a decision and logs it.
pub fn carry_out(path: &Path, decision: &Decision) -> Result<actions::Outcome> {
    let outcome = actions::apply(&decision.action, path)?;
    let to = match &outcome {
        actions::Outcome::Moved(to) => Some(to.clone()),
        actions::Outcome::Trashed => None,
    };
    let entry = history::Entry { time: Local::now(), rule: decision.rule.clone(), from: path.to_path_buf(), to, undone: false };
    if let Err(error) = history::record(&entry) {
        eprintln!("sift: moved, but couldn't log it: {error:#}");
    }
    Ok(outcome)
}

/// "~/Documents/School/COSC 490/Lab.pdf  (tags: School)" or "the Trash".
pub fn describe(action: &Action) -> String {
    match action {
        Action::Move { to, tags } if tags.is_empty() => tilde(to),
        Action::Move { to, tags } => format!("{}  (tags: {})", tilde(to), tags.join(", ")),
        Action::Trash => "the Trash".into(),
    }
}

// ---- Review ----

/// A file waiting for review, as the app shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub path: String,
    pub name: String,
    /// The folder it's in now: "~/Downloads".
    pub from: String,
    /// Where it would go ("~/Documents/Finance/2026/2026-10-07 receipt.pdf"), or "the Trash".
    pub to: String,
    pub rule: String,
    pub trash: bool,
    pub tags: Vec<String>,
}

impl Suggestion {
    fn new(path: &Path, decision: &Decision) -> Suggestion {
        let (to, trash, tags) = match &decision.action {
            Action::Move { to, tags } => (tilde(to), false, tags.clone()),
            Action::Trash => ("the Trash".into(), true, vec![]),
        };
        Suggestion {
            path: path.display().to_string(),
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            from: path.parent().map(tilde).unwrap_or_default(),
            to,
            rule: decision.rule.clone(),
            trash,
            tags,
        }
    }
}

/// The review list, each with what would happen now. Files that are gone or no
/// longer match a rule drop off the list.
pub fn suggestions(config: &Config) -> Vec<Suggestion> {
    let mut keep = vec![];
    let mut out = vec![];
    for path in history::pending() {
        if let Some(decision) = plan(config, &path) {
            out.push(Suggestion::new(&path, &decision));
            keep.push(path);
        }
    }
    let _ = history::set_pending(&keep);
    out
}

/// Every file sorting would touch right now, as suggestions (for the app's
/// "Scan now", whatever the mode).
pub fn preview(config: &Config) -> Vec<Suggestion> {
    scan(config).iter().map(|(path, decision)| Suggestion::new(path, decision)).collect()
}

/// Approves files from the review list: does each one and takes it off the list.
/// Returns one message per file that couldn't be done.
pub fn approve(config: &Config, paths: &[PathBuf]) -> Vec<String> {
    let mut problems = vec![];
    for path in paths {
        match plan(config, path) {
            Some(decision) => {
                if let Err(error) = carry_out(path, &decision) {
                    problems.push(format!("{}: {error:#}", path.display()));
                }
            }
            None => problems.push(format!("{}: no rule matches it any more", path.display())),
        }
    }
    let left: Vec<PathBuf> = history::pending().into_iter().filter(|p| !paths.contains(p)).collect();
    let _ = history::set_pending(&left);
    problems
}

/// Leaves files where they are: off the review list, untouched.
pub fn dismiss(paths: &[PathBuf]) -> Result<()> {
    let left: Vec<PathBuf> = history::pending().into_iter().filter(|p| !paths.contains(p)).collect();
    history::set_pending(&left)
}

// ---- Explain ----

#[derive(Debug, Serialize)]
pub struct Verdict {
    pub rule: String,
    pub matched: bool,
    /// Why not, when it didn't match.
    pub reason: Option<String>,
}

/// Everything sift knows about a file and each rule's verdict, in order, up to
/// the first match.
#[derive(Debug, Serialize)]
pub struct Explanation {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub arrived: String,
    pub unused_days: i64,
    pub from: Vec<String>,
    pub course: Option<String>,
    /// A Canvas course number with no name yet, to add under [course_names].
    pub unnamed_course: Option<String>,
    pub tags: Vec<String>,
    pub text_preview: String,
    pub verdicts: Vec<Verdict>,
    /// What would happen: "~/Documents/..." or "the Trash", or None to stay put.
    pub outcome: Option<String>,
}

pub fn explain(config: &Config, path: &Path) -> Result<Explanation> {
    let facts = Facts::of(path)?;
    let now = Local::now();
    let mut verdicts = vec![];
    for rule in &config.rules {
        let result = rules::check(rule, &facts, now);
        let matched = result.is_ok();
        verdicts.push(Verdict { rule: rule.name.clone(), matched, reason: result.err() });
        if matched {
            break; // first match wins; the rest never run
        }
    }
    let text = facts.text();
    Ok(Explanation {
        path: path.display().to_string(),
        name: facts.file_name(),
        kind: if facts.ext.is_empty() { "(none)".into() } else { facts.ext.clone() },
        arrived: facts.created.format("%b %-d, %Y %-I:%M %p").to_string(),
        unused_days: facts.days_unused(now),
        from: facts.hosts.clone(),
        course: facts.course(&config.course_codes, &config.courses),
        unnamed_course: facts.urls.iter().filter_map(|u| canvas_course_number(u)).find(|n| !config.courses.contains_key(n)),
        tags: actions::tags(path),
        text_preview: text.split_whitespace().take(24).collect::<Vec<_>>().join(" "),
        outcome: rules::decide(config, &facts, now).map(|d| describe(&d.action)),
        verdicts,
    })
}

/// A macOS notification, through AppleScript: quick, and needs no app bundle.
pub fn notify(message: &str) {
    let script = format!("display notification {:?} with title \"sift\"", message);
    let _ = std::process::Command::new("osascript").args(["-e", &script]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_hidden_and_half_downloaded_files() {
        let dir = tempfile::tempdir().unwrap();
        let make = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, "x").unwrap();
            path
        };
        assert!(skip(&make(".DS_Store")));
        assert!(skip(&make("movie.mp4.crdownload")));
        assert!(skip(&make("big.zip.download")));
        assert!(skip(dir.path())); // a folder
        assert!(!skip(&make("Syllabus.pdf")));
    }
}
