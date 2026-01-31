use std::{
  fmt,
  ops::{Add, Range, RangeBounds, Sub},
};

use crop::{Rope, RopeSlice};
use unicode_width::UnicodeWidthStr;

mod edit;
mod fs;
mod search;

pub use crop;
pub use edit::{Change, Edit};
pub use search::FindIter;

#[macro_use]
extern crate be_macros;

#[derive(Default, Clone)]
pub struct Document {
  pub rope: Rope,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
  pub line:          Line,
  pub column:        Column,
  pub target_column: VisualColumn,
}

/// A logical line, ie, lines from the start of the file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Line(pub usize);

/// A logical column, ie, graphemes from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Column(pub usize);

/// A visual column, ie, counted in unicode-width from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VisualColumn(pub usize);

impl From<&str> for Document {
  fn from(s: &str) -> Document { Document { rope: Rope::from(s) } }
}

impl Cursor {
  pub const START: Cursor =
    Cursor { line: Line(0), column: Column(0), target_column: VisualColumn(0) };
}

impl Column {
  pub const MAX: Column = Column(usize::MAX);
}

impl VisualColumn {
  pub const MAX: VisualColumn = VisualColumn(usize::MAX);
}

impl Default for Cursor {
  fn default() -> Self { Cursor::START }
}

impl PartialEq<usize> for Line {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}
impl PartialEq<usize> for Column {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}

impl fmt::Debug for Document {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{:?}", self.rope) }
}
impl Document {
  pub fn new() -> Document { Document { rope: Rope::new() } }

  #[track_caller]
  pub fn line(&self, line: Line) -> RopeSlice<'_> {
    if line.0 >= self.len_lines() {
      fatal!("line {} is out of bounds", line.0);
      return self.rope.byte_slice(0..0);
    }

    self.rope.line(line.0)
  }

  #[track_caller]
  pub fn line_with_terminator(&self, line: Line) -> RopeSlice<'_> {
    if line.0 >= self.len_lines() {
      fatal!("line {} is out of bounds", line.0);
      return self.rope.byte_slice(0..0);
    }

    self.rope.line_slice(line.0..line.0 + 1)
  }

  #[track_caller]
  pub fn byte_of_line(&self, line: Line) -> usize {
    if line.0 > self.len_lines() {
      fatal!("line {} is out of bounds", line.0);
      return self.rope.byte_len();
    }

    self.rope.byte_of_line(line.0)
  }

  /// Returns the byte of the end of the line. This byte points to the first
  /// character in the line terminator.
  #[track_caller]
  pub fn byte_of_line_end(&self, line: Line) -> usize {
    if line.0 > self.len_lines() {
      fatal!("line {} is out of bounds", line.0);
      return self.rope.byte_len();
    }

    let terminator = self.line_with_terminator(line).graphemes().rev().next();

    if let Some(term) = terminator
      && term.chars().all(|c| c.is_whitespace())
    {
      self.byte_of_line(line + 1) - term.len()
    } else {
      // If there is no terminator, we're at the end of the file.
      self.rope.byte_len()
    }
  }
  pub fn len_lines(&self) -> usize { self.rope.line_len() }

  #[track_caller]
  pub fn visual_column(&self, cursor: Cursor) -> VisualColumn {
    if cursor.line.0 >= self.len_lines() {
      fatal!("line {} is out of bounds", cursor.line.0);
      return VisualColumn(0);
    }

    let mut offset = 0;
    for g in self.rope.line(cursor.line.0).graphemes().take(cursor.column.0) {
      offset += g.width();
    }
    VisualColumn(offset)
  }

  #[track_caller]
  pub fn column_from_visual(&self, line: Line, visual_column: VisualColumn) -> Column {
    if line.0 >= self.len_lines() {
      fatal!("line {} is out of bounds", line.0);
      return Column(0);
    }

    let mut offset = 0;
    Column(
      self
        .rope
        .line(line.0)
        .graphemes()
        .take_while(|g| {
          offset += g.width();
          offset <= visual_column.0
        })
        .count(),
    )
  }

  #[track_caller]
  pub fn cursor_offset(&self, cursor: Cursor) -> usize {
    if cursor.line.0 >= self.len_lines() {
      fatal!("line {} is out of bounds", cursor.line.0);
      return self.rope.byte_len();
    }

    self.rope.byte_of_line(cursor.line.0) + self.cursor_column_offset(cursor)
  }

  #[track_caller]
  pub fn offset_to_cursor(&self, offset: usize) -> Cursor {
    if offset >= self.rope.byte_len() {
      fatal!("byte {} is out of bounds", offset);
    }

    let line = Line(self.rope.line_of_byte(offset).clamp(0, self.len_lines().saturating_sub(1)));
    let column = Column(self.range(self.byte_of_line(line)..offset).graphemes().count());
    let mut cursor = Cursor { line, column, target_column: VisualColumn(0) };
    cursor.target_column = self.visual_column(cursor);
    cursor
  }

  pub fn cursor_column_offset(&self, cursor: Cursor) -> usize {
    let line = self.line(cursor.line);
    line.graphemes().take(cursor.column.0).map(|g| g.len()).sum()
  }

  pub fn offset_by_graphemes(&self, index: usize, offset: isize) -> usize {
    if offset > 0 {
      index
        + self
          .rope
          .byte_slice(index..)
          .graphemes()
          .take(offset as usize)
          .map(|g| g.len())
          .sum::<usize>()
    } else {
      index
        - self
          .rope
          .byte_slice(..index)
          .graphemes()
          .rev()
          .take(-offset as usize)
          .map(|g| g.len())
          .sum::<usize>()
    }
  }

  #[track_caller]
  pub fn grapheme_slice(&self, cursor: Cursor, len: usize) -> Range<usize> {
    let offset = self.cursor_offset(cursor);
    let count =
      self.rope.byte_slice(offset..).graphemes().take(len).map(|g| g.len()).sum::<usize>();
    offset..offset + count
  }

  #[track_caller]
  pub fn range(&self, range: impl RangeBounds<usize>) -> RopeSlice<'_> {
    let start = match range.start_bound() {
      std::ops::Bound::Unbounded => 0,
      std::ops::Bound::Included(start) => self.clamp_inclusive(*start),
      // Not sure if this is correct.
      std::ops::Bound::Excluded(start) => self.clamp_exclusive(*start),
    };
    let end = match range.end_bound() {
      std::ops::Bound::Unbounded => self.rope.byte_len(),
      std::ops::Bound::Included(end) => self.clamp_inclusive(*end),
      std::ops::Bound::Excluded(end) => self.clamp_exclusive(*end),
    };

    self.rope.byte_slice(start..end)
  }

  /// Returns the line of the given byte.
  #[track_caller]
  pub fn line_of_byte(&self, mut byte: usize) -> Line {
    byte = self.clamp_inclusive(byte);

    // NB: `crop::line_of_byte` panics on non-char boundaries, so advance to the
    // next char.
    while !self.rope.is_char_boundary(byte) {
      fatal!("byte {} is not a char boundary", byte);
      byte += 1;
    }

    byte = self.clamp_inclusive(byte);

    Line(self.rope.line_of_byte(byte))
  }

  #[track_caller]
  fn clamp_inclusive(&self, byte: usize) -> usize {
    if byte >= self.rope.byte_len() {
      fatal!("byte {} is out of bounds", byte);
    }
    byte.clamp(0, self.rope.byte_len() - 1)
  }
  #[track_caller]
  fn clamp_exclusive(&self, byte: usize) -> usize {
    if byte > self.rope.byte_len() {
      fatal!("byte {} is out of bounds", byte);
    }
    byte.clamp(0, self.rope.byte_len())
  }
}

impl Column {
  pub fn as_usize(&self) -> usize { self.0 }
  pub fn clamp(self, max: Column) -> Column { Column(self.0.clamp(0, max.0)) }
}

impl Line {
  pub fn as_usize(&self) -> usize { self.0 }
  pub fn clamp(self, max: Line) -> Line { Line(self.0.clamp(0, max.0)) }
}

impl Add<i32> for Column {
  type Output = Column;

  fn add(self, rhs: i32) -> Column { Column((self.0 as isize + rhs as isize).max(0) as usize) }
}

impl Sub<i32> for Column {
  type Output = Column;

  fn sub(self, rhs: i32) -> Column { Column((self.0 as isize - rhs as isize).max(0) as usize) }
}

impl Add<i32> for Line {
  type Output = Line;

  fn add(self, rhs: i32) -> Line { Line((self.0 as isize + rhs as isize).max(0) as usize) }
}

impl Sub<i32> for Line {
  type Output = Line;

  fn sub(self, rhs: i32) -> Line { Line((self.0 as isize - rhs as isize).max(0) as usize) }
}

impl PartialEq<&str> for Document {
  fn eq(&self, other: &&str) -> bool { self.rope == *other }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn delete_graphemes() {
    let mut doc = Document::from("abc");
    doc.rope.replace(doc.grapheme_slice(Cursor::START, 2), "");
    assert_eq!(doc.rope, "c");
  }

  #[test]
  fn delete_graphemes_handles_emojis() {
    let mut doc = Document::from("💖a💖");
    doc.rope.replace(doc.grapheme_slice(Cursor::START, 2), "");
    assert_eq!(doc.rope, "💖");
  }

  #[test]
  fn byte_of_line_end() {
    for doc in ["abc\ndef", "abc\r\ndef", "abc\ndef\n", "abc\r\ndef\n", "abc\r\ndef\r\n"] {
      let doc = Document::from(doc);
      assert_eq!(
        doc.range(doc.byte_of_line(Line(0))..doc.byte_of_line_end(Line(0))),
        "abc",
        "in doc {doc:?}"
      );
      assert_eq!(
        doc.range(doc.byte_of_line(Line(1))..doc.byte_of_line_end(Line(1))),
        "def",
        "in doc {doc:?}"
      );
    }
  }

  #[test]
  fn line_of_byte_doesnt_panic() {
    let doc = Document::from("💖a💖");
    // first emoji
    assert_eq!(doc.line_of_byte(0), 0);
    // 'a'
    assert_eq!(doc.line_of_byte(4), 0);
    // second emoji
    assert_eq!(doc.line_of_byte(5), 0);
  }

  #[test]
  #[cfg_attr(debug_assertions, should_panic)]
  fn line_of_byte_panic() {
    let doc = Document::from("💖a💖");
    assert_eq!(doc.line_of_byte(1), 0);
  }
}
