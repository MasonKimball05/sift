//! Everything sift learns about a file before deciding where it goes.
//!
//! Most facts are cheap (the name, the dates), so they're read up front. The
//! text inside a PDF is not, so it's read the first time a rule asks for it and
//! kept after that: see `text()` and `OnceCell` below.

use chrono::{DateTime, Local};
use regex::Regex;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Facts {
    /// The file itself.
    pub path: PathBuf,
    /// The folder it's in (one of the watched ones).
    pub folder: PathBuf,
    /// Its name without the extension: "Syllabus" for "Syllabus.pdf".
    pub stem: String,
    /// The extension, lowercased, without the dot ("pdf"), or "" for none.
    pub ext: String,
    /// When it was made (for a download, when it arrived).
    pub created: DateTime<Local>,
    /// When it was last opened, if macOS knows.
    pub last_used: Option<DateTime<Local>>,
    /// The links it was downloaded from (the page and the file).
    pub urls: Vec<String>,
    /// Just the websites of those links ("samford.instructure.com").
    pub hosts: Vec<String>,
    /// The file's text, read on first use. `OnceCell` is a slot that starts
    /// empty and can be filled exactly once; `get_or_init` fills it the first
    /// time and hands back the same value after that, without needing `&mut self`.
    text: OnceCell<String>,
}

impl Facts {
    /// Gathers the facts about a file. Fails only if the file can't be read at all.
    pub fn of(path: &Path) -> std::io::Result<Facts> {
        let metadata = std::fs::metadata(path)?;
        // Not every file system records a creation time; fall back to the last change.
        let created = metadata.created().or_else(|_| metadata.modified())?;
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let urls = where_froms(path);
        Ok(Facts {
            path: path.to_path_buf(),
            folder: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            stem: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            ext,
            created: created.into(),
            last_used: last_used(path),
            hosts: urls.iter().filter_map(|url| host(url)).collect(),
            urls,
            text: OnceCell::new(),
        })
    }

    /// "Syllabus.pdf"
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// The text inside the file (PDFs, Word and text files), at most 20,000
    /// characters. Read once, the first time it's asked for.
    pub fn text(&self) -> &str {
        self.text.get_or_init(|| read_text(&self.path, &self.ext))
    }

    /// Days since it was last opened, or since it arrived if it never was.
    pub fn days_unused(&self, now: DateTime<Local>) -> i64 {
        let since = self.last_used.unwrap_or(self.created);
        (now - since).num_days()
    }

    /// The file's course, "COSC 490". Checked in order of how sure the answer is
    /// and how cheap it is to get:
    ///   1. a Canvas download's course number, looked up in `courses`
    ///   2. a course code in the file's name ("cosc490_syllabus.pdf")
    ///   3. a course code in its text (the slow one)
    pub fn course(&self, codes: &[String], courses: &HashMap<String, String>) -> Option<String> {
        if let Some(name) = self.urls.iter().filter_map(|url| canvas_course_number(url)).find_map(|n| courses.get(&n)) {
            return Some(name.clone());
        }
        let pattern = course_pattern(codes);
        find_course(&pattern, &self.stem.to_uppercase()).or_else(|| find_course(&pattern, self.text()))
    }

    /// For tests: facts about a made-up file, with its text already known.
    #[cfg(test)]
    pub fn sample(path: &str, text: &str) -> Facts {
        let path = PathBuf::from(path);
        let facts = Facts {
            folder: path.parent().unwrap().to_path_buf(),
            stem: path.file_stem().unwrap().to_string_lossy().into_owned(),
            ext: path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
            created: Local::now(),
            last_used: None,
            urls: vec![],
            hosts: vec![],
            text: OnceCell::new(),
            path,
        };
        facts.text.set(text.to_string()).unwrap();
        facts
    }
}

/// The regex for course codes: the listed prefixes, or any 3 or 4 capitals,
/// then an optional space, dash or underscore, then 3 digits.
///
/// It can't use `\b` (a word boundary) at the ends: to a regex `_` is part of a
/// word, so "COSC490_syllabus" has no boundary after the 490. Instead it says
/// directly what may come before and after: the start or end of the text, or
/// anything that isn't a letter or digit. `(?:...)` groups without capturing,
/// so the letters and digits are still captures 1 and 2.
fn course_pattern(codes: &[String]) -> Regex {
    let prefix = if codes.is_empty() {
        "[A-Z]{3,4}".to_string()
    } else {
        // regex::escape so a code can't accidentally be read as regex syntax.
        codes.iter().map(|c| regex::escape(&c.to_uppercase())).collect::<Vec<_>>().join("|")
    };
    Regex::new(&format!(r"(?:^|[^A-Za-z0-9])({prefix})[\s_-]?(\d{{3}})(?:[^0-9]|$)")).expect("course pattern")
}

fn find_course(pattern: &Regex, text: &str) -> Option<String> {
    // `captures` gives the pieces in parentheses: 1 is the letters, 2 the digits.
    let found = pattern.captures(text)?;
    Some(format!("{} {}", &found[1], &found[2]))
}

/// The course number in a Canvas link: ".../courses/55120/files/9" -> "55120".
pub fn canvas_course_number(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("/courses/")?;
    let number: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!number.is_empty()).then_some(number)
}

/// The host of a URL: "https://samford.instructure.com/files/12" -> "samford.instructure.com".
/// Done by hand rather than with a URL crate: split off the scheme, then cut at
/// the first character that ends a host.
pub fn host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#', ':']).next()?.to_lowercase();
    // "user@host" style URLs: keep what's after the @.
    let host = host.rsplit('@').next().unwrap_or(&host).to_string();
    (!host.is_empty()).then_some(host)
}

/// Where a download came from. macOS keeps the page and file URLs in an
/// extended attribute (extra data stored with the file, outside its contents),
/// written as a binary property list of strings.
fn where_froms(path: &Path) -> Vec<String> {
    let Ok(Some(bytes)) = xattr::get(path, "com.apple.metadata:kMDItemWhereFroms") else {
        return vec![];
    };
    plist::from_bytes(&bytes).unwrap_or_default()
}

/// When the file was last opened, from Spotlight's `mdls` tool. It prints a
/// date like "2026-10-01 14:22:10 +0000", or "(null)" if it's never been opened.
fn last_used(path: &Path) -> Option<DateTime<Local>> {
    let output = Command::new("mdls").args(["-raw", "-name", "kMDItemLastUsedDate"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    DateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S %z").ok().map(|date| date.with_timezone(&Local))
}

const TEXT_LIMIT: usize = 20_000;

fn read_text(path: &Path, ext: &str) -> String {
    let text = match ext {
        "pdf" => pdf_text(path),
        // macOS's own `textutil` converts Word, RTF and web pages to plain text.
        "docx" | "doc" | "rtf" | "odt" | "html" | "webarchive" => Command::new("textutil")
            .args(["-convert", "txt", "-stdout"])
            .arg(path)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default(),
        "txt" | "md" | "csv" => std::fs::read_to_string(path).unwrap_or_default(),
        _ => String::new(),
    };
    text.chars().take(TEXT_LIMIT).collect()
}

/// PDF text. The `pdf-extract` crate can panic on unusual PDFs instead of
/// returning an error, so the call is wrapped in `catch_unwind`, which turns a
/// panic back into an ordinary `Err`: one odd file shouldn't stop the sorter.
/// The panic hook (which prints the panic message) is silenced for the call.
fn pdf_text(path: &Path) -> String {
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| pdf_extract::extract_text(path));
    std::panic::set_hook(quiet);
    match result {
        Ok(Ok(text)) => text,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host("https://samford.instructure.com/files/12/download?x=1").as_deref(), Some("samford.instructure.com"));
        assert_eq!(host("https://Example.com:8443/path").as_deref(), Some("example.com"));
        assert_eq!(host("not a url"), None);
    }

    #[test]
    fn canvas_links_name_their_course() {
        assert_eq!(canvas_course_number("https://samford.instructure.com/courses/55120/files/9?wrap=1").as_deref(), Some("55120"));
        assert_eq!(canvas_course_number("https://samford.instructure.com/calendar"), None);
        let mut facts = Facts::sample("/tmp/Lecture 7.pptx", "");
        facts.urls = vec!["https://samford.instructure.com/courses/55120/files/9".into()];
        let courses = HashMap::from([("55120".to_string(), "COSC 470".to_string())]);
        assert_eq!(facts.course(&[], &courses).as_deref(), Some("COSC 470"));
        assert_eq!(facts.course(&[], &HashMap::new()), None); // no list: no answer from the link
    }

    #[test]
    fn courses_from_names_and_text() {
        let any: Vec<String> = vec![];
        let none = HashMap::new();
        let named = Facts::sample("/tmp/cosc490_syllabus.pdf", "");
        assert_eq!(named.course(&any, &none).as_deref(), Some("COSC 490"));
        assert_eq!(Facts::sample("/tmp/HW3_COSC_470.pdf", "").course(&any, &none).as_deref(), Some("COSC 470"));
        let inside = Facts::sample("/tmp/Syllabus.pdf", "Fall 2026\nMATH-260.02 Linear Algebra\nPrereq: MATH 160");
        assert_eq!(inside.course(&any, &none).as_deref(), Some("MATH 260"));
        // Limited to listed codes: "PAGE 123" isn't a course.
        let listed = vec!["COSC".to_string()];
        assert_eq!(Facts::sample("/tmp/a.pdf", "PAGE 123 of COSC 470").course(&listed, &none).as_deref(), Some("COSC 470"));
        assert_eq!(Facts::sample("/tmp/a.pdf", "a 2026 receipt").course(&any, &none), None);
    }

    #[test]
    fn reads_plain_text_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "Office hours: Tue 2pm").unwrap();
        let facts = Facts::of(&path).unwrap();
        assert_eq!(facts.ext, "txt");
        assert!(facts.text().contains("Office hours"));
        assert!(facts.hosts.is_empty());
    }
}
