use std::ops::Add;

use crop::{Rope, RopeSlice};
use unicode_width::UnicodeWidthStr;

mod fs;

#[derive(Default)]
pub struct Document {
  pub rope: Rope,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
  pub line:          Line,
  pub column:        Column,
  pub target_column: Column,
}

/// A logical line, ie, lines from the start of the file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line(pub usize);

/// A logical column, ie, graphemes from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Column(pub usize);

/// A visual column, ie, counted in unicode-width from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VisualColumn(pub usize);

impl From<&str> for Document {
  fn from(s: &str) -> Document { Document { rope: Rope::from(s) } }
}

impl Cursor {
  pub const START: Cursor =
    Cursor { line: Line(0), column: Column(0), target_column: Column(0) };
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

  fn cursor_offset(&self, cursor: Cursor) -> usize {
    let mut offset = self.rope.byte_of_line(cursor.line.0);
    for g in self.rope.line(cursor.line.0).graphemes().take(cursor.column.0) {
      offset += g.len();
    }
    offset
  }

  pub fn insert(&mut self, cursor: Cursor, s: &str) {
    self.rope.insert(self.cursor_offset(cursor), s)
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
