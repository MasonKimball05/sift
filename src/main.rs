//! The `sift` command line. All the work is in the library (src/lib.rs and the
//! modules it lists); this file reads the command, calls the library, and
//! prints what comes back. The app (app/src-tauri) calls the same functions
//! and shows the results in a window instead.

use anyhow::Result;
use clap::{Parser, Subcommand};
use sift::config::{tilde, Config, Mode};
use sift::{engine, history, launchd, watch};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;

/// clap reads the doc comments below as the help text (`sift --help`).
#[derive(Parser)]
#[command(version, about = "Sorts new files in Downloads and Desktop into the right folders")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sort what's in the watched folders now.
    Run {
        /// Only show what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// Keep watching and sort new files as they arrive (what the login item runs).
    Watch,
    /// Go through the files waiting for your OK (suggest mode).
    Review,
    /// Put back the last moves.
    Undo {
        /// How many moves to undo.
        #[arg(default_value_t = 1)]
        count: usize,
    },
    /// Show what sift knows about a file and why each rule does or doesn't match.
    Explain { file: PathBuf },
    /// Show recent moves.
    History {
        #[arg(default_value_t = 15)]
        count: usize,
    },
    /// Start sift at login, watching in the background.
    Install,
    /// Stop the background watcher and remove the login item.
    Uninstall,
    /// Print the rules file's location.
    Rules,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // These shouldn't fail just because the rules file has a mistake.
    match cli.command {
        Command::Rules => {
            println!("{}", Config::path().display());
            return Ok(());
        }
        Command::Uninstall => {
            launchd::uninstall()?;
            println!("sift is no longer running in the background.");
            return Ok(());
        }
        _ => {}
    }
    let config = Config::load()?;
    match cli.command {
        Command::Run { dry_run: true } => {
            for (path, decision) in engine::scan(&config) {
                println!("{}  \u{2192}  {}   [{}]", tilde(&path), engine::describe(&decision.action), decision.rule);
            }
        }
        Command::Run { dry_run: false } => {
            let lined_up = engine::sort_all(&config);
            if config.mode == Mode::Suggest {
                println!("{lined_up} new file{} waiting. Review with: sift review", if lined_up == 1 { "" } else { "s" });
            }
        }
        Command::Watch => watch::run(&config)?,
        Command::Review => review(&config)?,
        Command::Undo { count } => {
            let report = history::undo(count)?;
            if report.is_empty() {
                println!("Nothing to undo.");
            }
            report.iter().for_each(|line| println!("{line}"));
        }
        Command::Explain { file } => explain(&config, &file)?,
        Command::History { count } => {
            for entry in history::read().iter().rev().take(count) {
                let to = entry.to.as_deref().map(tilde).unwrap_or_else(|| "the Trash".into());
                let undone = if entry.undone { "  (undone)" } else { "" };
                println!("{}  {} \u{2192} {}  [{}]{undone}", entry.time.format("%b %d %H:%M"), tilde(&entry.from), to, entry.rule);
            }
        }
        Command::Install => {
            launchd::install()?;
            println!("sift is watching in the background, and will start again at login.");
            println!("macOS will ask once to let sift use your Downloads and Desktop folders.");
        }
        Command::Uninstall | Command::Rules => unreachable!("handled above"),
    }
    Ok(())
}

/// One file at a time: y to do it, n to leave it, a for every file from the
/// same rule, q to stop.
fn review(config: &Config) -> Result<()> {
    let waiting = engine::suggestions(config);
    if waiting.is_empty() {
        println!("Nothing waiting. (In suggest mode, new files show up here.)");
        return Ok(());
    }
    // Rules answered "all" for. A `HashSet` answers "is this in here?" quickly.
    let mut all_from: HashSet<String> = HashSet::new();
    for (index, item) in waiting.iter().enumerate() {
        let path = PathBuf::from(&item.path);
        if all_from.contains(&item.rule) {
            report(engine::approve(config, &[path]));
            continue;
        }
        let same_rule = waiting[index..].iter().filter(|s| s.rule == item.rule).count();
        println!("\n{}/{}\n  \u{2192} {}   [{}]", item.from, item.name, item.to, item.rule);
        print!("Do it? [y]es / [n]o / [a]ll {same_rule} from \"{}\" / [q]uit: ", item.rule);
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        match answer.trim().to_lowercase().as_str() {
            "y" | "yes" => report(engine::approve(config, &[path])),
            "a" | "all" => {
                report(engine::approve(config, &[path]));
                all_from.insert(item.rule.clone());
            }
            "q" | "quit" => break, // the rest stay on the list for next time
            _ => {
                engine::dismiss(&[path])?;
                println!("  Left where it is.");
            }
        }
    }
    if config.mode == Mode::Suggest {
        println!("\nWhen these look right every time, set mode = \"move\" in {}", tilde(&Config::path()));
    }
    Ok(())
}

fn report(problems: Vec<String>) {
    problems.iter().for_each(|p| eprintln!("  {p}"));
}

fn explain(config: &Config, file: &std::path::Path) -> Result<()> {
    let e = engine::explain(config, file)?;
    let or = |list: &[String], none: &str| if list.is_empty() { none.to_string() } else { list.join(", ") };
    println!("{}", tilde(file));
    println!("  type:          {}", e.kind);
    println!("  arrived:       {}", e.arrived);
    println!("  unused for:    {} days", e.unused_days);
    println!("  from:          {}", or(&e.from, "(not a download)"));
    println!("  course:        {}", e.course.as_deref().unwrap_or("(none found)"));
    if let Some(number) = &e.unnamed_course {
        println!("                 Canvas course {number} has no name yet. Add it to the rules file:");
        println!("                 [course_names]");
        println!("                 \"{number}\" = \"COSC 360\"   (its real code)");
    }
    println!("  tags:          {}", or(&e.tags, "(none)"));
    println!("  text:          {}", if e.text_preview.is_empty() { "(none read)".into() } else { format!("{}\u{2026}", e.text_preview) });
    println!();
    for verdict in &e.verdicts {
        match &verdict.reason {
            None => println!("  \u{2713} {}  \u{2192}  {}", verdict.rule, e.outcome.as_deref().unwrap_or("")),
            Some(reason) => println!("  \u{2717} {}: {reason}", verdict.rule),
        }
    }
    if e.outcome.is_none() {
        println!("\n  No rule matches, so it stays put.");
    }
    Ok(())
}
