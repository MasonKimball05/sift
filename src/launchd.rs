//! Running sift in the background, from login on.
//!
//! macOS starts background programs with launchd. A "LaunchAgent" is a small
//! property list in ~/Library/LaunchAgents saying what to run and when; this
//! one runs `sift watch` at login and restarts it if it ever exits.
//! `launchctl bootstrap` loads it now, so there's no need to log out first.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.masonkimball.sift";

fn plist_path() -> PathBuf {
    crate::config::home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

/// launchd's per-user domain: "gui/501", where 501 is your user id.
fn domain() -> Result<String> {
    let output = Command::new("id").arg("-u").output()?;
    Ok(format!("gui/{}", String::from_utf8_lossy(&output.stdout).trim()))
}

/// Whether the login item is set up.
pub fn is_installed() -> bool {
    plist_path().exists()
}

/// Whether launchd has the watcher loaded right now.
pub fn is_running() -> bool {
    let Ok(domain) = domain() else { return false };
    Command::new("launchctl").args(["print", &format!("{domain}/{LABEL}")]).output()
        .is_ok_and(|out| out.status.success())
}

/// Installs from a given program path (the app passes the `sift` it bundles).
pub fn install() -> Result<()> {
    install_program(std::env::current_exe()?.canonicalize()?)
}

pub fn install_program(program: PathBuf) -> Result<()> {
    // The program launchd will run: the installed `sift`.
    if program.to_string_lossy().contains("/target/") {
        bail!("run this from the installed copy (make install), not the build folder");
    }
    let log = crate::config::home().join("Library/Logs/sift.log");
    // A plist is XML; `plist::Value` builds it as data so nothing needs escaping by hand.
    let mut dict = plist::Dictionary::new();
    dict.insert("Label".into(), LABEL.into());
    dict.insert("ProgramArguments".into(), plist::Value::Array(vec![
        program.to_string_lossy().into_owned().into(),
        "watch".into(),
    ]));
    dict.insert("RunAtLoad".into(), true.into());
    dict.insert("KeepAlive".into(), true.into());
    // Background work at low priority, so sorting never competes with what you're doing.
    dict.insert("ProcessType".into(), "Background".into());
    dict.insert("StandardOutPath".into(), log.to_string_lossy().into_owned().into());
    dict.insert("StandardErrorPath".into(), log.to_string_lossy().into_owned().into());

    let path = plist_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    plist::Value::Dictionary(dict).to_file_xml(&path)?;

    let domain = domain()?;
    // Unload any older copy first; it's fine if there isn't one.
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    let status = Command::new("launchctl").args(["bootstrap", &domain]).arg(&path).status()
        .context("running launchctl")?;
    if !status.success() {
        bail!("launchctl couldn't load {}", path.display());
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let domain = domain()?;
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    let path = plist_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
