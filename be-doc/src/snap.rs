use std::ops::{Range, RangeBounds};

use crop::{Rope, RopeSlice};
use unicode_width::UnicodeWidthStr;

use crate::{Column, Cursor, Document, Line, VisualColumn};

/// An immutable copy of a Document at a particular point in time.
#[derive(Default, Clone)]
pub struct DocumentSnapshot {
  pub(crate) rope: Rope,
}

impl<T> From<T> for DocumentSnapshot
where
  Document: From<T>,
{
  fn from(v: T) -> Self { Document::from(v).snapshot() }
}

impl DocumentSnapshot {
  pub fn raw_lines(&self) -> crop::iter::RawLines<'_> { self.rope.raw_lines() }

  pub fn len(&self) -> usize { self.rope.byte_len() }

  pub fn len_lines(&self) -> usize { self.rope.line_len() }

  // TODO: Should Document impl Display?
  pub fn to_string(&self) -> String { self.rope.to_string() }

  pub fn snapshot(&self) -> DocumentSnapshot { DocumentSnapshot { rope: self.rope.clone() } }

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

    let line = Line(self.line_of_byte(offset).0.clamp(0, self.len_lines().saturating_sub(1)));
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
    let len = self.rope.byte_len();
    if len == 0 {
      return Line(0);
    }
    if byte > len {
      fatal!("byte {} is out of bounds", byte);
      byte = len;
    }
    if byte == len {
      return Line(self.len_lines().saturating_sub(1));
    }

    // NB: `crop::line_of_byte` panics on non-char boundaries, so advance to the
    // next char.
    while byte < len && !self.rope.is_char_boundary(byte) {
      fatal!("byte {} is not a char boundary", byte);
      byte += 1;
    }

    if byte >= len {
      Line(self.len_lines().saturating_sub(1))
    } else {
      Line(self.rope.line_of_byte(byte))
    }
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
