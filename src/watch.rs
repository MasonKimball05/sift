//! Watching Downloads and Desktop for new files.
//!
//! macOS reports file changes through FSEvents; the `notify` crate wraps that,
//! and `notify-debouncer-mini` waits until a file has been quiet for a few
//! seconds before passing it on. That wait matters: a download writes its file
//! in pieces, and sorting it halfway would move a broken file.
//!
//! The events arrive over a channel (`mpsc`: many senders, one receiver), a
//! queue between the watcher's thread and this loop.

use crate::config::Config;
use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use std::sync::mpsc;
use std::time::Duration;

/// Runs until stopped. Every hour, and once at the start, it also looks over
/// everything in the folders, which is how the "unused for 30 days" rule ever
/// fires: nothing happens to an old file to make it show up as an event.
pub fn run(config: &Config) -> Result<()> {
    let (sender, receiver) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_secs(config.settle_seconds), sender)?;
    for folder in config.watched() {
        // NonRecursive: only the folder itself, not its subfolders.
        debouncer.watcher().watch(&folder, RecursiveMode::NonRecursive)?;
        eprintln!("sift: watching {}", crate::config::tilde(&folder));
    }

    crate::engine::sort_all(config);
    loop {
        match receiver.recv_timeout(Duration::from_secs(3600)) {
            Ok(Ok(events)) => {
                for event in events {
                    // Events also come for files that were just moved away or
                    // deleted; `sort_one` skips anything that isn't there.
                    crate::engine::sort_one(config, &event.path);
                }
            }
            Ok(Err(error)) => eprintln!("sift: watch error: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                crate::engine::sort_all(config);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
