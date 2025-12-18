use be_doc::{Column, Cursor, Document, Line};
use be_input::Mode;

pub struct EditorState {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,
}

impl From<&str> for EditorState {
  fn from(s: &str) -> EditorState {
    EditorState { doc: Document::from(s), cursor: Cursor::START, mode: Mode::Normal }
  }
}

impl EditorState {
  pub fn new() -> EditorState {
    EditorState { doc: Document::new(), cursor: Cursor::START, mode: Mode::Normal }
  }

  pub fn doc(&self) -> &Document { &self.doc }
  pub fn cursor(&self) -> &Cursor { &self.cursor }
  pub fn mode(&self) -> Mode { self.mode }

  pub fn move_row(&mut self, dist: i32) {
    let line = self.cursor.line + dist;
    self.cursor.line = line.clamp(self.max_line());

    let max_col = self.max_column();
    self.cursor.column = self.cursor.target_column.clamp(max_col);
  }

  pub fn move_col(&mut self, dist: i32) {
    let max_col = self.max_column();

    let col = self.cursor.column + dist as i32;
    self.cursor.column = col.clamp(max_col);
    self.cursor.target_column = self.cursor.column;
  }

  fn max_line(&self) -> Line { Line(self.doc.len_lines().saturating_sub(1)) }

  fn max_column(&self) -> Column {
    let line = self.doc.line(self.cursor.line);

    let mut max_col = line.graphemes().count();
    if self.mode == Mode::Normal {
      max_col = max_col.saturating_sub(1);
    }

    Column(max_col)
  }

  pub fn set_mode(&mut self, m: Mode) {
    self.mode = m;
    self.cursor.column = self.max_column();
    self.cursor.target_column = self.cursor.column;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn move_col_works() {
    let mut state = EditorState::from("ab");

    state.move_col(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 1);

    state.move_col(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 1);
  }
}
