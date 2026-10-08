//! Changing the rules from the app's Where Files Go page: a rule's folder, turning
//! a rule on or off, adding and removing rules.
//!
//! `config.rs` reads the rules with serde, which is all that reading needs, but
//! writing them back that way would lose every comment and blank line. The
//! `toml_edit` crate edits the file as a document instead: change one value and
//! everything else, comments included, stays exactly as it was.
//!
//! Each change is a pure function from the old text to the new (`with_*` below),
//! so the tests can check it on a string; the public functions read the rules
//! file, apply one, and save through `Config::save_text`, which refuses anything
//! that doesn't parse.

use crate::config::{tilde, Config, Rule};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

/// File kinds to pick from when adding a rule, each a list of extensions.
pub const KINDS: &[(&str, &[&str])] = &[
    ("Images", &["png", "jpg", "jpeg", "heic", "gif", "webp", "tiff", "bmp", "svg"]),
    ("PDFs", &["pdf"]),
    ("Documents", &["doc", "docx", "pages", "txt", "rtf", "md", "odt"]),
    ("Spreadsheets", &["xls", "xlsx", "csv", "numbers"]),
    ("Presentations", &["ppt", "pptx", "key"]),
    ("Videos", &["mov", "mp4", "m4v", "avi", "mkv", "webm"]),
    ("Audio", &["mp3", "m4a", "wav", "aac", "flac"]),
    ("Archives", &["zip", "rar", "7z", "tar", "gz"]),
    ("Installers", &["dmg", "pkg"]),
    ("Code", &["py", "js", "ts", "swift", "rs", "java", "c", "cpp", "h", "html", "css", "json", "ipynb"]),
];

/// A rule as the Where Files Go page shows it.
#[derive(Debug, Serialize)]
pub struct RuleInfo {
    pub name: String,
    /// What it matches, in words: "PDFs with “receipt”, “invoice” and more".
    pub summary: String,
    /// The folder part of `to`, without the {placeholders}: "~/Pictures/Screenshots".
    pub folder: String,
    /// The {placeholder} part, if any: "{year}-{month}".
    pub subfolders: String,
    pub trash: bool,
    pub enabled: bool,
}

/// Splits a `to` template where the placeholders start:
/// "~/Pictures/Screenshots/{year}-{month}" -> ("~/Pictures/Screenshots", "{year}-{month}").
pub fn split_destination(to: &str) -> (String, String) {
    let parts: Vec<&str> = to.split('/').collect();
    match parts.iter().position(|part| part.contains('{')) {
        Some(at) => (parts[..at].join("/"), parts[at..].join("/")),
        None => (to.to_string(), String::new()),
    }
}

fn join(folder: &str, subfolders: &str) -> String {
    let folder = folder.trim_end_matches('/');
    if subfolders.is_empty() { folder.to_string() } else { format!("{folder}/{subfolders}") }
}

/// "Images from Downloads", "PDFs with “receipt”, “invoice” and more", "Unused 30+ days, in Downloads".
pub fn summary(rule: &Rule) -> String {
    let mut parts = vec![];
    if !rule.extensions.is_empty() {
        // A named kind if the extensions are exactly one of ours, else the list.
        let mut wanted: Vec<String> = rule.extensions.iter().map(|e| e.to_lowercase()).collect();
        wanted.sort();
        let kind = KINDS.iter().find(|(_, exts)| {
            let mut list: Vec<String> = exts.iter().map(|e| e.to_string()).collect();
            list.sort();
            list == wanted
        });
        parts.push(match kind {
            Some((name, _)) => name.to_string(),
            None => rule.extensions.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", ") + " files",
        });
    }
    if let Some(pattern) = &rule.name_matches {
        parts.push(format!("named like /{pattern}/"));
    }
    if !rule.from_hosts.is_empty() {
        parts.push(format!("downloaded from {}", rule.from_hosts.join(" or ")));
    }
    if !rule.text_any.is_empty() {
        let shown: Vec<String> = rule.text_any.iter().take(2).map(|t| format!("\u{201C}{t}\u{201D}")).collect();
        let more = if rule.text_any.len() > 2 { " and more" } else { "" };
        parts.push(format!("with {}{more}", shown.join(", ")));
    }
    if let Some(days) = rule.unused_days {
        parts.push(format!("unused {days}+ days"));
    }
    if !rule.folders.is_empty() {
        let names: Vec<String> = rule.folders.iter().map(|f| f.trim_start_matches("~/").to_string()).collect();
        parts.push(format!("in {}", names.join(" or ")));
    }
    if parts.is_empty() { "Every file".into() } else { parts.join(", ") }
}

pub fn rules(config: &Config) -> Vec<RuleInfo> {
    config
        .rules
        .iter()
        .map(|rule| {
            let (folder, subfolders) = split_destination(rule.to.as_deref().unwrap_or(""));
            RuleInfo { name: rule.name.clone(), summary: summary(rule), folder, subfolders, trash: rule.trash, enabled: rule.enabled }
        })
        .collect()
}

/// A rule made on the Where Files Go page.
#[derive(Debug, Deserialize)]
pub struct NewRule {
    pub name: String,
    /// Names from KINDS ("Images", "PDFs").
    pub kinds: Vec<String>,
    /// "~/Downloads", "~/Desktop", or "" for either.
    pub from: String,
    pub folder: String,
    /// "", "{year}", "{year}-{month}" or "{course}".
    pub subfolders: String,
}

// ---- The edits, on text ----

fn parse(text: &str) -> Result<DocumentMut> {
    Ok(text.parse::<DocumentMut>()?)
}

fn rule_tables(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    if doc.get("rule").is_none() {
        doc.insert("rule", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    doc["rule"].as_array_of_tables_mut().ok_or_else(|| anyhow!("`rule` in the rules file isn't a list of [[rule]] tables"))
}

fn find<'a>(rules: &'a mut ArrayOfTables, name: &str) -> Result<&'a mut Table> {
    rules
        .iter_mut()
        .find(|table| table.get("name").and_then(|n| n.as_str()) == Some(name))
        .ok_or_else(|| anyhow!("there's no rule named \u{201C}{name}\u{201D}"))
}

/// Sends a rule's files to `folder` (with optional {placeholder} subfolders).
pub fn with_destination(text: &str, name: &str, folder: &str, subfolders: &str) -> Result<String> {
    let mut doc = parse(text)?;
    let rule = find(rule_tables(&mut doc)?, name)?;
    if rule.get("trash").and_then(|t| t.as_bool()) == Some(true) {
        bail!("\u{201C}{name}\u{201D} sends files to the Trash, so it has no folder");
    }
    rule["to"] = value(join(folder, subfolders));
    Ok(doc.to_string())
}

pub fn with_enabled(text: &str, name: &str, on: bool) -> Result<String> {
    let mut doc = parse(text)?;
    let rule = find(rule_tables(&mut doc)?, name)?;
    if on {
        rule.remove("enabled"); // on is the default, so just drop the line
    } else {
        rule["enabled"] = value(false);
    }
    Ok(doc.to_string())
}

pub fn without(text: &str, name: &str) -> Result<String> {
    let mut doc = parse(text)?;
    let rules = rule_tables(&mut doc)?;
    let at = rules
        .iter()
        .position(|table| table.get("name").and_then(|n| n.as_str()) == Some(name))
        .ok_or_else(|| anyhow!("there's no rule named \u{201C}{name}\u{201D}"))?;
    rules.remove(at);
    Ok(doc.to_string())
}

/// Adds a rule just above the first Trash rule, so "old downloads to the
/// Trash" stays the last thing tried.
pub fn with_new_rule(text: &str, new: &NewRule) -> Result<String> {
    let name = new.name.trim();
    if name.is_empty() {
        bail!("give the rule a name");
    }
    let mut extensions = vec![];
    for kind in &new.kinds {
        let (_, exts) = KINDS.iter().find(|(k, _)| k == kind).ok_or_else(|| anyhow!("unknown file kind {kind}"))?;
        extensions.extend(exts.iter().map(|e| e.to_string()));
    }
    if extensions.is_empty() {
        bail!("pick at least one kind of file");
    }
    if new.folder.trim().is_empty() {
        bail!("pick a folder for these files");
    }

    let mut doc = parse(text)?;
    let rules = rule_tables(&mut doc)?;
    if rules.iter().any(|table| table.get("name").and_then(|n| n.as_str()) == Some(name)) {
        bail!("there's already a rule named \u{201C}{name}\u{201D}");
    }
    let mut table = Table::new();
    table["name"] = value(name);
    if !new.from.is_empty() {
        table["folders"] = value(Array::from_iter([new.from.as_str()]));
    }
    table["extensions"] = value(Array::from_iter(extensions.iter().map(String::as_str)));
    table["to"] = value(join(&new.folder, &new.subfolders));

    // ArrayOfTables has no "insert at", so take the tables out, place the new
    // one, and put them all back. Each keeps its comments (`decor`) as it goes.
    let mut tables: Vec<Table> = rules.iter().cloned().collect();
    let at = tables.iter().position(|t| t.get("trash").and_then(|v| v.as_bool()) == Some(true)).unwrap_or(tables.len());
    tables.insert(at, table);
    rules.clear();
    for table in tables {
        rules.push(table);
    }
    Ok(doc.to_string())
}

// ---- The same, on the real rules file ----

fn change(edit: impl FnOnce(&str) -> Result<String>) -> Result<()> {
    let text = Config::read_text()?;
    Config::save_text(&edit(&text)?)
}

pub fn set_destination(name: &str, folder: &std::path::Path, subfolders: &str) -> Result<()> {
    change(|text| with_destination(text, name, &tilde(folder), subfolders))
}

pub fn set_enabled(name: &str, on: bool) -> Result<()> {
    change(|text| with_enabled(text, name, on))
}

pub fn remove(name: &str) -> Result<()> {
    change(|text| without(text, name))
}

pub fn add(new: &NewRule) -> Result<()> {
    change(|text| with_new_rule(text, new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::STARTER_RULES;

    fn rule<'a>(config: &'a Config, name: &str) -> &'a Rule {
        config.rules.iter().find(|r| r.name == name).unwrap()
    }

    #[test]
    fn splits_folders_from_placeholders() {
        assert_eq!(split_destination("~/Pictures/Screenshots/{year}-{month}"), ("~/Pictures/Screenshots".into(), "{year}-{month}".into()));
        assert_eq!(split_destination("~/Downloads/To Install"), ("~/Downloads/To Install".into(), String::new()));
    }

    #[test]
    fn moving_a_rules_folder_keeps_its_subfolders_and_the_comments() {
        let text = with_destination(STARTER_RULES, "Screenshots", "~/Desktop/Pictures", "{year}-{month}").unwrap();
        let config = Config::parse(&text).unwrap();
        assert_eq!(rule(&config, "Screenshots").to.as_deref(), Some("~/Desktop/Pictures/{year}-{month}"));
        assert!(text.contains("# sift's rules")); // comments survive
        assert!(text.contains("# Last: downloads you haven't opened"));
        assert!(with_destination(STARTER_RULES, "Old downloads", "~/x", "").is_err()); // a Trash rule has no folder
    }

    #[test]
    fn turning_off_and_on() {
        let off = with_enabled(STARTER_RULES, "Receipts", false).unwrap();
        assert!(!rule(&Config::parse(&off).unwrap(), "Receipts").enabled);
        let on = with_enabled(&off, "Receipts", true).unwrap();
        assert!(rule(&Config::parse(&on).unwrap(), "Receipts").enabled);
        assert!(!on.contains("enabled"));
    }

    #[test]
    fn new_rules_go_above_the_trash_rule() {
        let new = NewRule { name: "Pictures".into(), kinds: vec!["Images".into()], from: "~/Downloads".into(),
                            folder: "~/Desktop/Pictures".into(), subfolders: String::new() };
        let config = Config::parse(&with_new_rule(STARTER_RULES, &new).unwrap()).unwrap();
        let names: Vec<&str> = config.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.last(), Some(&"Old downloads"));
        assert_eq!(names[names.len() - 2], "Pictures");
        let pictures = rule(&config, "Pictures");
        assert!(pictures.extensions.contains(&"heic".to_string()));
        assert_eq!(summary(pictures), "Images, in Downloads");
        assert!(with_new_rule(STARTER_RULES, &NewRule { kinds: vec![], ..new }).is_err());
    }

    #[test]
    fn removing() {
        let config = Config::parse(&without(STARTER_RULES, "Installers").unwrap()).unwrap();
        assert!(config.rules.iter().all(|r| r.name != "Installers"));
        assert!(without(STARTER_RULES, "Nope").is_err());
    }
}
