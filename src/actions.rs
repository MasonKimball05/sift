//! Carrying out a decision: moving (and renaming), Finder tags, the Trash.
//!
//! The one rule here: never overwrite. If the destination name is taken, the
//! file gets " (2)", " (3)" and so on, like Finder does.

use crate::rules::Action;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Where a file ended up: a new path, or the Trash.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Moved(PathBuf),
    Trashed,
}

pub fn apply(action: &Action, path: &Path) -> Result<Outcome> {
    match action {
        Action::Move { to, tags } => {
            let moved = move_file(path, to)?;
            if !tags.is_empty() {
                // A file that moved but didn't get its tag is still sorted;
                // report the problem but don't treat the move as failed.
                if let Err(error) = add_tags(&moved, tags) {
                    eprintln!("sift: couldn't tag {}: {error:#}", moved.display());
                }
            }
            Ok(Outcome::Moved(moved))
        }
        Action::Trash => {
            // The `trash` crate asks Finder to do it, so Put Back works.
            trash::delete(path).with_context(|| format!("moving {} to the Trash", path.display()))?;
            Ok(Outcome::Trashed)
        }
    }
}

/// Moves `from` to `to` (or to a free name next to it), making folders as
/// needed, and returns where it really went.
pub fn move_file(from: &Path, to: &Path) -> Result<PathBuf> {
    if from == to {
        return Ok(to.to_path_buf());
    }
    let folder = to.parent().context("destination has no folder")?;
    std::fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    let to = free_name(to);
    // `rename` is instant on the same disk. Across disks (an external drive)
    // the OS refuses with error 18 ("cross-device link"); then copy and delete.
    match std::fs::rename(from, &to) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(18) => {
            std::fs::copy(from, &to).with_context(|| format!("copying to {}", to.display()))?;
            std::fs::remove_file(from)?;
        }
        Err(error) => return Err(error).with_context(|| format!("moving {} to {}", from.display(), to.display())),
    }
    Ok(to)
}

/// `path` if nothing's there, else "name (2).ext", "name (3).ext", ...
pub fn free_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    // `(2..)` is an endless range; `find` takes the first number that's free.
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|candidate| !candidate.exists())
        .expect("some name is free")
}

/// Finder keeps tags in an extended attribute: a binary property list of names.
/// Read what's there, add the new ones, write it back.
pub fn add_tags(path: &Path, tags: &[String]) -> Result<()> {
    const KEY: &str = "com.apple.metadata:_kMDItemUserTags";
    let mut current: Vec<String> = match xattr::get(path, KEY)? {
        Some(bytes) => plist::from_bytes(&bytes).unwrap_or_default(),
        None => vec![],
    };
    for tag in tags {
        // A tag can be stored as "Name" or "Name\n<color number>"; compare the name part.
        if !current.iter().any(|t| t.split('\n').next() == Some(tag.as_str())) {
            current.push(tag.clone());
        }
    }
    let mut bytes = Vec::new();
    plist::to_writer_binary(&mut bytes, &current)?;
    xattr::set(path, KEY, &bytes)?;
    Ok(())
}

pub fn tags(path: &Path) -> Vec<String> {
    xattr::get(path, "com.apple.metadata:_kMDItemUserTags")
        .ok()
        .flatten()
        .and_then(|bytes| plist::from_bytes::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.pdf");
        let second = dir.path().join("b.pdf");
        std::fs::write(&first, "one").unwrap();
        std::fs::write(&second, "two").unwrap();
        let target = dir.path().join("School/COSC 490/Lab.pdf");
        assert_eq!(move_file(&first, &target).unwrap(), target);
        let again = move_file(&second, &target).unwrap();
        assert_eq!(again, dir.path().join("School/COSC 490/Lab (2).pdf"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "one");
        assert!(!first.exists() && !second.exists());
    }

    #[test]
    fn tags_add_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("receipt.pdf");
        std::fs::write(&file, "x").unwrap();
        add_tags(&file, &["Finance".into()]).unwrap();
        add_tags(&file, &["Finance".into(), "2026".into()]).unwrap();
        assert_eq!(tags(&file), vec!["Finance", "2026"]);
    }
}
