//! Append-only command history.
//!
//! File format, one entry per line, fields separated by TAB:
//! `timestamp \t duration_ms \t exit_code \t cwd \t command`
//! where `cwd` and `command` escape `\` as `\\`, newline as `\n`, CR as `\r`
//! and TAB as `\t`, so a multi-line command stays on one line.

use std::{
  fs::{self, OpenOptions},
  io::{self, Write},
  path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
  /// Seconds since the unix epoch.
  pub timestamp:   u64,
  pub cwd:         String,
  pub exit_code:   i32,
  pub duration_ms: u64,
  pub command:     String,
}

pub struct History {
  path:    PathBuf,
  entries: Vec<HistoryEntry>,
}

impl History {
  /// `$XDG_DATA_HOME/be/shell_history`, or `~/.local/share/be/shell_history`.
  pub fn default_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
      Some(p) if !p.is_empty() => PathBuf::from(p),
      _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(base.join("be/shell_history"))
  }

  /// Loads the history at `path`. A missing file is an empty history.
  /// Malformed lines are skipped.
  pub fn open(path: impl Into<PathBuf>) -> io::Result<History> {
    let path = path.into();
    let entries = match fs::read(&path) {
      Ok(bytes) => parse_file(&String::from_utf8_lossy(&bytes)),
      Err(e) if e.kind() == io::ErrorKind::NotFound => vec![],
      Err(e) => return Err(e),
    };
    Ok(History { path, entries })
  }

  pub fn path(&self) -> &Path { &self.path }

  /// Oldest first.
  pub fn entries(&self) -> &[HistoryEntry] { &self.entries }

  pub fn len(&self) -> usize { self.entries.len() }

  pub fn is_empty(&self) -> bool { self.entries.is_empty() }

  /// Appends to the file and to memory.
  pub fn append(&mut self, entry: HistoryEntry) -> io::Result<()> {
    self.write_all(std::slice::from_ref(&entry))?;
    self.entries.push(entry);
    Ok(())
  }

  fn write_all(&self, entries: &[HistoryEntry]) -> io::Result<()> {
    if let Some(dir) = self.path.parent() {
      fs::create_dir_all(dir)?;
    }
    let mut buf = String::new();
    for e in entries {
      buf.push_str(&format_entry(e));
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
    f.write_all(buf.as_bytes())
  }

  /// Imports a zsh extended history file (`~/.zsh_history`), appending its
  /// entries before anything already present is *not* attempted: they are
  /// appended after the existing entries. Returns the number imported.
  pub fn import_zsh(&mut self, zsh_history: impl AsRef<Path>) -> io::Result<usize> {
    let bytes = fs::read(zsh_history)?;
    let imported = parse_zsh_history(&bytes);
    self.write_all(&imported)?;
    let n = imported.len();
    self.entries.extend(imported);
    Ok(n)
  }

  /// Entries whose command starts with `prefix`, newest first.
  pub fn search_prefix(&self, prefix: &str, dedupe: bool) -> Vec<&HistoryEntry> {
    self.search(|c| c.starts_with(prefix), dedupe)
  }

  /// Entries whose command contains `needle`, newest first.
  pub fn search_substring(&self, needle: &str, dedupe: bool) -> Vec<&HistoryEntry> {
    self.search(|c| c.contains(needle), dedupe)
  }

  /// All entries, newest first.
  pub fn recent(&self, dedupe: bool) -> Vec<&HistoryEntry> { self.search(|_| true, dedupe) }

  fn search(&self, f: impl Fn(&str) -> bool, dedupe: bool) -> Vec<&HistoryEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for e in self.entries.iter().rev() {
      if !f(&e.command) {
        continue;
      }
      if dedupe && !seen.insert(e.command.as_str()) {
        continue;
      }
      out.push(e);
    }
    out
  }
}

fn escape(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for c in s.chars() {
    match c {
      '\\' => out.push_str("\\\\"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      c => out.push(c),
    }
  }
  out
}

fn unescape(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut it = s.chars();
  while let Some(c) = it.next() {
    if c != '\\' {
      out.push(c);
      continue;
    }
    match it.next() {
      Some('n') => out.push('\n'),
      Some('r') => out.push('\r'),
      Some('t') => out.push('\t'),
      Some('\\') => out.push('\\'),
      Some(o) => {
        out.push('\\');
        out.push(o);
      }
      None => out.push('\\'),
    }
  }
  out
}

pub fn format_entry(e: &HistoryEntry) -> String {
  format!(
    "{}\t{}\t{}\t{}\t{}\n",
    e.timestamp,
    e.duration_ms,
    e.exit_code,
    escape(&e.cwd),
    escape(&e.command)
  )
}

fn parse_line(line: &str) -> Option<HistoryEntry> {
  let mut parts = line.splitn(5, '\t');
  let timestamp = parts.next()?.parse().ok()?;
  let duration_ms = parts.next()?.parse().ok()?;
  let exit_code = parts.next()?.parse().ok()?;
  let cwd = unescape(parts.next()?);
  let command = unescape(parts.next()?);
  Some(HistoryEntry { timestamp, cwd, exit_code, duration_ms, command })
}

pub fn parse_file(text: &str) -> Vec<HistoryEntry> { text.lines().filter_map(parse_line).collect() }

/// Reverses zsh's metafication: byte `0x83` (Meta) means "xor the next byte
/// with 0x20".
pub fn unmetafy(bytes: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(bytes.len());
  let mut it = bytes.iter();
  while let Some(&b) = it.next() {
    if b == 0x83 {
      if let Some(&n) = it.next() {
        out.push(n ^ 0x20);
      }
    } else {
      out.push(b);
    }
  }
  out
}

/// Parses `~/.zsh_history`. Handles the extended format (`: ts:dur;cmd`),
/// plain lines, backslash-continued multi-line commands, and metafied bytes.
/// Imported entries have no cwd and exit code 0; zsh's duration (seconds) is
/// converted to milliseconds.
pub fn parse_zsh_history(bytes: &[u8]) -> Vec<HistoryEntry> {
  let bytes = unmetafy(bytes);
  let text = String::from_utf8_lossy(&bytes);
  let mut out = vec![];
  let mut lines = text.split('\n').peekable();
  while let Some(first) = lines.next() {
    if first.is_empty() && lines.peek().is_none() {
      break;
    }
    // Join continuation lines: an odd number of trailing backslashes escapes
    // the newline.
    let mut cmd_text = first.to_string();
    while trailing_backslashes(&cmd_text) % 2 == 1 {
      let Some(next) = lines.next() else { break };
      cmd_text.pop();
      cmd_text.push('\n');
      cmd_text.push_str(next);
    }
    let (timestamp, duration, command) = match parse_extended_prefix(&cmd_text) {
      Some((ts, dur, rest)) => (ts, dur, rest.to_string()),
      None => (0, 0, cmd_text.clone()),
    };
    if command.trim().is_empty() {
      continue;
    }
    out.push(HistoryEntry {
      timestamp,
      cwd: String::new(),
      exit_code: 0,
      duration_ms: duration * 1000,
      command,
    });
  }
  out
}

fn trailing_backslashes(s: &str) -> usize { s.bytes().rev().take_while(|b| *b == b'\\').count() }

/// Parses `: 1700000000:3;rest`.
fn parse_extended_prefix(s: &str) -> Option<(u64, u64, &str)> {
  let s = s.strip_prefix(": ")?;
  let (meta, rest) = s.split_once(';')?;
  let (ts, dur) = meta.split_once(':')?;
  Some((ts.trim().parse().ok()?, dur.trim().parse().ok()?, rest))
}

#[cfg(test)]
mod tests {
  use super::*;
  use expect_test::expect;

  fn entry(ts: u64, cmd: &str) -> HistoryEntry {
    HistoryEntry {
      timestamp:   ts,
      cwd:         "/home/x".into(),
      exit_code:   0,
      duration_ms: 5,
      command:     cmd.into(),
    }
  }

  fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("be-shell-hist-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
  }

  #[test]
  fn escape_roundtrip() {
    for s in ["plain", "a\nb", "tab\there", "back\\slash", "\\n literal", "end\\", "cr\r\n", ""] {
      assert_eq!(unescape(&escape(s)), s, "{s:?}");
    }
  }

  #[test]
  fn format_is_one_line() {
    let e = entry(1, "echo a\necho b\t\\");
    expect![@r#"
        "1\t5\t0\t/home/x\techo a\\necho b\\t\\\\\n"
    "#]
    .assert_debug_eq(&format_entry(&e));
    assert_eq!(parse_line(format_entry(&e).trim_end_matches('\n')), Some(e));
  }

  #[test]
  fn append_and_reopen() {
    let dir = tmp("append");
    let path = dir.join("sub/shell_history");
    let mut h = History::open(&path).unwrap();
    assert!(h.is_empty());
    h.append(entry(1, "ls")).unwrap();
    h.append(entry(2, "echo 'a\nb'")).unwrap();
    let h2 = History::open(&path).unwrap();
    assert_eq!(h2.entries(), h.entries());
    assert_eq!(h2.len(), 2);
    let _ = fs::remove_dir_all(dir);
  }

  #[test]
  fn malformed_lines_skipped() {
    let entries = parse_file("garbage\n1\t5\t0\t/x\tls\nx\ty\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "ls");
  }

  #[test]
  fn search() {
    let dir = tmp("search");
    let mut h = History::open(dir.join("h")).unwrap();
    for (i, c) in ["git status", "ls", "git commit", "git status", "cargo test"].iter().enumerate()
    {
      h.append(entry(i as u64, c)).unwrap();
    }
    let cmds = |v: Vec<&HistoryEntry>| v.iter().map(|e| e.command.clone()).collect::<Vec<_>>();
    assert_eq!(cmds(h.search_prefix("git", false)), ["git status", "git commit", "git status"]);
    assert_eq!(cmds(h.search_prefix("git", true)), ["git status", "git commit"]);
    assert_eq!(cmds(h.search_substring("st", true)), ["cargo test", "git status"]);
    assert_eq!(cmds(h.recent(true)), ["cargo test", "git status", "git commit", "ls"]);
    assert!(h.search_prefix("zzz", false).is_empty());
    let _ = fs::remove_dir_all(dir);
  }

  #[test]
  fn zsh_import_format() {
    let src = b": 1700000000:0;ls -la\n: 1700000005:12;git commit -m 'a\\\nb'\nplain command\n: 1700000009:0;echo hi\\\\\n: 1700000010:0;next\n";
    let entries = parse_zsh_history(src);
    expect![@r#"
        [
            HistoryEntry {
                timestamp: 1700000000,
                cwd: "",
                exit_code: 0,
                duration_ms: 0,
                command: "ls -la",
            },
            HistoryEntry {
                timestamp: 1700000005,
                cwd: "",
                exit_code: 0,
                duration_ms: 12000,
                command: "git commit -m 'a\nb'",
            },
            HistoryEntry {
                timestamp: 0,
                cwd: "",
                exit_code: 0,
                duration_ms: 0,
                command: "plain command",
            },
            HistoryEntry {
                timestamp: 1700000009,
                cwd: "",
                exit_code: 0,
                duration_ms: 0,
                command: "echo hi\\\\",
            },
            HistoryEntry {
                timestamp: 1700000010,
                cwd: "",
                exit_code: 0,
                duration_ms: 0,
                command: "next",
            },
        ]
    "#]
    .assert_debug_eq(&entries);
  }

  #[test]
  fn zsh_metafied() {
    // "echo \u{e9}" is bytes c3 a9; zsh stores each byte >= 0x80 as 83 (b^20).
    let mut src = b": 1:0;echo ".to_vec();
    src.extend([0x83, 0xc3 ^ 0x20, 0x83, 0xa9 ^ 0x20, b'\n']);
    let entries = parse_zsh_history(&src);
    assert_eq!(entries[0].command, "echo \u{e9}");
  }

  #[test]
  fn import_appends() {
    let dir = tmp("import");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("zh"), ": 1:0;a\n: 2:0;b\n").unwrap();
    let mut h = History::open(dir.join("h")).unwrap();
    assert_eq!(h.import_zsh(dir.join("zh")).unwrap(), 2);
    assert_eq!(History::open(dir.join("h")).unwrap().len(), 2);
    let _ = fs::remove_dir_all(dir);
  }
}
