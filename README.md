# sift

[![Tests](https://github.com/MasonKimball05/sift/actions/workflows/tests.yml/badge.svg)](https://github.com/MasonKimball05/sift/actions/workflows/tests.yml)

Sorts new files in Downloads and Desktop into the right folders, on macOS.
A Canvas download lands in `School/COSC 490`, a receipt in `Finance/2026` with
the date on its name, a screenshot in `Pictures/Screenshots/2026-10`, and
downloads you haven't opened in a month go to the Trash.

It decides from rules you can read and edit, and it starts out only
*suggesting*: nothing moves until you approve it, unless you switch it to just move.

## Using it

```bash
make app                # builds sift.app into ~/Applications
make install            # the command line: puts `sift` in ~/.local/bin
sift run --dry-run      # what it would do with what's there now
sift install            # watch in the background, from login on
```

| Command | What it does |
|---|---|
| `sift run [--dry-run]` | Sort what's in the watched folders now |
| `sift review` | Approve or skip the moves waiting (suggest mode) |
| `sift explain <file>` | What sift knows about a file, and why each rule does or doesn't match |
| `sift undo [n]` | Put back the last move (or the last n) |
| `sift history` | Recent moves |
| `sift rules` | Where the rules file is |
| `sift install` / `uninstall` | Start or stop the background watcher |

The rules are in `~/.config/sift/rules.toml`, written with starter rules the
first time sift runs. Each rule has conditions (which folder, file type, name
pattern, the website it came from, words in its text, how long since it was
opened) and an action (a folder, a new name, Finder tags, or the Trash). The
first rule that matches wins. Once `sift review` has looked right for a while,
change `mode = "suggest"` to `mode = "move"`.

## The app

`make app` builds **sift.app** (with [Tauri](https://tauri.app)) into ~/Applications:

- **Review**: files waiting for your OK, grouped by rule, with Move / Leave and Move All.
- **Activity**: everything moved, with Undo on each.
- **Where Files Go**: each rule's folder, with Choose Folder… and subfolders (by year,
  month or course); turn rules on and off; add a rule by picking file kinds (Images,
  PDFs, Videos…), where they come from, and a folder. Edits keep the rules file's
  comments (see `src/settings.rs` and `toml_edit`).
- **Rules File**: the rules file itself, checked before it's saved (a typo can't break sorting).
- **Test a File**: drop a file to see what sift knows about it and each rule's verdict;
  a Canvas download with an unnamed course can be named right there.
- In the sidebar: ask first or just move, and watch in the background from login on.
  The background watcher is the app itself, started with `watch`, so macOS's permission
  prompt names sift.

The app is two parts: `app/src-tauri/src/main.rs` (Rust: each `#[tauri::command]`
is a thin wrapper around the library) and `app/ui/` (the page: plain HTML, CSS and
JavaScript, no framework, calling those commands with `invoke`).

## Reading the code

It's written to be read. A good order:

1. **`src/lib.rs`**: the map of the library, and why it's split from `main.rs`.
2. **`src/engine.rs`**: the path every file takes (`plan`, `sort_one`, `approve`),
   returning data that the command line and the app each show their own way.
3. **`src/main.rs`**: the command line (clap turns the `Command` enum into it).
4. **`src/config.rs`**: the rules file. serde fills the structs from TOML;
   note how `#[serde(default)]` makes every condition optional.
5. **`src/facts.rs`**: what's known about a file. Look at `OnceCell` (the PDF
   text is read only when a rule needs it), the extended attributes behind
   "where was this downloaded from", and `catch_unwind` around the PDF reader.
6. **`src/rules.rs`**: matching. `check` returns `Result<(), String>` so the
   same code answers both "does it match?" and "why not?" for `sift explain`.
7. **`src/actions.rs`**: moving without overwriting (`free_name` and its
   endless `2..` range), and Finder tags as a binary property list.
8. **`src/history.rs`**: the undo log, one JSON object per line.
9. **`src/watch.rs`**: file events over a channel, debounced so half-finished
   downloads aren't touched.
10. **`src/launchd.rs`**: starting at login with a LaunchAgent.
11. **`src/settings.rs`**: editing the rules file without losing its comments,
    each edit a pure function from old text to new.

Every module has tests at the bottom (`#[cfg(test)] mod tests`); `make test`
runs them and `make lint` runs Clippy.

## Safety

- Only files directly in the watched folders, never their subfolders.
- Never overwrites: a taken name becomes "name (2)".
- Skips hidden files and downloads still in progress.
- Every move is logged, and `sift undo` reverses it. The Trash is Finder's, so
  Put Back works.

Building the app needs the Tauri command line once: `cargo install tauri-cli --version "^2" --locked`.
