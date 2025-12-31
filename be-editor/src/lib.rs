use std::{cell::RefCell, collections::HashSet, path::Path, rc::Rc};

use be_config::Config;
use be_doc::{Change, Column, Cursor, Document, Edit, Line};
use be_input::{Action, Direction, Mode, Move};
use unicode_segmentation::UnicodeSegmentation;

use crate::{fs::OpenedFile, status::Status};

mod filetype;
mod fs;
mod highlight;
mod lsp;
mod status;
mod treesitter;

pub use highlight::HighlightKey;
pub use lsp::{Diagnostic, DiagnosticLevel};

#[derive(Default)]
pub struct EditorState {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,

  file:    Option<OpenedFile>,
  status:  Option<Status>,
  command: Option<CommandState>,

  filetype:   Option<filetype::FileType>,
  highligher: Option<treesitter::Highlighter>,
  damages:    HashSet<Line>,
  damage_all: bool,

  current_edit:     Option<Edit>,
  history_position: usize,
  history:          Vec<Edit>,

  pub config:   Rc<RefCell<Config>>,
  pub lsp:      lsp::LspState,
  pub exit_cmd: Option<Box<dyn Fn()>>,
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
  pub fn cursor(&self) -> Cursor { self.cursor }
  pub fn mode(&self) -> Mode { self.mode }
  pub fn command(&self) -> Option<&CommandState> { self.command.as_ref() }
  pub fn status(&self) -> Option<&Status> { self.status.as_ref() }
  pub fn file_type(&self) -> Option<filetype::FileType> { self.filetype }
  pub fn take_damage_all(&mut self) -> bool { std::mem::take(&mut self.damage_all) }
  pub fn take_damages(&mut self) -> impl Iterator<Item = Line> { self.damages.drain() }

  fn on_open_file(&mut self) {
    let Some(_) = self.file.as_ref() else { return };

    self.move_to_col(Column(0));
    self.move_to_line(Line(0));

    self.detect_filetype();
    self.on_open_file_highlight();
    self.connect_to_lsp();
  }

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

  /// Move to column.
  ///
  /// If `col` is `Column::MAX`, then the target column will also be set to
  /// `VisualColumn::MAX`. Otherwise, the target column will be set to the
  /// visual column of the cursor after clamping to the maximum column in the
  /// current mode.
  fn move_to_col(&mut self, col: Column) {
    let target = col.clamp(self.max_column());
    let changed = self.cursor.column != target;
    self.cursor.column = target;
    if changed {
      if col == Column::MAX {
        self.cursor.target_column = be_doc::VisualColumn::MAX;
      } else {
        self.cursor.target_column = self.doc.visual_column(self.cursor);
      }
    }
  }

  fn clamp_cursor(&mut self) { self.move_to_line(self.cursor.line.clamp(self.max_line())); }

  fn clamp_column(&mut self) {
    self.cursor.column = self.cursor.column.clamp(self.max_column());
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

  fn keep_cursor_for_change(&mut self, change: &Change) {
    let current_offset = self.doc.cursor_offset(self.cursor);
    if current_offset >= change.range.end {
      let line_delta = change.text.chars().filter(|c| *c == '\n').count() as isize
        - self.doc.range(change.range.clone()).chars().filter(|c| *c == '\n').count() as isize;

      if line_delta == 0 {
        let target_line = be_doc::Line(self.doc.rope.line_of_byte(change.range.end));
        if self.cursor.line == target_line {
          let column_delta = change.text.graphemes(true).count() as isize
            - self.doc.range(change.range.clone()).graphemes().count() as isize;
          self.move_col_rel(column_delta as i32);
        }
      } else {
        self.move_line_rel(line_delta as i32);
      }
    } else if current_offset >= change.range.start {
      self.cursor = self.doc.offset_to_cursor(change.range.start);
    }
  }

  pub fn set_mode(&mut self, m: Mode) {
    self.mode = m;
    self.move_to_col(self.cursor.column.clamp(self.max_column()));

    if m == Mode::Command {
      self.command = Some(CommandState::default());
    } else {
      self.command = None;
    }

    if m == Mode::Normal {
      if let Some(edit) = self.current_edit.take() {
        self.add_to_history(edit);
      }
    } else if m == Mode::Insert {
      self.current_edit = Some(Edit::empty());
    }
  }

  /// Should only be called after calling `current_edit.take()` or when applying
  /// a change.
  fn add_to_history(&mut self, edit: Edit) {
    if self.history_position > 0 {
      self.history.drain(self.history.len() - self.history_position..);
    }
    self.history_position = 0;
    self.history.push(edit);
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::SetMode { mode, delta } => {
        if delta < 0 {
          self.move_col_rel(delta);
          self.set_mode(mode);
        } else {
          self.set_mode(mode);
          self.move_col_rel(delta);
        }
      }
      Action::Append { after } => {
        self.set_mode(Mode::Insert);

        if after {
          let target = self.doc.byte_of_line(self.cursor().line + 1);
          self.change(Change::insert(target, "\n"));
          self.move_to_line(self.cursor.line + 1);
        } else {
          let target = self.doc.byte_of_line(self.cursor().line);
          self.change(Change::insert(target, "\n"));
        }

        self.move_to_col(Column(0));
      }
      Action::Move { count: _, m } => self.perform_move(m),
      Action::Edit { count: _, e } => self.perform_edit(e),
      Action::Autocomplete => self.perform_autocomplete(),
      Action::Navigate { .. } => unreachable!(),
      Action::Control { .. } => {} // only really used for the shell
    }
  }

  fn perform_move(&mut self, m: be_input::Move) {
    if let Some(command) = &mut self.command {
      command.perform_move(m);
      return;
    }

    match m {
      Move::Single(Direction::Left) => self.move_col_rel(-1),
      Move::Single(Direction::Right) => self.move_col_rel(1),
      Move::Single(Direction::Up) => self.move_line_rel(-1),
      Move::Single(Direction::Down) => self.move_line_rel(1),

      Move::LineEnd => self.move_to_col(Column::MAX),
      Move::LineStart => self.move_to_col(Column(0)),

      Move::FileStart => self.move_to_line(Line(0)),
      Move::FileEnd => self.move_to_line(self.max_line()),

      _ => {}
    }
  }

  fn perform_edit(&mut self, e: be_input::Edit) {
    use be_input::Edit;

    if let Some(command) = &mut self.command {
      if matches!(e, Edit::Insert('\n')) {
        self.run_command();
        self.set_mode(Mode::Normal);
        return;
      }

      command.perform_edit(e);
      return;
    }

    match e {
      Edit::Insert(c) => {
        let mut bytes = [0; 4];
        let s = c.encode_utf8(&mut bytes);
        self.change(Change::insert(self.doc.cursor_offset(self.cursor), s));
        self.move_graphemes(1);
      }
      Edit::Replace(c) => {
        let mut bytes = [0; 4];
        let s = c.encode_utf8(&mut bytes);
        self.change(Change::replace(self.doc.grapheme_slice(self.cursor, 1), s));
      }
      Edit::Delete => {
        self.change(Change::remove(self.doc.grapheme_slice(self.cursor, 1)));
      }
      Edit::DeleteLine => {
        self.change(Change::remove(
          self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line(self.cursor.line + 1),
        ));
        self.clamp_column();
      }
      Edit::DeleteRestOfLine => {
        self.change(Change::remove(
          self.doc.cursor_offset(self.cursor)
            ..self.doc.offset_by_graphemes(self.doc.byte_of_line(self.cursor.line + 1), -1),
        ));
        self.clamp_column();
      }
      Edit::Backspace => {
        self.move_graphemes(-1);
        self.change(Change::remove(self.doc.grapheme_slice(self.cursor, 1)));
      }
      Edit::Undo => {
        if self.history_position < self.history.len() {
          self.history_position += 1;
          for change in self.history[self.history.len() - self.history_position].clone().undo() {
            self.keep_cursor_for_change(change);
            self.change_no_history(change.clone());
          }
          self.clamp_cursor();
        }
      }
      Edit::Redo => {
        if self.history_position > 0 {
          for change in self.history[self.history.len() - self.history_position].clone().redo() {
            self.keep_cursor_for_change(change);
            self.change_no_history(change.clone());
          }
          self.history_position -= 1;
          self.clamp_cursor();
        }
      }
    }
  }

  fn perform_autocomplete(&mut self) { self.lsp_request_completions(); }

  fn change(&mut self, change: Change) {
    if let Some(edit) = &mut self.current_edit {
      edit.push(&change, &self.doc);
    } else {
      self.add_to_history(Edit::new(&change, &self.doc));
    }

    self.change_no_history(change);
  }

  fn change_no_history(&mut self, change: Change) {
    let start_pos = self.offset_to_ts_point(change.range.start);
    let end_pos = self.offset_to_ts_point(change.range.end);

    for line in start_pos.row..=end_pos.row {
      self.damages.insert(Line(line));
    }

    if change.text.contains('\n') || self.doc.range(change.range.clone()).chars().any(|c| c == '\n')
    {
      self.damage_all = true;
    }

    self.doc.apply(&change);

    self.on_change_highlight(&change, start_pos, end_pos);

    self.lsp_notify_change(change);
  }

  fn run_command(&mut self) {
    let Some(command) = self.command.take() else { return };

    let (cmd, args) = command.text.split_once(' ').unwrap_or((&command.text, ""));

    let res = match cmd {
      "w" => {
        self.save().map(|()| format!("{}: written", self.file.as_ref().unwrap().path().display()))
      }
      "q" => {
        if let Some(cmd) = &self.exit_cmd {
          cmd();
        }
        Ok("exiting".to_string())
      }
      "e" => self
        .open(Path::new(args))
        .map(|()| format!("{}: opened", self.file.as_ref().unwrap().path().display())),

      _ => Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("unknown command: {}", cmd),
      )),
    };

    match res {
      Ok(m) => self.status = Some(Status::for_success(m)),
      Err(e) => self.status = Some(Status::for_error(e)),
    }
  }
}

impl CommandState {
  fn perform_move(&mut self, m: Move) {
    match m {
      Move::Single(Direction::Left) => self.move_cursor(-1),
      Move::Single(Direction::Right) => self.move_cursor(1),

      _ => {}
    }
  }
  fn perform_edit(&mut self, e: be_input::Edit) {
    use be_input::Edit;

    match e {
      Edit::Insert(c) => {
        self.text.insert(self.cursor, c);
        self.move_cursor(1);
      }
      Edit::Delete => {
        self.delete_graphemes(1);
      }
      Edit::Backspace => {
        if self.cursor > 0 {
          self.move_cursor(-1);
          self.delete_graphemes(1);
        }
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

  fn delete_graphemes(&mut self, len: usize) {
    let count = self.text[self.cursor..].graphemes(true).take(len).map(|g| g.len()).sum::<usize>();
    self.text.replace_range(self.cursor..self.cursor + count, "");
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
