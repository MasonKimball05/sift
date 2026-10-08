//! sift: sorts new files in Downloads and Desktop into the right folders.
//!
//! This is the library: everything except a way to drive it. Two programs use
//! it: the `sift` command line (src/main.rs) and the sift app (app/src-tauri).
//! Splitting it this way is common in Rust: `lib.rs` makes a crate others can
//! `use`, and `main.rs` in the same package is a thin program on top of it.
//!
//! The pieces, in the order a file meets them:
//!   watch.rs    notices a new file and waits for it to finish downloading
//!   facts.rs    learns about it: name, type, where it came from, its text
//!   rules.rs    picks the first rule in rules.toml that matches
//!   actions.rs  moves, renames, tags or trashes it, never overwriting
//!   history.rs  logs the move, so it can be undone
//!   engine.rs   the steps that tie those together
//!   config.rs   reads (and saves) the rules
//!   settings.rs changes them from the app, keeping comments
//!   launchd.rs  runs the watcher at login

// `pub mod` makes each module part of the library's public API.
pub mod actions;
pub mod config;
pub mod engine;
pub mod facts;
pub mod history;
pub mod launchd;
pub mod rules;
pub mod settings;
pub mod watch;
