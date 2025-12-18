use crop::Rope;

pub struct Document {
  pub rope: Rope,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
  pub line:          Line,
  pub column:        Column,
  pub target_column: Column,
}

/// A visual line, ie, lines from the start of the file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line(usize);

/// A visual column, ie, graphemes from the start of the line.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Column(usize);

impl From<&str> for Document {
  fn from(s: &str) -> Document { Document { rope: Rope::from(s) } }
}

impl Cursor {
  pub const START: Cursor =
    Cursor { line: Line(0), column: Column(0), target_column: Column(0) };
}

impl PartialEq<usize> for Line {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}
impl PartialEq<usize> for Column {
  fn eq(&self, other: &usize) -> bool { self.0 == *other }
}

impl Document {
  pub fn new() -> Document { Document { rope: Rope::new() } }

  pub fn move_row(&self, mut cursor: Cursor, dist: i32) -> Cursor {
    let max_line = self.rope.line_len() as i32;

    let line = cursor.line.0 as i32 + dist as i32;
    cursor.line = Line(line.clamp(0, max_line) as usize);

    let line = self.rope.line(cursor.line.0);
    let max_col = line.graphemes().count();
    cursor.column = Column(cursor.target_column.0.clamp(0, max_col));

    cursor
  }

  pub fn move_col(&self, mut cursor: Cursor, dist: i32) -> Cursor {
    let line = self.rope.line(cursor.line.0);

    let max_col = line.graphemes().count() as i32;

    let col = cursor.column.0 as i32 + dist as i32;
    cursor.column = Column(col.clamp(0, max_col) as usize);
    cursor.target_column = cursor.column;

    cursor
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn visual_pos_works() {
    let doc = Document::from("❄");

    let mut cursor = Cursor::START;
    assert_eq!(cursor.line, 0);
    assert_eq!(cursor.column, 0);

    cursor = doc.move_col(cursor, 1);
    assert_eq!(cursor.line, 0);
    assert_eq!(cursor.column, 1);

    cursor = doc.move_col(cursor, 1);
    assert_eq!(cursor.line, 0);
    assert_eq!(cursor.column, 1);
  }
}
