use std::{cell::RefCell, collections::HashSet, ops::Range, rc::Rc};

use be_config::{Config, LanguageName};
use be_doc::{Change, Column, Cursor, Document, Edit, Line, crop::RopeSlice};
use be_git::{LineDiffSimilarity, Repo};
use be_input::{Action, Direction, Mode, Move, VerticalDirection};
use unicode_segmentation::UnicodeSegmentation;

use crate::{fs::OpenedFile, status::Status};

mod edit;
mod filetype;
mod fs;
mod highlight;
mod lsp;
mod moves;
mod status;
mod treesitter;

#[cfg(test)]
mod tests;

pub use highlight::HighlightKey;
pub use lsp::{Diagnostic, DiagnosticLevel};

#[macro_use]
extern crate log;

#[cfg(test)]
#[macro_use]
extern crate expect_test;

#[derive(Default)]
pub struct EditorState {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,

  file:        Option<OpenedFile>,
  status:      Option<Status>,
  command:     Option<CommandState>,
  search_text: Option<String>,

  filetype:   Option<LanguageName>,
  highligher: Option<treesitter::Highlighter>,
  damages:    HashSet<Line>,
  damage_all: bool,

  current_edit:     Option<Edit>,
  history_position: usize,
  history:          Vec<Edit>,

  pub config:  Rc<RefCell<Config>>,
  pub lsp:     lsp::LspState,
  pub run_cmd: Option<Box<dyn Fn(&str)>>,

  // TODO: Share this
  repo:        Option<Repo>,
  pub changes: Option<LineDiffSimilarity>,
}

#[derive(Default)]
pub struct CommandState {
  pub text:   String,
  pub mode:   CommandMode,
  pub cursor: usize, // in bytes
}

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum CommandMode {
  #[default]
  Command,
  Search,
}

#[derive(Copy, Clone)]
pub struct IndentLevel(pub usize);

impl IndentLevel {
  pub const ZERO: IndentLevel = IndentLevel(0);
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
  pub fn file_type(&self) -> Option<LanguageName> { self.filetype }
  pub fn take_damage_all(&mut self) -> bool { std::mem::take(&mut self.damage_all) }
  pub fn take_damages(&mut self) -> impl Iterator<Item = Line> { self.damages.drain() }
  pub fn progress(&self) -> Vec<String> { self.lsp.progress() }

  pub fn update(&mut self) {
    if self.repo.is_none() {
      self.repo = Some(Repo::open(std::path::Path::new(".")));
    }

    self.lsp_update_diagnostics();
    self.update_save_task();

    if let Some(repo) = &self.repo {
      if let Some(file) = &self.file.as_ref() {
        if let Some(diff) = repo.changes_in(file.path()) {
          self.changes = Some(diff);
        }
      }
    }
  }

  fn on_open_file(&mut self) {
    let Some(_) = self.file.as_ref() else { return };

    self.move_to_line(Line(0));
    self.move_to_col(Column(0));

    self.detect_filetype();
    self.on_open_file_highlight();
    self.connect_to_lsp();

    if self.repo.is_none() {
      self.repo = Some(Repo::open(std::path::Path::new(".")));
    }
    if let Some(repo) = &mut self.repo {
      repo.open_file(self.file.as_ref().unwrap().path());
    }
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
        let target_line = self.doc.line_of_byte(change.range.end);
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

    match m {
      Mode::Normal => {
        self.trim_line(self.cursor.line);
        self.remove_current_edit();
        self.lsp.completions.hide();
      }

      Mode::Insert => {
        self.current_edit = Some(Edit::empty());
      }

      _ => {}
    }
  }

  fn remove_current_edit(&mut self) {
    if let Some(edit) = self.current_edit.take() {
      self.add_to_history(edit);
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

        if mode == Mode::Insert {
          self.auto_indent(VerticalDirection::Up);
        }
      }
      Action::OpenSearch => {
        self.set_mode(Mode::Command);
        self.command.as_mut().unwrap().mode = CommandMode::Search;
      }
      Action::Append { after } => {
        self.set_mode(Mode::Insert);

        if after {
          let target = self.doc.byte_of_line(self.cursor().line + 1);
          self.change(Change::insert(target, "\n"));
          self.move_to_line(self.cursor.line + 1);
          self.move_to_col(Column(0));
          self.auto_indent(VerticalDirection::Up);
        } else {
          let target = self.doc.byte_of_line(self.cursor().line);
          self.change(Change::insert(target, "\n"));
          self.move_to_col(Column(0));
          self.auto_indent(VerticalDirection::Down);
        }
      }
      Action::Move { count: _, m } => self.perform_move(m),
      Action::Edit { count: _, e } => self.perform_edit(e),
      Action::Autocomplete => self.perform_autocomplete(),
      Action::Navigate { nav } => error!("unhandled navigate passed to editor: {nav:?}"),
      Action::Control { .. } => {} // only really used for the terminal
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

  fn damage_line(&mut self, line: Line) { self.damages.insert(line); }

  fn damage_range(&mut self, range: Range<usize>) {
    let start_line = self.doc.line_of_byte(range.start);
    let end_line = self.doc.line_of_byte(range.end);

    if self.doc.range(range.clone()).chars().any(|c| c == '\n') {
      self.damage_all = true;
    } else {
      for line in start_line.0..=end_line.0 {
        self.damage_line(Line(line));
      }
    }
  }

  fn change_no_history(&mut self, change: Change) {
    let start_pos = self.offset_to_ts_point(change.range.start);
    let end_pos = self.offset_to_ts_point(change.range.end);

    for line in start_pos.row..=end_pos.row {
      self.damage_line(Line(line));
    }

    if change.text.contains('\n') || self.doc.range(change.range.clone()).chars().any(|c| c == '\n')
    {
      self.damage_all = true;
    }

    self.lsp_notify_change(&change);

    self.doc.apply(&change);

    self.on_change_highlight(&change, start_pos, end_pos);

    if let Some(repo) = &mut self.repo {
      if let Some(file) = &self.file.as_ref() {
        repo.update_file(file.path(), &self.doc);
      }
    }
  }

  fn run_command(&mut self) {
    let Some(command) = self.command.take() else { return };

    match command.mode {
      CommandMode::Search => {
        self.search_text = Some(command.text);
        self.damage_all = true;
        self.status = None;
      }
      CommandMode::Command => {
        if let Some(cmd) = &self.run_cmd {
          cmd(&command.text);
        }

        /*
        match res {
          Ok(m) => self.status = Some(Status::for_success(m)),
          Err(e) => self.status = Some(Status::for_error(e)),
        }
        */
      }
    }
  }

  fn update_save_task(&mut self) {
    if self.lsp.save_task.is_some() {
      self.lsp_finish_on_save();

      if self.lsp.save_task.is_none() {
        let res = self
          .save()
          .map(|()| format!("{}: written", self.file.as_ref().unwrap().path().display()));

        match res {
          Ok(m) => self.status = Some(Status::for_success(m)),
          Err(e) => self.status = Some(Status::for_error(e)),
        }
      }
    }
  }

  pub fn trim_line(&mut self, line: Line) {
    let slice = self.doc.line(line);
    let whitespace =
      slice.chars().rev().take_while(|c| c.is_whitespace()).map(|c| c.len_utf8()).sum::<usize>();

    if whitespace != 0 {
      let end = self.doc.byte_of_line_end(line);
      self.change(Change::remove(end - whitespace..end));
    }
    if line == self.cursor.line {
      self.clamp_column();
    }
  }

  pub fn auto_indent(&mut self, direction: VerticalDirection) {
    if self.cursor.column != 0 || !self.doc.line(self.cursor.line).is_empty() {
      return;
    }

    let line = self.cursor.line;
    let indent = self.guess_indent(line, direction);
    let columns = indent.0 * self.config.borrow().settings.editor.indent_width as usize;
    let indent_str = " ".repeat(columns);
    self.change(Change::insert(self.doc.byte_of_line(line), &indent_str));
    self.move_col_rel(columns as i32);
  }

  pub fn fix_indent(&mut self) {
    let line = self.doc.line(self.cursor.line);
    let mut iter = line.bytes().rev();
    if !matches!(iter.next(), Some(b'}' | b']' | b')')) {
      return;
    }
    let whitespace = iter.len();
    if !iter.all(|c| c.is_ascii_whitespace()) {
      return;
    }

    let mut indent = self.guess_indent(self.cursor.line, VerticalDirection::Up);
    indent.0 = indent.0.saturating_sub(1);

    let columns = indent.0 * self.config.borrow().settings.editor.indent_width as usize;
    let indent_str = " ".repeat(columns);
    self.change(Change::replace(
      self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line(self.cursor.line) + whitespace,
      &indent_str,
    ));
    self.move_to_col(be_doc::Column(columns + 1));
  }

  pub fn guess_indent(&self, line: Line, direction: VerticalDirection) -> IndentLevel {
    {
      let line = self.doc.line(line);
      if !line.chars().all(|c| c.is_whitespace()) {
        return IndentLevel::guess(&self.config.borrow().settings.editor, line);
      }
    }

    match direction {
      VerticalDirection::Up => {
        if let Some(prev) = self.prev_non_empty_line(line) {
          let mut level = IndentLevel::guess(&self.config.borrow().settings.editor, prev);
          for c in prev.chars().rev() {
            match c {
              '{' | '(' | '[' => level.0 += 1,
              ' ' => {}
              _ => break,
            }
          }
          level
        } else {
          IndentLevel::ZERO
        }
      }
      VerticalDirection::Down => {
        if let Some(next) = self.next_non_empty_line(line) {
          let mut level = IndentLevel::guess(&self.config.borrow().settings.editor, next);
          for c in next.chars() {
            match c {
              '}' | ')' | ']' => level.0 += 1,
              ' ' => {}
              _ => break,
            }
          }
          level
        } else {
          IndentLevel::ZERO
        }
      }
    }
  }

  fn prev_non_empty_line(&self, mut line: Line) -> Option<RopeSlice<'_>> {
    while line.0 > 0 {
      line.0 -= 1;
      let line = self.doc.line(line);
      if !line.chars().all(|c| c.is_whitespace()) {
        return Some(line);
      }
    }
    None
  }

  fn next_non_empty_line(&self, mut line: Line) -> Option<RopeSlice<'_>> {
    while line.0 < self.doc.len_lines() {
      line.0 += 1;
      let line = self.doc.line(line);
      if !line.chars().all(|c| c.is_whitespace()) {
        return Some(line);
      }
    }
    None
  }

  pub fn begin_save(&mut self) {
    self.lsp_on_save();

    if self.lsp.save_task.is_some() {
      self.status = Some(Status::for_success("saving..."));
    } else {
      match self.save() {
        Ok(()) => {
          self.status = Some(Status::for_success(format!(
            "{}: written",
            self.file.as_ref().unwrap().path().display()
          )))
        }
        Err(e) => self.status = Some(Status::for_error(e)),
      }
    }
  }

  pub fn clear_search(&mut self) {
    self.search_text = None;
    self.damage_all = true;
    self.status = None;
  }
}

impl IndentLevel {
  pub fn guess(config: &be_config::EditorSettings, line: RopeSlice<'_>) -> IndentLevel {
    let mut width = 0;
    for c in line.chars() {
      match c {
        '\t' => {} // TODO
        ' ' => width += 1,
        _ => break,
      }
    }

    IndentLevel(width / config.indent_width as usize)
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
      Edit::Delete(Move::Single(Direction::Right)) => {
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
