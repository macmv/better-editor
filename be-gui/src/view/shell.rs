//! A minimal shell: one line of text at the bottom. Enter runs it with
//! `sh -c` in a pty, and the output is shown above the line, after a header
//! row with the command.

use std::path::Path;

use be_input::{Action, Clipboard, Direction, Edit, Mode, Move};
use be_shared::SharedHandle;
use be_shell::{Job, JobId, Shell, ShellEvent};
use be_terminal::{Arrow, InputModes, Position, keys};
use kurbo::{Point, Rect, Size, Stroke};

use super::terminal_paint::{CursorDraw, DrawOptions, TerminalPainter, character_size, grid_size};
use crate::{Font, Layout, MouseEvent, Render, RenderStore};

/// The columns taken by the `% ` before a command.
const SYMBOL_COLS: usize = 2;

pub struct ShellView {
  shell:     Shell,
  clipboard: SharedHandle<Clipboard>,

  input:   InputLine,
  focused: bool,
  waker:   bool,

  /// Pixels scrolled up from the bottom.
  scroll:    f64,
  /// Rows of output of each job, in the order of `Shell::jobs`.
  rows:      Vec<usize>,
  char_size: Size,
  size:      Size,
  grid:      be_terminal::Size,
  painters:  Vec<(JobId, TerminalPainter)>,
}

impl ShellView {
  pub fn new(clipboard: &SharedHandle<Clipboard>, path: &Path) -> Self {
    ShellView {
      shell:     Shell::new(path),
      clipboard: clipboard.clone(),
      input:     InputLine::default(),
      focused:   false,
      waker:     false,
      scroll:    0.0,
      rows:      vec![],
      char_size: Size::new(1.0, 1.0),
      size:      Size::ZERO,
      grid:      be_terminal::Size { rows: 24, cols: 80 },
      painters:  vec![],
    }
  }

  pub fn animated(&self) -> bool { false }

  pub fn mode(&self) -> Mode { Mode::Insert }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn on_mouse(
    &mut self,
    ev: &MouseEvent,
    _size: Size,
    _store: &RenderStore,
  ) -> crate::CursorKind {
    if let MouseEvent::Scroll { delta, .. } = *ev {
      let max = (self.total_height() - self.size.height).max(0.0);
      self.scroll = (self.scroll + delta.y).clamp(0.0, max);
    }
    crate::CursorKind::Default
  }

  pub fn perform_action(&mut self, action: Action) {
    if let Some(job) = self.shell.running() {
      let id = job.id;
      let modes = job.emulator.input_modes();
      let bytes = match &action {
        Action::ClipboardPaste => Some(keys::paste(&self.clipboard.paste(), modes)),
        other => encode_action(other, modes),
      };
      if let Some(bytes) = bytes {
        self.scroll = 0.0;
        self.shell.send_input(id, &bytes);
      }
      return;
    }

    match &action {
      Action::Edit { e: Edit::Insert('\n'), .. } | Action::Control { char: '\n' } => self.submit(),
      Action::Edit { e: Edit::Insert(c), .. } => self.input.insert(*c),
      Action::Edit { e: Edit::Backspace, .. } => self.input.backspace(),
      Action::Edit { e: Edit::Delete(_), .. } => self.input.delete(),
      Action::Move { m: Move::Single(Direction::Left), .. } => self.input.left(),
      Action::Move { m: Move::Single(Direction::Right), .. } => self.input.right(),
      Action::Control { char: 'a' } => self.input.home(),
      Action::Control { char: 'e' } => self.input.end(),
      Action::Control { char: 'b' } => self.input.left(),
      Action::Control { char: 'f' } => self.input.right(),
      Action::Control { char: 'u' } => self.input.kill_to_start(),
      Action::Control { char: 'k' } => self.input.kill_to_end(),
      Action::Control { char: 'w' } => self.input.kill_word_back(),
      Action::Control { char: 'c' } => self.input.clear(),
      Action::Control { char: 'l' } => self.shell.clear_finished(),
      Action::ClipboardPaste => self.input.insert_str(&self.clipboard.paste()),
      _ => return,
    }
    // Typing brings the prompt back into view.
    if !matches!(action, Action::Move { .. }) {
      self.scroll = 0.0;
    }
  }

  fn submit(&mut self) {
    let line = self.input.take();
    if line.trim().is_empty() {
      return;
    }
    if let Err(e) = self.shell.submit(&line, self.grid) {
      log::warn!("failed to run {line:?}: {e}");
    }
  }

  fn prompt_rows(&self) -> usize { if self.shell.running().is_some() { 0 } else { 1 } }

  fn total_height(&self) -> f64 {
    let rows: usize = self.rows.iter().map(|r| 1 + r).sum::<usize>() + self.prompt_rows();
    rows as f64 * self.char_size.height
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    puffin::profile_function!();

    if !self.waker {
      self.waker = true;
      let waker = layout.notifier();
      self.shell.set_wake(move || waker.wake());
    }

    for event in self.shell.poll() {
      if let ShellEvent::Finished(..) = event {
        self.scroll = 0.0;
      }
    }

    self.size = layout.size();
    self.char_size = character_size(layout.store);
    let grid = grid_size(self.size, self.char_size);
    let grid = be_terminal::Size { rows: grid.rows.max(5), cols: grid.cols.max(10) };
    if grid != self.grid {
      self.grid = grid;
      self.shell.resize_running(grid);
    }

    self.rows = self.shell.jobs().iter().map(output_rows).collect();
    let max = (self.total_height() - self.size.height).max(0.0);
    self.scroll = self.scroll.clamp(0.0, max);

    // There is no wakeup for output left in the pty to keep this frame short.
    if self.shell.needs_poll() {
      layout.notifier().wake();
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    let size = render.size();
    render.fill(&Rect::from_origin_size(Point::ZERO, size), render.theme().background);

    // A program on the alternate screen gets the whole view.
    if let Some(job) = self.shell.running().filter(|j| j.emulator.alt_screen()) {
      let state = job.emulator.state();
      let mut options =
        DrawOptions::screen(state, Rect::from_origin_size(Point::ZERO, size), self.grid.rows);
      options.cursor = state
        .cursor
        .visible
        .then(|| CursorDraw { pos: state.cursor.pos, focused: self.focused });
      painter(&mut self.painters, job.id).draw(render, state, &options);
      return;
    }

    let ch = self.char_size;
    let mut y = -(self.total_height() - size.height).max(0.0) + self.scroll;
    let mut drawn = vec![];

    for (i, job) in self.shell.jobs().iter().enumerate() {
      let rows = self.rows[i];
      let height = (1 + rows) as f64 * ch.height;
      if y + height >= 0.0 && y <= size.height {
        let text_dim = render.theme().text_dim;
        let header = match job.exit {
          Some(exit) if exit != 0 => format!("% {}  (exit {exit})", header_command(&job.line)),
          _ => format!("% {}", header_command(&job.line)),
        };
        let layout = render.layout_text(Font::Terminal, &header, text_dim);
        render.draw_text(&layout, (0.0, y));

        if rows > 0 {
          drawn.push(job.id);
          let painter = painter(&mut self.painters, job.id);
          draw_output(render, painter, job, y + ch.height, rows, self.focused, ch);
        }
      }
      y += height;
    }
    // Painters keep layouts for every line they drew, so forget the ones that
    // are off screen.
    self.painters.retain(|(id, _)| drawn.contains(id));

    if self.prompt_rows() > 0 {
      self.draw_prompt(render, y);
    }
  }

  /// Draws `% ` and the command line, with its cursor, in the row at `y`. A
  /// line wider than the view shows the part around the cursor.
  fn draw_prompt(&self, render: &mut Render, y: f64) {
    let ch = self.char_size;
    let avail = self.grid.cols.saturating_sub(SYMBOL_COLS + 1).max(1);

    let text = self.input.text();
    let cursor_col = text[..self.input.cursor()].chars().count();
    let skip = (cursor_col + 1).saturating_sub(avail);
    let shown: String = text.chars().skip(skip).collect();

    let line = format!("% {shown}");
    let layout = render.layout_text(Font::Terminal, &line, render.theme().text);
    render.draw_text(&layout, (0.0, y));

    let cell = Rect::from_origin_size(
      (((SYMBOL_COLS + cursor_col - skip) as f64 * ch.width).round(), y.round()),
      (ch.width.ceil(), ch.height.ceil()),
    );
    if self.focused {
      render.fill(&cell, render.theme().text);
      if let Some(c) = text[self.input.cursor()..].chars().next()
        && !c.is_whitespace()
      {
        let layout = render.layout_text(Font::Terminal, &c.to_string(), render.theme().background);
        render.draw_text(&layout, (cell.x0, cell.y0));
      }
    } else {
      render.stroke(&cell.inset(-0.5 * render.scale()), render.theme().text, Stroke::new(1.0));
    }
  }
}

fn painter(painters: &mut Vec<(JobId, TerminalPainter)>, id: JobId) -> &mut TerminalPainter {
  let index = match painters.iter().position(|(i, _)| *i == id) {
    Some(index) => index,
    None => {
      painters.push((id, TerminalPainter::default()));
      painters.len() - 1
    }
  };
  &mut painters[index].1
}

/// The bytes a running job should receive for `action`, if it is a key.
/// Pasting is handled separately, as it needs the clipboard.
fn encode_action(action: &Action, modes: InputModes) -> Option<Vec<u8>> {
  Some(match action {
    Action::Move { m: Move::Single(dir), .. } => keys::arrow(
      match dir {
        Direction::Up => Arrow::Up,
        Direction::Down => Arrow::Down,
        Direction::Left => Arrow::Left,
        Direction::Right => Arrow::Right,
      },
      modes,
    ),
    Action::Edit { e: Edit::Insert('\n'), .. } => keys::input('\r'),
    Action::Edit { e: Edit::Insert(c), .. } => keys::input(*c),
    Action::Edit { e: Edit::Backspace, .. } => keys::backspace(),
    Action::Edit { e: Edit::Delete(_), .. } => keys::delete(),
    Action::Control { char: '\n' } => keys::ctrl_enter(modes),
    Action::Control { char: c @ 'a'..='z' } => keys::control(*c as u8 - b'a' + 1),
    Action::Control { char: c @ ('[' | '\\' | ']' | '^' | '_') } => keys::control(*c as u8 & 0x1f),
    // We are in insert mode, so escape arrives as this.
    Action::SetMode { mode: Mode::Normal, .. } => keys::escape(),
    Action::Tab { shift: false } => keys::tab(),
    Action::Tab { shift: true } => keys::shift_tab(),
    _ => return None,
  })
}

/// The rows of a job's output worth showing: all of scrollback and screen,
/// minus blank rows at the end. A running job keeps the row its cursor is on
/// once it has printed something.
fn output_rows(job: &Job) -> usize {
  let emulator = &job.emulator;
  let state = emulator.state();
  let used = emulator.used_rows();

  let mut rows = used;
  while rows > 0 && state.absolute_line(rows - 1).is_none_or(|l| l.to_string().trim().is_empty()) {
    rows -= 1;
  }

  if rows > 0 && job.is_running() {
    rows = rows.max(state.scrollback_len() + state.cursor.pos.row + 1).min(used);
  }
  rows
}

/// A command for display in a one-line header.
fn header_command(line: &str) -> String {
  line.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Draws the visible part of a job's output, which starts at `top`.
fn draw_output(
  render: &mut Render,
  painter: &mut TerminalPainter,
  job: &Job,
  top: f64,
  rows: usize,
  focused: bool,
  char_size: Size,
) {
  let height = render.size().height;
  let first = ((-top) / char_size.height).floor().max(0.0) as usize;
  let last = (((height - top) / char_size.height).ceil().max(0.0) as usize).min(rows);
  if first >= last {
    return;
  }

  let state = job.emulator.state();
  let cursor = (job.is_running() && state.cursor.visible).then(|| {
    let row = state.scrollback_len() + state.cursor.pos.row;
    CursorDraw {
      pos: Position { row: row.saturating_sub(first), col: state.cursor.pos.col },
      focused,
    }
  });
  let cursor = cursor.filter(|c| c.pos.row < last - first);

  let options = DrawOptions {
    rect: Rect::from_origin_size(
      (0.0, top + first as f64 * char_size.height),
      (render.size().width, (last - first) as f64 * char_size.height),
    ),
    first_row: first,
    rows: last - first,
    pixel_offset: 0.0,
    selection: None,
    cursor,
  };
  painter.draw(render, state, &options);
}

/// A single line of text with a cursor.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct InputLine {
  text:   String,
  /// A byte offset into `text`, always on a char boundary.
  cursor: usize,
}

impl InputLine {
  fn text(&self) -> &str { &self.text }
  fn cursor(&self) -> usize { self.cursor }

  fn clear(&mut self) {
    self.text.clear();
    self.cursor = 0;
  }

  /// Takes the text, leaving the line empty.
  fn take(&mut self) -> String {
    self.cursor = 0;
    std::mem::take(&mut self.text)
  }

  fn insert(&mut self, c: char) {
    if c.is_control() {
      return;
    }
    self.text.insert(self.cursor, c);
    self.cursor += c.len_utf8();
  }

  /// Inserts pasted text. Line breaks and tabs become spaces, as this is a
  /// single line, and other control characters are dropped.
  fn insert_str(&mut self, s: &str) {
    for c in s.chars() {
      match c {
        '\n' | '\t' => self.insert(' '),
        '\r' if s.contains('\n') => {}
        '\r' => self.insert(' '),
        c => self.insert(c),
      }
    }
  }

  fn backspace(&mut self) {
    let Some(prev) = self.prev_boundary() else { return };
    self.text.replace_range(prev..self.cursor, "");
    self.cursor = prev;
  }

  fn delete(&mut self) {
    let Some(next) = self.next_boundary() else { return };
    self.text.replace_range(self.cursor..next, "");
  }

  fn left(&mut self) {
    if let Some(prev) = self.prev_boundary() {
      self.cursor = prev;
    }
  }

  fn right(&mut self) {
    if let Some(next) = self.next_boundary() {
      self.cursor = next;
    }
  }

  fn home(&mut self) { self.cursor = 0; }
  fn end(&mut self) { self.cursor = self.text.len(); }

  fn kill_to_start(&mut self) {
    self.text.replace_range(..self.cursor, "");
    self.cursor = 0;
  }

  fn kill_to_end(&mut self) { self.text.truncate(self.cursor); }

  /// Deletes the word before the cursor, and any spaces after it.
  fn kill_word_back(&mut self) {
    let before = &self.text[..self.cursor];
    let start = before
      .trim_end()
      .char_indices()
      .rev()
      .find(|(_, c)| c.is_whitespace())
      .map_or(0, |(i, c)| i + c.len_utf8());
    self.text.replace_range(start..self.cursor, "");
    self.cursor = start;
  }

  fn prev_boundary(&self) -> Option<usize> {
    self.text[..self.cursor].char_indices().next_back().map(|(i, _)| i)
  }

  fn next_boundary(&self) -> Option<usize> {
    self.text[self.cursor..].chars().next().map(|c| self.cursor + c.len_utf8())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keys_for_a_running_job() {
    let normal = InputModes::default();
    let insert = |c| Action::Edit { count: None, e: Edit::Insert(c) };

    assert_eq!(encode_action(&insert('x'), normal), Some(b"x".to_vec()));
    assert_eq!(encode_action(&insert('\n'), normal), Some(b"\r".to_vec()));
    assert_eq!(encode_action(&Action::Control { char: 'c' }, normal), Some(vec![3]));
    assert_eq!(encode_action(&Action::Yank, normal), None);
  }

  #[test]
  fn editing_the_line() {
    let mut line = InputLine::default();
    for c in "git status".chars() {
      line.insert(c);
    }
    line.kill_word_back();
    assert_eq!(line.text(), "git ");
    line.home();
    assert_eq!(line.cursor(), 0);
    line.end();
    line.insert_str("a\nb");
    assert_eq!(line.text(), "git a b");
  }
}
