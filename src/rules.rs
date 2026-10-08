//! Deciding where a file goes: try each rule in order, take the first that
//! matches, and fill in its folder and name.

use crate::config::{expand_home, Config, Rule};
use crate::facts::Facts;
use chrono::{DateTime, Local};
use regex::Regex;
use std::path::PathBuf;

/// What to do with a file. An `enum` whose variants carry data: a move knows
/// where to and which tags, a trash needs nothing more.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Move { to: PathBuf, tags: Vec<String> },
    Trash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The rule that matched, for the history and for `sift explain`.
    pub rule: String,
    pub action: Action,
}

/// The first rule that matches, turned into a decision; `None` leaves the file alone.
///
/// `find` stops at the first rule for which the closure returns true, so later
/// rules (and their text reading) never run once one has matched.
pub fn decide(config: &Config, facts: &Facts, now: DateTime<Local>) -> Option<Decision> {
    config
        .rules
        .iter()
        .find(|rule| check(rule, facts, now).is_ok())
        .map(|rule| decision(rule, facts, config))
}

/// Whether a rule matches, and if not, why not (for `sift explain`).
/// `Result<(), String>`: `Ok(())` is a match, `Err(reason)` the first condition
/// that failed. The `?`-free early returns read top to bottom like the rule.
///
/// The text check comes last on purpose: reading a PDF is the slow part, so it
/// only happens for files that every other condition already let through.
pub fn check(rule: &Rule, facts: &Facts, now: DateTime<Local>) -> Result<(), String> {
    if !rule.enabled {
        return Err("turned off".into());
    }
    if !rule.folders.is_empty() && !rule.folders.iter().any(|f| expand_home(f) == facts.folder) {
        return Err(format!("not in {}", rule.folders.join(" or ")));
    }
    if !rule.extensions.is_empty() && !rule.extensions.iter().any(|e| e.eq_ignore_ascii_case(&facts.ext)) {
        return Err(format!("not a {} file", rule.extensions.join("/")));
    }
    if let Some(pattern) = &rule.name_matches {
        // Already checked when the rules were loaded, so this can't fail.
        let regex = Regex::new(pattern).expect("checked in Config::parse");
        if !regex.is_match(&facts.file_name()) {
            return Err(format!("name doesn't match /{pattern}/"));
        }
    }
    if !rule.from_hosts.is_empty() {
        // "instructure.com" also covers "samford.instructure.com".
        let from = |want: &String| facts.hosts.iter().any(|h| h == want || h.ends_with(&format!(".{want}")));
        if !rule.from_hosts.iter().any(from) {
            return Err(format!("not downloaded from {}", rule.from_hosts.join(" or ")));
        }
    }
    if let Some(days) = rule.unused_days {
        let unused = facts.days_unused(now);
        if unused < days as i64 {
            return Err(format!("opened {unused} days ago, under {days}"));
        }
    }
    if !rule.text_any.is_empty() {
        let text = facts.text().to_lowercase();
        if !rule.text_any.iter().any(|phrase| text.contains(&phrase.to_lowercase())) {
            return Err(format!("text has none of: {}", rule.text_any.join(", ")));
        }
    }
    Ok(())
}

fn decision(rule: &Rule, facts: &Facts, config: &Config) -> Decision {
    let action = if rule.trash {
        Action::Trash
    } else {
        // Config::parse made sure a rule that doesn't trash has a `to`.
        let folder = expand_home(&fill(rule.to.as_deref().unwrap_or("~"), facts, config));
        let stem = rule.rename.as_deref().map(|r| fill(r, facts, config)).unwrap_or_else(|| facts.stem.clone());
        let name = if facts.ext.is_empty() { stem } else { format!("{stem}.{}", facts.ext) };
        Action::Move { to: folder.join(clean(&name)), tags: rule.tags.clone() }
    };
    Decision { rule: rule.name.clone(), action }
}

/// Fills in a template: "{date} {name}" -> "2026-10-07 Receipt".
/// {course} is only worked out when the template uses it, because finding it
/// can mean reading the file's text.
pub fn fill(template: &str, facts: &Facts, config: &Config) -> String {
    let mut out = template
        .replace("{name}", &facts.stem)
        .replace("{date}", &facts.created.format("%Y-%m-%d").to_string())
        .replace("{year}", &facts.created.format("%Y").to_string())
        .replace("{month}", &facts.created.format("%m").to_string());
    if out.contains("{course}") {
        let course = facts.course(&config.course_codes, &config.courses).unwrap_or_else(|| "Other".into());
        out = out.replace("{course}", &course);
    }
    out
}

/// A slash or colon in a file name would make a folder or confuse Finder.
fn clean(name: &str) -> String {
    name.replace(['/', ':'], "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{home, STARTER_RULES};

    fn starter() -> Config {
        Config::parse(STARTER_RULES).unwrap()
    }

    fn downloads(name: &str) -> String {
        home().join("Downloads").join(name).display().to_string()
    }

    #[test]
    fn screenshots_by_name() {
        let facts = Facts::sample(&home().join("Desktop/Screenshot 2026-10-07 at 9.41.12 AM.png").display().to_string(), "");
        let decision = decide(&starter(), &facts, Local::now()).unwrap();
        assert_eq!(decision.rule, "Screenshots");
        let Action::Move { to, .. } = decision.action else { panic!("expected a move") };
        assert!(to.starts_with(home().join("Pictures/Screenshots")));
        assert!(to.ends_with("Screenshot 2026-10-07 at 9.41.12 AM.png"));
    }

    #[test]
    fn canvas_downloads_go_to_their_course() {
        let mut facts = Facts::sample(&downloads("Lab4.pdf"), "COSC 490 Lab 4: MNIST");
        facts.hosts = vec!["samford.instructure.com".into()];
        let decision = decide(&starter(), &facts, Local::now()).unwrap();
        assert_eq!(decision.rule, "Canvas downloads");
        assert_eq!(decision.action, Action::Move {
            to: home().join("Documents/School/COSC 490/Lab4.pdf"),
            tags: vec!["School".into()],
        });
    }

    #[test]
    fn receipts_get_dated_names() {
        let facts = Facts::sample(&downloads("amazon.pdf"), "Order Total: $23.17");
        let decision = decide(&starter(), &facts, Local::now()).unwrap();
        assert_eq!(decision.rule, "Receipts");
        let Action::Move { to, .. } = decision.action else { panic!() };
        let today = Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(to.file_name().unwrap().to_string_lossy(), format!("{today} amazon.pdf"));
    }

    #[test]
    fn old_downloads_go_to_the_trash_and_new_ones_stay() {
        let mut facts = Facts::sample(&downloads("thing.zip"), "");
        assert_eq!(decide(&starter(), &facts, Local::now()), None); // new and unrecognized: left alone
        facts.created = Local::now() - chrono::Duration::days(45);
        assert_eq!(decide(&starter(), &facts, Local::now()).unwrap().action, Action::Trash);
        // On the Desktop the old-downloads rule doesn't apply.
        let mut desktop = Facts::sample(&home().join("Desktop/thing.zip").display().to_string(), "");
        desktop.created = facts.created;
        assert_eq!(decide(&starter(), &desktop, Local::now()), None);
    }

    #[test]
    fn explains_why_not() {
        let config = starter();
        let facts = Facts::sample(&downloads("notes.pdf"), "nothing useful");
        let receipts = config.rules.iter().find(|r| r.name == "Receipts").unwrap();
        assert_eq!(check(receipts, &facts, Local::now()).unwrap_err(),
                   "text has none of: receipt, order total, amount paid, payment received, invoice");
    }

    #[test]
    fn names_never_make_folders() {
        assert_eq!(clean("Q3/Q4: totals.pdf"), "Q3-Q4- totals.pdf");
    }
}
