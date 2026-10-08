//! The rules file: which folders sift watches, and where each kind of file goes.
//!
//! It lives at `~/.config/sift/rules.toml`. The first time sift runs it writes
//! the starter rules below there, so the file is something you edit, not
//! something you write from scratch.
//!
//! Reading it is almost all done by `serde`: the structs here describe the shape
//! of the TOML, `#[derive(Deserialize)]` generates the code that fills them in,
//! and `#[serde(default)]` lets a rule leave out anything it doesn't need.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The whole rules file.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// "suggest" (the default) only lines moves up for `sift review`; "move" does them.
    #[serde(default)]
    pub mode: Mode,

    /// Folders to watch. Only files directly inside them are sorted, never
    /// anything in their subfolders, so folders you've already organized are safe.
    #[serde(default = "default_watch")]
    pub watch: Vec<String>,

    /// How long a file has to sit unchanged before it's sorted, so a download
    /// isn't moved while it's still being written.
    #[serde(default = "default_settle")]
    pub settle_seconds: u64,

    /// Course codes to look for ("COSC" finds "COSC 490" and "COSC490.01").
    /// Empty means any 3 or 4 capital letters followed by 3 digits.
    #[serde(default)]
    pub course_codes: Vec<String>,

    /// A JSON file of Canvas course numbers and names, {"55120": "COSC 470"}.
    /// Daybook writes it from the Canvas calendar feed. Canvas download links
    /// have the course number in them ("/courses/55120/files/..."), so this is
    /// how a download finds its course even when its name and text don't say.
    #[serde(default = "default_canvas_courses")]
    pub canvas_courses: String,

    /// Canvas courses named by hand, for ones Daybook's list doesn't cover
    /// (past semesters): a `[course_names]` table of "number" = "name".
    /// These win over Daybook's list.
    #[serde(default)]
    pub course_names: HashMap<String, String>,

    /// In TOML each rule is a `[[rule]]` table; serde collects them in order.
    #[serde(rename = "rule", default)]
    pub rules: Vec<Rule>,

    /// The course list read from `canvas_courses`. `#[serde(skip)]`: it isn't in
    /// the TOML; `load` fills it in after.
    #[serde(skip)]
    pub courses: HashMap<String, String>,
}

/// `#[serde(rename_all = "lowercase")]` maps the TOML strings "suggest" and
/// "move" onto these variants.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Suggest,
    Move,
}

/// One rule. Every condition it sets has to hold; conditions it leaves out
/// don't count. Rules are tried top to bottom and the first match wins.
#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub name: String,

    /// `enabled = false` turns a rule off without deleting it (the app's switch).
    #[serde(default = "on")]
    pub enabled: bool,

    // ---- Conditions ----
    /// Only files in these watched folders (empty: any of them).
    #[serde(default)]
    pub folders: Vec<String>,
    /// File extensions, without the dot, any case ("pdf", "dmg").
    #[serde(default)]
    pub extensions: Vec<String>,
    /// A regular expression the file name has to match ("^Screenshot ").
    pub name_matches: Option<String>,
    /// Websites the file was downloaded from. "instructure.com" also matches
    /// "samford.instructure.com".
    #[serde(default)]
    pub from_hosts: Vec<String>,
    /// Words or phrases, any one of which has to be in the file's text (PDFs
    /// and plain text files), ignoring case.
    #[serde(default)]
    pub text_any: Vec<String>,
    /// Only files you haven't opened in this many days.
    pub unused_days: Option<u64>,

    // ---- What to do ----
    /// The folder to move it to. Can use {course}, {year}, {month} and {date}.
    pub to: Option<String>,
    /// A new name, without the extension. Can use {name} (the old name), {date},
    /// {year}, {month} and {course}.
    pub rename: Option<String>,
    /// Finder tags to add.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Move it to the Trash instead (Finder's Put Back still works).
    #[serde(default)]
    pub trash: bool,
}

fn on() -> bool {
    true
}

fn default_watch() -> Vec<String> {
    vec!["~/Downloads".into(), "~/Desktop".into()]
}

fn default_settle() -> u64 {
    5
}

fn default_canvas_courses() -> String {
    "~/Library/Application Support/Daybook/canvas-courses.json".into()
}

/// The starter rules, written to rules.toml the first time. `include_str!`
/// copies the file's text into the program when it's compiled.
pub const STARTER_RULES: &str = include_str!("starter_rules.toml");

impl Config {
    /// `~/.config/sift/rules.toml`
    pub fn path() -> PathBuf {
        home().join(".config/sift/rules.toml")
    }

    /// Reads the rules file, writing the starter one first if there isn't one.
    pub fn load() -> Result<Config> {
        let path = Self::path();
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, STARTER_RULES)?;
            eprintln!("Wrote the starter rules to {}", path.display());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut config = Self::parse(&text).with_context(|| format!("in {}", path.display()))?;
        // No course list (no Daybook, or no Canvas yet) just means no lookups.
        let from_daybook: HashMap<String, String> = std::fs::read_to_string(expand_home(&config.canvas_courses))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        // `extend` adds the hand-named ones on top, replacing any with the same number.
        config.courses = from_daybook;
        config.courses.extend(config.course_names.clone());
        Ok(config)
    }

    /// The rules file's text, for the app's editor.
    pub fn read_text() -> Result<String> {
        Self::load()?; // writes the starter file if there's none yet
        Ok(std::fs::read_to_string(Self::path())?)
    }

    /// Saves edited rules, but only if they parse: a typo shouldn't leave sift
    /// with no rules at all. The error says what's wrong and where.
    pub fn save_text(text: &str) -> Result<()> {
        Self::parse(text)?;
        std::fs::write(Self::path(), text)?;
        Ok(())
    }

    /// Switches between "suggest" and "move" by rewriting just the `mode` line,
    /// so the rest of the file (comments included) stays as written.
    pub fn set_mode(mode: Mode) -> Result<()> {
        Self::save_text(&with_mode(&Self::read_text()?, mode))
    }

    pub fn parse(text: &str) -> Result<Config> {
        let config: Config = toml::from_str(text)?;
        // Catch a bad regular expression now, with the rule's name, rather than
        // the first time a file happens to reach that rule.
        for rule in &config.rules {
            if let Some(pattern) = &rule.name_matches {
                regex::Regex::new(pattern).with_context(|| format!("rule \"{}\": bad name_matches", rule.name))?;
            }
            if rule.to.is_none() && !rule.trash {
                anyhow::bail!("rule \"{}\" needs a `to` folder or `trash = true`", rule.name);
            }
        }
        Ok(config)
    }

    /// The watched folders with `~` filled in.
    pub fn watched(&self) -> Vec<PathBuf> {
        self.watch.iter().map(|folder| expand_home(folder)).collect()
    }
}

/// The rules text with its `mode` line changed (or added at the top).
pub fn with_mode(text: &str, mode: Mode) -> String {
    let word = match mode {
        Mode::Suggest => "suggest",
        Mode::Move => "move",
    };
    // `(?m)` makes ^ and $ match at each line, not just the whole text's ends.
    let line = regex::Regex::new(r#"(?m)^mode\s*=.*$"#).expect("mode pattern");
    if line.is_match(text) {
        line.replace(text, format!("mode = \"{word}\"")).into_owned()
    } else {
        format!("mode = \"{word}\"\n{text}")
    }
}

pub fn home() -> PathBuf {
    dirs::home_dir().expect("no home folder")
}

/// "~/Documents" -> "/Users/you/Documents"
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}

/// The reverse, for printing: "/Users/you/Documents" -> "~/Documents".
pub fn tilde(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_rules_parse() {
        let config = Config::parse(STARTER_RULES).unwrap();
        assert_eq!(config.mode, Mode::Suggest);
        assert!(config.rules.len() >= 5);
        assert_eq!(config.watch, vec!["~/Downloads", "~/Desktop"]);
    }

    #[test]
    fn switching_mode_keeps_the_rest() {
        let moved = with_mode(STARTER_RULES, Mode::Move);
        assert_eq!(Config::parse(&moved).unwrap().mode, Mode::Move);
        assert!(moved.contains("# sift's rules")); // comments survive
        assert_eq!(Config::parse(&with_mode(&moved, Mode::Suggest)).unwrap().mode, Mode::Suggest);
        assert!(with_mode("watch = []\n", Mode::Move).starts_with("mode = \"move\"\n"));
    }

    #[test]
    fn courses_named_by_hand() {
        let config = Config::parse("[course_names]\n\"52266\" = \"COSC 360\"\n").unwrap();
        assert_eq!(config.course_names.get("52266").map(String::as_str), Some("COSC 360"));
    }

    #[test]
    fn rules_need_somewhere_to_go() {
        let error = Config::parse("[[rule]]\nname = \"Lost\"\nextensions = [\"pdf\"]\n").unwrap_err();
        assert!(format!("{error:#}").contains("needs a `to` folder"));
    }

    #[test]
    fn bad_patterns_name_their_rule() {
        let error = Config::parse("[[rule]]\nname = \"Broken\"\nname_matches = \"(\"\nto = \"~/x\"\n").unwrap_err();
        assert!(format!("{error:#}").contains("rule \"Broken\""));
    }
}
