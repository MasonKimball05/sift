//! The sift app's Rust side.
//!
//! The window shows a web page (app/ui). The page can't touch files itself; it
//! asks Rust by calling the functions marked `#[tauri::command]` below, by name,
//! with `invoke("approve", { paths })`. Tauri turns the page's JSON arguments
//! into Rust values and the return value back into JSON, using serde. Each
//! command is a thin wrapper around the sift library, the same code the
//! command line uses.
//!
//! Errors go back to the page as plain strings (`Result<T, String>`), because
//! the page can only receive things that turn into JSON; `{e:#}` formats an
//! anyhow error with its whole chain of causes.

// In a release build, don't open a Terminal window next to the app on Windows.
// (Harmless on macOS; it's the line every Tauri app starts with.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use sift::config::{tilde, Config, Mode};
use sift::engine::{self, Explanation, Suggestion};
use sift::settings::{self, NewRule, RuleInfo};
use sift::{history, launchd};
use std::path::PathBuf;

fn config() -> Result<Config, String> {
    Config::load().map_err(|e| format!("{e:#}"))
}

/// Turns a list of path strings from the page into real paths.
fn paths(list: Vec<String>) -> Vec<PathBuf> {
    list.into_iter().map(PathBuf::from).collect()
}

#[derive(Serialize)]
struct Status {
    mode: String,
    /// The background watcher is set to run at login.
    watching: bool,
    rules_path: String,
    waiting: usize,
    /// A problem with the rules file, if there is one, so the page can say so.
    error: Option<String>,
}

#[tauri::command]
fn status() -> Status {
    let base = |error| Status {
        mode: "suggest".into(),
        watching: launchd::is_installed(),
        rules_path: tilde(&Config::path()),
        waiting: history::pending().len(),
        error,
    };
    match Config::load() {
        Ok(config) => Status { mode: if config.mode == Mode::Move { "move" } else { "suggest" }.into(), ..base(None) },
        Err(e) => base(Some(format!("{e:#}"))),
    }
}

/// Files waiting for review.
#[tauri::command]
fn suggestions() -> Result<Vec<Suggestion>, String> {
    Ok(engine::suggestions(&config()?))
}

/// Looks over the watched folders now and lines up anything a rule matches
/// (or, in move mode, sorts it). Returns how many joined the review list.
#[tauri::command]
fn scan() -> Result<usize, String> {
    Ok(engine::sort_all(&config()?))
}

#[tauri::command]
fn approve(paths: Vec<String>) -> Result<Vec<String>, String> {
    Ok(engine::approve(&config()?, &self::paths(paths)))
}

#[tauri::command]
fn dismiss(paths: Vec<String>) -> Result<(), String> {
    engine::dismiss(&self::paths(paths)).map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
struct Move {
    /// Its position in the history file, for undo.
    index: usize,
    time: String,
    name: String,
    from: String,
    to: String,
    rule: String,
    undone: bool,
}

/// The most recent moves, newest first.
#[tauri::command]
fn history(limit: usize) -> Vec<Move> {
    let entries = history::read();
    entries
        .iter()
        .enumerate()
        .rev()
        .take(limit)
        .map(|(index, entry)| Move {
            index,
            time: entry.time.format("%b %-d, %-I:%M %p").to_string(),
            name: entry.from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            from: entry.from.parent().map(tilde).unwrap_or_default(),
            to: entry.to.as_deref().map(tilde).unwrap_or_else(|| "the Trash".into()),
            rule: entry.rule.clone(),
            undone: entry.undone,
        })
        .collect()
}

#[tauri::command]
fn undo(index: usize) -> Result<String, String> {
    history::undo_at(index).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn rules_text() -> Result<String, String> {
    Config::read_text().map_err(|e| format!("{e:#}"))
}

/// Saves the edited rules if they're valid; otherwise says what's wrong.
#[tauri::command]
fn save_rules(text: String) -> Result<(), String> {
    Config::save_text(&text).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn set_mode(mode: String) -> Result<(), String> {
    let mode = if mode == "move" { Mode::Move } else { Mode::Suggest };
    Config::set_mode(mode).map_err(|e| format!("{e:#}"))
}

/// Turns the background watcher on or off. It's this app, started at login with
/// `watch` (see `main` below), so macOS's permission prompt names sift.
#[tauri::command]
fn set_watching(on: bool) -> Result<(), String> {
    let result = if on {
        std::env::current_exe().map_err(anyhow::Error::from).and_then(launchd::install_program)
    } else {
        launchd::uninstall()
    };
    result.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn explain(path: String) -> Result<Explanation, String> {
    engine::explain(&config()?, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

// ---- Where Files Go ----

/// Every rule with its folder, for the Where Files Go page.
#[tauri::command]
fn rule_list() -> Result<Vec<RuleInfo>, String> {
    Ok(settings::rules(&config()?))
}

/// The file kinds to pick from when adding a rule.
#[tauri::command]
fn kinds() -> Vec<String> {
    settings::KINDS.iter().map(|(name, _)| name.to_string()).collect()
}

#[tauri::command]
fn set_rule_folder(name: String, folder: String, subfolders: String) -> Result<(), String> {
    let folder = sift::config::expand_home(&folder);
    settings::set_destination(&name, &folder, &subfolders).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn set_rule_enabled(name: String, on: bool) -> Result<(), String> {
    settings::set_enabled(&name, on).map_err(|e| format!("{e:#}"))
}

/// `rule` arrives from the page as JSON and serde turns it into a `NewRule`.
#[tauri::command]
fn add_rule(rule: NewRule) -> Result<(), String> {
    settings::add(&rule).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn remove_rule(name: String) -> Result<(), String> {
    settings::remove(&name).map_err(|e| format!("{e:#}"))
}

/// Shows a file (or the folder it went to) in Finder.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    let path = sift::config::expand_home(&path);
    let mut command = std::process::Command::new("open");
    if path.is_dir() { command.arg(&path) } else { command.arg("-R").arg(&path) };
    command.status().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens the rules file in the default text editor.
#[tauri::command]
fn open_rules() -> Result<(), String> {
    std::process::Command::new("open").arg("-t").arg(Config::path()).status().map(|_| ()).map_err(|e| e.to_string())
}

fn main() {
    // Started at login by launchd as `sift watch`: no window, just the watcher.
    if std::env::args().nth(1).as_deref() == Some("watch") {
        let config = Config::load().unwrap_or_else(|e| panic!("sift: {e:#}"));
        if let Err(e) = sift::watch::run(&config) {
            eprintln!("sift: {e:#}");
        }
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Every command the page may call has to be listed here.
        .invoke_handler(tauri::generate_handler![
            status, suggestions, scan, approve, dismiss, history, undo,
            rules_text, save_rules, set_mode, set_watching, explain, reveal, open_rules,
            rule_list, kinds, set_rule_folder, set_rule_enabled, add_rule, remove_rule
        ])
        .run(tauri::generate_context!())
        .expect("error while running the sift app");
}
