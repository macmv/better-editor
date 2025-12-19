use be_doc::{Column, Cursor, Document, Line};
use be_input::{Action, Mode, Move};

use crate::fs::OpenedFile;

mod fs;

#[derive(Default)]
pub struct EditorState {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,

  file: Option<OpenedFile>,
}

impl From<&str> for EditorState {
  fn from(s: &str) -> EditorState {
    let mut state = EditorState::default();
    state.doc = Document::from(s);
    state
  }
}

impl EditorState {
  pub fn new() -> EditorState { EditorState::default() }

  pub fn doc(&self) -> &Document { &self.doc }
  pub fn cursor(&self) -> &Cursor { &self.cursor }
  pub fn mode(&self) -> Mode { self.mode }

  pub fn move_line_rel(&mut self, dist: i32) { self.move_to_line(self.cursor.line + dist); }
  pub fn move_col_rel(&mut self, dist: i32) { self.move_to_col(self.cursor.column + dist as i32); }

  fn move_to_line(&mut self, line: Line) {
    self.cursor.line = line.clamp(self.max_line());
    self.cursor.column = self.cursor.target_column.clamp(self.max_column());
  }
  fn move_to_col(&mut self, col: Column) {
    self.cursor.column = col.clamp(self.max_column());
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
    self.move_to_col(self.cursor.column.clamp(self.max_column()));
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::SetMode(m) => self.set_mode(m),
      Action::Move { count: _, m } => self.perform_move(m),
      Action::Edit { count: _, e } => self.perform_edit(e),
    }
  }

  fn perform_move(&mut self, m: be_input::Move) {
    match m {
      Move::Left => self.move_col_rel(-1),
      Move::Right => self.move_col_rel(1),
      Move::Up => self.move_line_rel(-1),
      Move::Down => self.move_line_rel(1),

      Move::LineEnd => self.move_to_col(self.max_column()),
      Move::LineStart => self.move_to_col(Column(0)),

      Move::FileStart => self.move_to_line(Line(0)),
      Move::FileEnd => self.move_to_line(self.max_line()),

      _ => {}
    }
  }
  fn perform_edit(&mut self, _: be_input::Edit) {}
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn move_col_works() {
    let mut state = EditorState::from("ab");

    state.move_col_rel(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 1);

    state.move_col_rel(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 1);
  }
}
