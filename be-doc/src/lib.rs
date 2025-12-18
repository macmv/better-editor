use crop::Rope;

pub struct Document {
  rope: Rope,
}

pub struct Cursor {
  fixed_col: Column,
  index:     usize,
}

pub struct Pos {
  pub line: usize,
  pub col:  Column,
}

/// A visual column, ie, graphemes from the start of the line.
pub struct Column(usize);

impl From<&str> for Document {
  fn from(s: &str) -> Document { Document { rope: Rope::from(s) } }
}

impl Document {
  pub fn new() -> Document { Document { rope: Rope::new() } }

  pub fn cursor_pos(&self, cursor: Cursor) -> Pos {
    let line = self.rope.line_of_byte(cursor.index);
    let mut col = Column(0);
    let mut index = cursor.index - self.rope.byte_of_line(line);
    for g in self.rope.line(line).graphemes() {
      if index >= cursor.index {
        break;
      }

      index += g.len();
      col.0 += 1;
    }

    Pos { line, col }
  }
}
