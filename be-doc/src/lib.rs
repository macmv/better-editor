use std::ops::{Add, Range};

use crop::{Rope, RopeSlice};
use unicode_width::UnicodeWidthStr;

mod fs;

pub use crop;

#[derive(Default)]
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

impl Document {
  pub fn new() -> Document { Document { rope: Rope::new() } }

  pub fn line(&self, line: Line) -> RopeSlice<'_> { self.rope.line(line.0) }
  pub fn line_with_terminator(&self, line: Line) -> RopeSlice<'_> {
    self.rope.line_slice(line.0..line.0 + 1)
  }
  pub fn len_lines(&self) -> usize { self.rope.line_len() }

  pub fn visual_column(&self, cursor: Cursor) -> VisualColumn {
    let mut offset = 0;
    for g in self.rope.line(cursor.line.0).graphemes().take(cursor.column.0) {
      offset += g.width();
    }
    VisualColumn(offset)
  }

  pub fn column_from_visual(&self, line: Line, visual_column: VisualColumn) -> Column {
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

  pub fn cursor_offset(&self, cursor: Cursor) -> usize {
    self.rope.byte_of_line(cursor.line.0) + self.cursor_column_offset(cursor)
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

  pub fn grapheme_slice(&self, cursor: Cursor, len: usize) -> Range<usize> {
    let offset = self.cursor_offset(cursor);
    let count =
      self.rope.byte_slice(offset..).graphemes().take(len).map(|g| g.len()).sum::<usize>();
    offset..offset + count
  }

  pub fn range(&self, range: Range<usize>) -> RopeSlice<'_> { self.rope.byte_slice(range) }

  pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
    self.rope.replace(range, text);
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

impl Add<i32> for Line {
  type Output = Line;

  fn add(self, rhs: i32) -> Line { Line((self.0 as isize + rhs as isize).max(0) as usize) }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn delete_graphemes() {
    let mut doc = Document::from("abc");
    doc.replace_range(doc.grapheme_slice(Cursor::START, 2), "");
    assert_eq!(doc.rope, "c");
  }

  #[test]
  fn delete_graphemes_handles_emojis() {
    let mut doc = Document::from("💖a💖");
    doc.replace_range(doc.grapheme_slice(Cursor::START, 2), "");
    assert_eq!(doc.rope, "💖");
  }
}
