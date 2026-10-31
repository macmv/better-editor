//! The editable command line of the shell view. This is plain text editing
//! with no rendering or shell knowledge, so that it can be swapped for an
//! `EditorState` later.

/// A single line of text with a cursor.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct InputLine {
  text:   String,
  /// A byte offset into `text`, always on a char boundary.
  cursor: usize,
}

impl InputLine {
  pub fn text(&self) -> &str { &self.text }
  pub fn cursor(&self) -> usize { self.cursor }
  #[cfg(test)]
  pub fn is_empty(&self) -> bool { self.text.is_empty() }

  /// Replaces the text, with the cursor at the end.
  pub fn set(&mut self, text: &str) {
    self.text = text.to_string();
    self.cursor = self.text.len();
  }

  pub fn clear(&mut self) {
    self.text.clear();
    self.cursor = 0;
  }

  /// Takes the text, leaving the line empty.
  pub fn take(&mut self) -> String {
    self.cursor = 0;
    std::mem::take(&mut self.text)
  }

  pub fn insert(&mut self, c: char) {
    if c.is_control() {
      return;
    }
    self.text.insert(self.cursor, c);
    self.cursor += c.len_utf8();
  }

  /// Inserts pasted text. Line breaks and tabs become spaces, as this is a
  /// single line, and other control characters are dropped.
  pub fn insert_str(&mut self, s: &str) {
    for c in s.chars() {
      match c {
        '\n' | '\t' => self.insert(' '),
        '\r' if s.contains('\n') => {}
        '\r' => self.insert(' '),
        c => self.insert(c),
      }
    }
  }

  pub fn backspace(&mut self) {
    let Some(prev) = self.prev_boundary() else { return };
    self.text.replace_range(prev..self.cursor, "");
    self.cursor = prev;
  }

  pub fn delete(&mut self) {
    let Some(next) = self.next_boundary() else { return };
    self.text.replace_range(self.cursor..next, "");
  }

  pub fn left(&mut self) {
    if let Some(prev) = self.prev_boundary() {
      self.cursor = prev;
    }
  }

  pub fn right(&mut self) {
    if let Some(next) = self.next_boundary() {
      self.cursor = next;
    }
  }

  pub fn home(&mut self) { self.cursor = 0; }
  pub fn end(&mut self) { self.cursor = self.text.len(); }

  /// `C-u`: deletes everything before the cursor.
  pub fn kill_to_start(&mut self) {
    self.text.replace_range(..self.cursor, "");
    self.cursor = 0;
  }

  /// `C-k`: deletes everything after the cursor.
  pub fn kill_to_end(&mut self) { self.text.truncate(self.cursor); }

  /// `C-w`: deletes the word before the cursor, and any spaces after it.
  pub fn kill_word_back(&mut self) {
    let before = &self.text[..self.cursor];
    let trimmed = before.trim_end();
    let start = trimmed
      .char_indices()
      .rev()
      .find(|(_, c)| c.is_whitespace())
      .map_or(0, |(i, c)| i + c.len_utf8());
    self.text.replace_range(start..self.cursor, "");
    self.cursor = start;
  }

  fn prev_boundary(&self) -> Option<usize> {
    self.text[..self.cursor].char_indices().next_back().map(|(i, _)| i)
  }

  fn next_boundary(&self) -> Option<usize> {
    self.text[self.cursor..].chars().next().map(|c| self.cursor + c.len_utf8())
  }
}

/// Walks through history entries that start with what was typed.
#[derive(Default)]
pub struct HistoryNav {
  /// What was typed before the first `prev`.
  draft:      String,
  /// Newest first.
  candidates: Vec<String>,
  pos:        Option<usize>,
}

impl HistoryNav {
  pub fn reset(&mut self) { *self = HistoryNav::default(); }

  /// The next older entry, or `None` if there is none (or nothing matched).
  /// `search` is called when starting, with the draft, and returns matching
  /// entries newest first.
  pub fn prev(&mut self, current: &str, search: impl FnOnce(&str) -> Vec<String>) -> Option<&str> {
    let next = match self.pos {
      None => {
        self.draft = current.to_string();
        self.candidates = search(current);
        0
      }
      Some(i) => i + 1,
    };
    if next >= self.candidates.len() {
      return None;
    }
    self.pos = Some(next);
    Some(&self.candidates[next])
  }

  /// The next newer entry, or the draft after the newest one.
  pub fn next(&mut self) -> Option<&str> {
    match self.pos? {
      0 => {
        self.pos = None;
        Some(&self.draft)
      }
      i => {
        self.pos = Some(i - 1);
        Some(&self.candidates[i - 1])
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn line(text: &str) -> InputLine {
    let mut l = InputLine::default();
    l.set(text);
    l
  }

  #[test]
  fn insert_and_move() {
    let mut l = InputLine::default();
    for c in "ls".chars() {
      l.insert(c);
    }
    l.left();
    l.insert('x');
    assert_eq!(l.text(), "lxs");
    assert_eq!(l.cursor(), 2);
    l.home();
    assert_eq!(l.cursor(), 0);
    l.end();
    assert_eq!(l.cursor(), 3);
    l.right();
    assert_eq!(l.cursor(), 3);
  }

  #[test]
  fn backspace_and_delete_respect_chars() {
    let mut l = line("a\u{e9}b");
    l.left();
    l.backspace();
    assert_eq!(l.text(), "ab");
    l.home();
    l.delete();
    assert_eq!(l.text(), "b");
    l.delete();
    l.delete();
    assert_eq!(l.text(), "");
    l.backspace();
    assert_eq!(l.text(), "");
  }

  #[test]
  fn kills() {
    let mut l = line("echo hello world");
    l.kill_word_back();
    assert_eq!(l.text(), "echo hello ");
    l.kill_word_back();
    assert_eq!(l.text(), "echo ");
    l.left();
    l.left();
    l.kill_to_end();
    assert_eq!(l.text(), "ech");
    l.kill_to_start();
    assert_eq!(l.text(), "");
    assert_eq!(l.cursor(), 0);

    let mut l = line("ls  ");
    l.kill_word_back();
    assert_eq!(l.text(), "");
  }

  #[test]
  fn paste_flattens() {
    let mut l = InputLine::default();
    l.insert_str("a\r\nb\tc\x1b");
    assert_eq!(l.text(), "a b c");
    assert_eq!(l.take(), "a b c");
    assert!(l.is_empty());
  }

  #[test]
  fn control_chars_are_not_inserted() {
    let mut l = InputLine::default();
    l.insert('\n');
    l.insert('\x03');
    assert!(l.is_empty());
  }

  #[test]
  fn history_walks_and_restores_draft() {
    let mut nav = HistoryNav::default();
    let search = |p: &str| {
      assert_eq!(p, "ca");
      vec!["cargo test".to_string(), "cat x".to_string()]
    };
    assert_eq!(nav.prev("ca", search), Some("cargo test"));
    assert_eq!(nav.prev("cargo test", |_| unreachable!()), Some("cat x"));
    assert_eq!(nav.prev("cat x", |_| unreachable!()), None);
    assert_eq!(nav.next(), Some("cargo test"));
    assert_eq!(nav.next(), Some("ca"));
    assert_eq!(nav.next(), None);
  }

  #[test]
  fn history_with_no_match() {
    let mut nav = HistoryNav::default();
    assert_eq!(nav.prev("zzz", |_| vec![]), None);
    assert_eq!(nav.next(), None);
  }
}
