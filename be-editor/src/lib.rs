use be_doc::{Column, Cursor, Document, Line};
use be_input::{Action, Edit, Mode, Move};
use unicode_segmentation::UnicodeSegmentation;

use crate::fs::OpenedFile;

mod fs;

#[derive(Default)]
pub struct EditorState {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,

  file:    Option<OpenedFile>,
  command: Option<CommandState>,
}

#[derive(Default)]
pub struct CommandState {
  pub text:   String,
  pub cursor: usize, // in bytes
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
  pub fn command(&self) -> Option<&CommandState> { self.command.as_ref() }

  pub fn move_line_rel(&mut self, dist: i32) { self.move_to_line(self.cursor.line + dist); }
  pub fn move_col_rel(&mut self, dist: i32) { self.move_to_col(self.cursor.column + dist as i32); }
  pub fn move_graphemes(&mut self, delta: isize) {
    let mut target_column = self.cursor.column.0 as isize + delta;

    while target_column < 0 {
      if self.cursor.line == 0 {
        self.cursor.line.0 = 0;
        self.move_to_col(Column(0));
        return;
      }

      self.cursor.column.0 = 0;
      self.cursor.line.0 -= 1;
      target_column += self.doc.line_with_terminator(self.cursor.line).graphemes().count() as isize;
    }

    while target_column
      >= self.doc.line_with_terminator(self.cursor.line).graphemes().count() as isize
    {
      if self.cursor.line == self.max_line() {
        self.move_to_col(self.max_column());
        return;
      }

      target_column -= self.doc.line_with_terminator(self.cursor.line).graphemes().count() as isize;
      self.cursor.column.0 = 0;
      self.cursor.line.0 += 1;
    }

    self.cursor.column.0 = target_column as usize;
  }

  fn move_to_line(&mut self, line: Line) {
    self.cursor.line = line.clamp(self.max_line());
    self.cursor.column = self
      .doc
      .column_from_visual(self.cursor.line, self.cursor.target_column)
      .clamp(self.max_column());
  }
  fn move_to_col(&mut self, col: Column) {
    self.cursor.column = col.clamp(self.max_column());
    self.cursor.target_column = self.doc.visual_column(self.cursor);
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

    if m == Mode::Command {
      self.command = Some(CommandState::default());
    } else {
      self.command = None;
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::SetMode(m) => self.set_mode(m),
      Action::Move { count: _, m } => self.perform_move(m),
      Action::Edit { count: _, e } => self.perform_edit(e),
    }
  }

  fn perform_move(&mut self, m: be_input::Move) {
    if let Some(command) = &mut self.command {
      command.perform_move(m);
      return;
    }

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
  fn perform_edit(&mut self, e: Edit) {
    if let Some(command) = &mut self.command {
      command.perform_edit(e);
      return;
    }

    match e {
      Edit::Insert(c) => {
        let mut bytes = [0; 4];
        let s = c.encode_utf8(&mut bytes);
        self.doc.insert(self.cursor, s);
        self.move_graphemes(1);
      }

      Edit::Delete => {
        self.doc.delete_graphemes(self.cursor, 1);
      }

      _ => {}
    }
  }

  pub fn cursor_column_byte(&self) -> usize {
    let line = self.doc.line(self.cursor.line);
    line.graphemes().take(self.cursor.column.0).map(|g| g.len()).sum()
  }
}

impl CommandState {
  fn perform_move(&mut self, m: Move) {
    match m {
      Move::Left => self.move_cursor(-1),
      Move::Right => self.move_cursor(1),

      _ => {}
    }
  }
  fn perform_edit(&mut self, e: Edit) {
    match e {
      Edit::Insert(c) => {
        self.text.insert(self.cursor, c);
        self.move_cursor(1);
      }

      _ => {}
    }
  }

  fn move_cursor(&mut self, dist: i32) {
    if dist >= 0 {
      for c in self.text[self.cursor..].graphemes(true).take(dist as usize) {
        self.cursor += c.len();
      }
    } else {
      for c in self.text[..self.cursor].graphemes(true).rev().take(-dist as usize) {
        self.cursor -= c.len();
      }
    }
  }
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

  #[test]
  fn move_graphemes_works() {
    let mut state = EditorState::from("abc\ndef");

    state.move_graphemes(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 1);

    state.move_graphemes(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 2);

    state.move_graphemes(1);
    assert_eq!(state.cursor.line, 0);
    assert_eq!(state.cursor.column, 3);

    state.move_graphemes(1);
    assert_eq!(state.cursor.line, 1);
    assert_eq!(state.cursor.column, 0);
  }
}
