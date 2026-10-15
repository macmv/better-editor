use std::{
  fmt,
  ops::{Add, Deref, Sub},
};

use be_rope::Rope;

mod edit;
mod fs;
mod search;
mod snap;

pub use edit::{Change, Edit};
pub use search::FindIter;
pub use snap::DocumentSnapshot;

#[macro_use]
extern crate be_macros;

#[derive(Default, Clone)]
pub struct Document {
  snap: DocumentSnapshot,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
  pub line:          Line,
  pub column:        Column,
  pub target_column: VisualColumn,
}

/// This is a dual position. It is only valid when both the offset and
/// line/column refer to the same position within a file.
///
/// This is stored this way so that ranges can easily be revised throughout
/// history - if a change adds/removes lines, we can easily tell that from the
/// `line` of the positions, and we don't need the document the change was
/// applied to to infer that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
  pub offset: usize,
  pub line:   Line,
  pub column: Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoricChange {
  pub start:      Position,
  pub end_before: Position,
  pub end_after:  Position,
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
  fn from(s: &str) -> Document { Document { snap: DocumentSnapshot { rope: Rope::from(s) } } }
}
impl From<std::borrow::Cow<'_, str>> for Document {
  fn from(s: std::borrow::Cow<'_, str>) -> Document {
    Document { snap: DocumentSnapshot { rope: Rope::from(s) } }
  }
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
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{:?}", self.snap.rope) }
}

impl Deref for Document {
  type Target = DocumentSnapshot;

  fn deref(&self) -> &DocumentSnapshot { &self.snap }
}

impl Document {
  pub fn new() -> Document { Document { snap: DocumentSnapshot { rope: Rope::new() } } }
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

impl HistoricChange {
  pub fn map_position(&self, position: &mut Position) {
    if position.offset > self.end_before.offset {
      let line_delta = self.end_after.line.0 as isize - self.end_before.line.0 as isize;
      let offset_delta = self.end_after.offset as isize - self.end_before.offset as isize;

      if position.line == self.end_before.line {
        let column_delta = self.end_after.column.0 as isize - self.end_before.column.0 as isize;
        position.column.0 = (position.column.0 as isize + column_delta).max(0) as usize;
      }
      position.line.0 = (position.line.0 as isize + line_delta).max(0) as usize;
      position.offset = (position.offset as isize + offset_delta).max(0) as usize;
    } else if position.offset > self.start.offset {
      *position = self.start;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn delete_graphemes() {
    let mut doc = Document::from("abc");
    doc.snap.rope.replace(doc.grapheme_slice(Cursor::START, 2), "");
    assert_eq!(doc.rope, "c");
  }

  #[test]
  fn delete_graphemes_handles_emojis() {
    let mut doc = Document::from("💖a💖");
    doc.snap.rope.replace(doc.grapheme_slice(Cursor::START, 2), "");
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
