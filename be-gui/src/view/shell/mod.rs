//! A shell made of blocks that reads like a terminal. Every command line is a
//! block: the prompt as it was when the line was typed (frozen, with the
//! command after it), then the output. A live prompt in the same style sits
//! below the blocks.
//!
//! Everything starts at the left edge of the view, like in a terminal. The
//! chevron that collapses a block is the first element of its info line (the
//! live prompt keeps the same two cells blank, so nothing shifts when a
//! prompt is frozen into a block). Rows are all one terminal cell tall:
//!
//! ```text
//! blank row                 \
//! info line (chevron first)  | the header of a block, `header_rows` rows
//! symbol line + command      | (more if the command wraps or has newlines)
//! (continuation rows)       /
//! output rows (or a `+N lines` hint row when collapsed)
//! ```
//!
//! The pure pieces live in `shell_layout` (blocks, scrolling), `shell_prompt`
//! (the prompt text) and `shell_input` (the command line), and the output of
//! each block is drawn by a `TerminalPainter` over that job's emulator.

mod input;
mod layout;
mod prompt;

use std::{
  collections::HashMap,
  io,
  path::Path,
  time::{Duration, SystemTime},
};

use be_input::{Action, Clipboard, Direction, Edit, KeyStroke, Mode, Move};
use be_shared::SharedHandle;
use be_shell::{
  GitHead, Job, JobId, JobState, PromptContext, STARTUP_ID, Shell, ShellEvent, SubmitResult,
  UiCommand, format_duration, format_local_time,
};
use be_terminal::{Arrow, InputModes, Position, keys, mouse};
use kurbo::{Point, Rect, RoundedRect, Size, Stroke};

use super::terminal_paint::{
  CursorDraw, DrawOptions, Span, TerminalPainter, character_size, styled_line,
};
use crate::{
  Color, Font, Layout, MouseButton, MouseEvent, Render, RenderStore, TextLayout, icon,
  spinner::Spinner,
};
use input::{HistoryNav, InputLine};
use layout::{
  BlockKind, Blocks, Item, PROMPT_KEY, ScrollState, header_command, is_fullscreen, mouse_cell,
  pty_grid, stack,
};
use prompt::{
  BlockResult, Chevron, Element, Kind, SYMBOL_COLS, Wrapped, prompt_elements, symbol, symbol_color,
};

/// Padding to the right of the text.
const PAD_X: f64 = 8.0;
/// Vertical padding inside the tooltip.
const TIP_PAD_Y: f64 = 4.0;
/// How far a pill reaches past the cells of its text, in cells. This is under
/// half a cell so that pills one cell apart never touch, and it only adds
/// room around the text: it never moves a cell.
const PILL_PAD_X: f64 = 0.35;
/// The space above and below a pill inside its row, in pixels.
const PILL_INSET_Y: f64 = 2.0;
/// How opaque the tinted background of a pill is.
const PILL_ALPHA: f32 = 0.18;
/// How opaque the band behind the header of the focused block is.
const FOCUS_ALPHA: f32 = 0.12;
/// Lines of `history` shown.
const HISTORY_LINES: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellMode {
  /// Keys edit the command line (or go to the running job).
  Input,
  /// Keys move between blocks and collapse them.
  Blocks,
}

pub struct ShellView {
  shell:     Shell,
  clipboard: SharedHandle<Clipboard>,

  blocks:    Blocks,
  scroll:    ScrollState,
  input:     InputLine,
  history:   HistoryNav,
  /// Captured when the shell state changes, as capturing it spawns a git
  /// status.
  prompt:    PromptContext,
  mode:      ShellMode,
  /// `z` was pressed in block mode, and a second key is expected.
  pending_z: bool,
  /// Block to scroll into view on the next layout.
  reveal:    Option<u64>,

  focused:       bool,
  set_waker:     bool,
  should_close:  bool,
  auto_collapse: usize,

  geom:     Geometry,
  /// The size of the pty given to new jobs, and kept up to date for running
  /// ones.
  grid:     be_terminal::Size,
  painters: HashMap<JobId, TerminalPainter>,

  /// The running job that has the alternate screen and so fills the view, as
  /// of the last layout.
  fullscreen: Option<JobId>,
  /// The mouse button held down in a fullscreen job.
  mouse_held: Option<u8>,

  mouse_pos: Option<Point>,
  drag:      Option<Selection>,
  selection: Option<Selection>,

  spinner: Spinner,
}

/// What `layout` measured, for `draw` and the mouse.
#[derive(Default)]
struct Geometry {
  items:       Vec<Item>,
  total:       f64,
  /// Rows of output of each block, in the order of `Blocks::list`.
  rows:        Vec<usize>,
  size:        Size,
  char_size:   Size,
  ui_line:     f64,
  /// Columns of text.
  cols:        usize,
  /// Rows of each block's header (zero for notices), in the order of
  /// `Blocks::list`.
  header_rows: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Selection {
  key:    u64,
  anchor: Position,
  head:   Position,
}

impl Selection {
  /// The selected cells in reading order.
  fn range(&self) -> (Position, Position) { ordered(self.anchor, self.head) }
}

fn ordered(a: Position, b: Position) -> (Position, Position) {
  if a.row == b.row {
    let (min, max) = (a.col.min(b.col), a.col.max(b.col));
    (Position { row: a.row, col: min }, Position { row: a.row, col: max })
  } else if a.row < b.row {
    (a, b)
  } else {
    (b, a)
  }
}

/// What the view has to do after the command line handled an action.
#[derive(Debug, PartialEq, Eq)]
enum LineCmd {
  None,
  /// The line changed.
  Edited,
  Submit,
  HistoryPrev,
  HistoryNext,
  ClearScreen,
  /// Leave the command line for the blocks.
  Blocks,
  Paste,
}

/// Applies an editing action to the command line.
fn edit_line(line: &mut InputLine, action: &Action) -> LineCmd {
  match action {
    Action::Edit { e: Edit::Insert('\n'), .. } => LineCmd::Submit,
    Action::Edit { e: Edit::Insert(c), .. } => {
      line.insert(*c);
      LineCmd::Edited
    }
    Action::Edit { e: Edit::Backspace, .. } => {
      line.backspace();
      LineCmd::Edited
    }
    Action::Edit { e: Edit::Delete(_), .. } => {
      line.delete();
      LineCmd::Edited
    }
    Action::Move { m: Move::Single(Direction::Left), .. } => {
      line.left();
      LineCmd::None
    }
    Action::Move { m: Move::Single(Direction::Right), .. } => {
      line.right();
      LineCmd::None
    }
    Action::Move { m: Move::Single(Direction::Up), .. } => LineCmd::HistoryPrev,
    Action::Move { m: Move::Single(Direction::Down), .. } => LineCmd::HistoryNext,
    Action::MoveCompletion { next: false } => LineCmd::HistoryPrev,
    Action::MoveCompletion { next: true } => LineCmd::HistoryNext,
    Action::Control { char } => match char {
      'a' => {
        line.home();
        LineCmd::None
      }
      'e' => {
        line.end();
        LineCmd::None
      }
      'b' => {
        line.left();
        LineCmd::None
      }
      'f' => {
        line.right();
        LineCmd::None
      }
      'u' => {
        line.kill_to_start();
        LineCmd::Edited
      }
      'k' => {
        line.kill_to_end();
        LineCmd::Edited
      }
      'w' => {
        line.kill_word_back();
        LineCmd::Edited
      }
      // Like a terminal, Ctrl-C abandons the line.
      'c' => {
        line.clear();
        LineCmd::Edited
      }
      'l' => LineCmd::ClearScreen,
      '\n' => LineCmd::Submit,
      _ => LineCmd::None,
    },
    Action::SetMode { mode: Mode::Normal, .. } => LineCmd::Blocks,
    Action::ClipboardPaste => LineCmd::Paste,
    _ => LineCmd::None,
  }
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

/// How a job's prompt shows its outcome.
fn job_result(job: &Job) -> BlockResult {
  match job.state {
    JobState::Running => BlockResult::Running,
    JobState::Done { exit } => BlockResult::Done { exit, duration: job.elapsed() },
  }
}

fn ui_text(render: &mut Render, text: &str, color: Color) -> TextLayout {
  render.layout_text(Font::Ui, text, color)
}

impl ShellView {
  pub fn new(clipboard: &SharedHandle<Clipboard>, path: &Path) -> io::Result<Self> {
    let shell = Shell::new(path)?;
    let prompt = shell.prompt();
    let mut blocks = Blocks::default();
    // The first block: what the rc files print when the shell starts.
    blocks.push(BlockKind::Startup(STARTUP_ID));
    Ok(ShellView {
      shell,
      clipboard: clipboard.clone(),
      blocks,
      scroll: ScrollState::default(),
      input: InputLine::default(),
      history: HistoryNav::default(),
      prompt,
      mode: ShellMode::Input,
      pending_z: false,
      reveal: None,
      focused: false,
      set_waker: false,
      should_close: false,
      auto_collapse: 0,
      geom: Geometry::default(),
      grid: be_terminal::Size { rows: 24, cols: 80 },
      painters: HashMap::new(),
      fullscreen: None,
      mouse_held: None,
      mouse_pos: None,
      drag: None,
      selection: None,
      spinner: Spinner::new(),
    })
  }

  /// Whether something is spinning, so the view must keep redrawing. Only a
  /// running job in the block layout spins; a fullscreen one has no spinner.
  pub fn animated(&self) -> bool { self.running().is_some() && self.fullscreen.is_none() }

  pub fn mode(&self) -> Mode {
    if self.running().is_some() || self.mode == ShellMode::Input {
      Mode::Insert
    } else {
      Mode::Normal
    }
  }

  pub fn on_focus(&mut self, focus: bool) {
    self.focused = focus;
    if let Some(job) = self.running()
      && let Some(bytes) = mouse::focus(job.emulator.mouse_modes(), focus)
    {
      let id = job.id;
      self.shell.send_input(id, &bytes);
    }
  }

  fn running(&self) -> Option<&Job> { self.shell.jobs().iter().find(|j| j.is_running()) }

  // Events and input.

  fn handle_event(&mut self, event: ShellEvent) {
    match event {
      ShellEvent::Output(_) => {}
      ShellEvent::Finished(id, _) => {
        self.prompt = self.shell.prompt();
        if self.auto_collapse > 0
          && let Some(job) = self.shell.job(id)
          && output_rows(job) > self.auto_collapse
          && let Some(block) = self.blocks.job_block(id)
        {
          let key = block.key;
          self.blocks.set_collapsed(key, true);
        }
      }
      ShellEvent::StateChanged => self.prompt = self.shell.prompt(),
      ShellEvent::Exit => self.should_close = true,
      ShellEvent::Clear => {
        self.blocks.clear();
        self.selection = None;
        self.mode = ShellMode::Input;
      }
      ShellEvent::History => {
        let entries = self.shell.history().entries();
        let start = entries.len().saturating_sub(HISTORY_LINES);
        let lines = entries[start..]
          .iter()
          .enumerate()
          .map(|(i, e)| format!("{:>5}  {}", start + i + 1, header_command(&e.command)))
          .collect();
        self.blocks.push(BlockKind::Notice(lines));
      }
      ShellEvent::Unsupported(cmd) => {
        let name = match cmd {
          UiCommand::Jobs => "jobs",
          UiCommand::Fg(_) => "fg",
          UiCommand::Bg(_) => "bg",
          UiCommand::Kill { .. } => "kill",
          UiCommand::Exit => "exit",
          UiCommand::Clear => "clear",
          UiCommand::History => "history",
        };
        self.blocks.push(BlockKind::Notice(vec![format!("{name}: not supported yet")]));
      }
    }
  }

  fn submit(&mut self) {
    let line = self.input.take();
    self.history.reset();
    if line.trim().is_empty() {
      return;
    }

    self.scroll.follow_bottom();
    self.selection = None;
    match self.shell.submit_with(&line, self.grid, Some(self.prompt.clone())) {
      SubmitResult::Started(id) => {
        self.blocks.push(BlockKind::Job(id));
      }
      SubmitResult::Event(event) => self.handle_event(event),
      SubmitResult::Failed(msg) => {
        self.blocks.push(BlockKind::Notice(vec![msg]));
      }
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    // Typing while the rc files run goes to the command line, except Ctrl-C,
    // which stops a rc that hangs.
    if self.shell.startup_running()
      && let Action::Control { char: 'c' } = action
    {
      self.shell.send_input(STARTUP_ID, &[0x03]);
      return;
    }

    if let Some(job) = self.running() {
      let id = job.id;
      let modes = job.emulator.input_modes();

      let bytes = match &action {
        Action::ClipboardCopy => {
          self.copy();
          return; // Don't clear the selection.
        }
        Action::ClipboardPaste => Some(keys::paste(&self.clipboard.paste(), modes)),
        other => encode_action(other, modes),
      };
      if let Some(bytes) = bytes {
        self.selection = None;
        self.scroll.follow_bottom();
        self.shell.send_input(id, &bytes);
      }
      return;
    }

    match self.mode {
      ShellMode::Input => self.perform_input_action(action),
      ShellMode::Blocks => {
        if let Action::SetMode { mode: Mode::Insert, .. } = action {
          self.enter_input();
        }
      }
    }
  }

  fn perform_input_action(&mut self, action: Action) {
    if let Action::ClipboardCopy = action {
      self.copy();
      return; // Don't clear the selection.
    }

    self.selection = None;
    match edit_line(&mut self.input, &action) {
      LineCmd::None => {}
      LineCmd::Edited => self.history.reset(),
      // The prompt is hidden while the rc files run; keep what was typed.
      LineCmd::Submit if self.shell.startup_running() => {}
      LineCmd::Submit => self.submit(),
      LineCmd::HistoryPrev => {
        let shell = &self.shell;
        let next = self
          .history
          .prev(self.input.text(), |prefix| {
            shell.history().search_prefix(prefix, true).iter().map(|e| e.command.clone()).collect()
          })
          .map(str::to_string);
        if let Some(next) = next {
          self.input.set(&next);
        }
      }
      LineCmd::HistoryNext => {
        if let Some(next) = self.history.next().map(str::to_string) {
          self.input.set(&next);
        }
      }
      LineCmd::ClearScreen => {
        self.blocks.clear();
        self.scroll.follow_bottom();
      }
      LineCmd::Blocks => self.enter_blocks(),
      LineCmd::Paste => {
        self.input.insert_str(&self.clipboard.paste());
        self.history.reset();
      }
    }
    // Typing brings the prompt back into view.
    if !matches!(action, Action::Move { .. }) {
      self.scroll.follow_bottom();
    }
  }

  fn enter_blocks(&mut self) {
    if self.blocks.list().iter().all(|b| matches!(b.kind, BlockKind::Startup(_))) {
      return;
    }
    self.mode = ShellMode::Blocks;
    self.pending_z = false;
    self.blocks.ensure_focus();
    self.reveal = self.blocks.focus();
  }

  fn enter_input(&mut self) {
    self.mode = ShellMode::Input;
    self.pending_z = false;
    self.scroll.follow_bottom();
  }

  /// Keys in block mode, which don't go through the keybinds. Returns whether
  /// the key was used.
  pub fn on_raw_key(&mut self, key: &KeyStroke) -> bool {
    use be_input::Key;

    if self.mode != ShellMode::Blocks || self.running().is_some() || key.alt || key.sup {
      return false;
    }

    if std::mem::take(&mut self.pending_z) {
      let focus = self.blocks.focus();
      match key.key {
        Key::Char('a') => {
          if let Some(f) = focus {
            self.blocks.toggle(f);
          }
        }
        Key::Char('o') => {
          if let Some(f) = focus {
            self.blocks.set_collapsed(f, false);
          }
        }
        Key::Char('c') => {
          if let Some(f) = focus {
            self.blocks.set_collapsed(f, true);
          }
        }
        Key::Char('M') => self.blocks.set_all_collapsed(true),
        Key::Char('R') => self.blocks.set_all_collapsed(false),
        _ => {}
      }
      self.reveal = self.blocks.focus();
      return true;
    }

    if key.control {
      let half = self.geom.size.height / 2.0;
      return match key.key {
        Key::Char('d') => {
          self.scroll_by(half);
          true
        }
        Key::Char('u') => {
          self.scroll_by(-half);
          true
        }
        _ => false,
      };
    }

    match key.key {
      Key::Char('j') | Key::ArrowDown => {
        self.blocks.move_focus(1);
        self.reveal = self.blocks.focus();
      }
      Key::Char('k') | Key::ArrowUp => {
        self.blocks.move_focus(-1);
        self.reveal = self.blocks.focus();
      }
      Key::Char('G') => {
        self.blocks.move_focus(isize::MAX / 2);
        self.reveal = self.blocks.focus();
      }
      Key::Char('\n') => {
        if let Some(f) = self.blocks.focus() {
          self.blocks.toggle(f);
          self.reveal = Some(f);
        }
      }
      Key::Char('z') => self.pending_z = true,
      Key::Char('i' | 'a') | Key::Escape => self.enter_input(),
      _ => return false,
    }
    true
  }

  fn scroll_by(&mut self, dy: f64) {
    self.scroll.scroll_by(dy, &self.geom.items, self.geom.total, self.geom.size.height);
  }

  // Mouse.

  /// The block under a point in view coordinates, and the point's position
  /// relative to it.
  fn block_at(&self, pos: Point) -> Option<(usize, Point)> {
    let y = pos.y + self.scroll.offset();
    let i = self.geom.items.iter().position(|i| y >= i.top && y < i.top + i.height)?;
    let item = self.geom.items[i];
    (item.key != PROMPT_KEY).then(|| (i, Point::new(pos.x, y - item.top)))
  }

  /// The height of the header of the block at index `i`.
  fn header_height(&self, i: usize) -> f64 {
    self.geom.header_rows.get(i).copied().unwrap_or(0) as f64 * self.geom.char_size.height
  }

  /// The cell of a block's output at `rel`, clamped to its rows.
  fn output_cell(&self, i: usize, rel: Point) -> Position {
    let rows = self.geom.rows[i];
    let ch = self.geom.char_size;
    let row = ((rel.y - self.header_height(i)) / ch.height).floor().max(0.0) as usize;
    Position {
      row: row.min(rows.saturating_sub(1)),
      col: (rel.x / ch.width).round().max(0.0) as usize,
    }
  }

  /// Whether `rel` (relative to block `i`) is over its header.
  fn in_header(&self, i: usize, rel: Point) -> bool { rel.y < self.header_height(i) }

  pub fn on_mouse(
    &mut self,
    ev: &MouseEvent,
    _size: Size,
    _store: &RenderStore,
  ) -> crate::CursorKind {
    if let Some(id) = self.fullscreen {
      return self.on_fullscreen_mouse(id, ev);
    }

    match *ev {
      MouseEvent::Move { pos } => {
        self.mouse_pos = Some(pos);

        if let Some(drag) = self.drag
          && let Some(i) = self.blocks.list().iter().position(|b| b.key == drag.key)
        {
          let top = self.geom.items[i].top - self.scroll.offset();
          let head = self.output_cell(i, Point::new(pos.x, pos.y - top));
          self.selection = (head != drag.anchor).then_some(Selection { head, ..drag });
          return crate::CursorKind::Beam;
        }

        match self.block_at(pos) {
          Some((i, rel)) if self.in_header(i, rel) => crate::CursorKind::Pointer,
          Some((i, _)) if self.geom.rows[i] > 0 => crate::CursorKind::Beam,
          _ => crate::CursorKind::Default,
        }
      }

      MouseEvent::Button { pos, pressed, button: MouseButton::Left } => {
        if !pressed {
          self.drag = None;
          return crate::CursorKind::Default;
        }

        self.selection = None;
        let Some((i, rel)) = self.block_at(pos) else { return crate::CursorKind::Default };
        let key = self.geom.items[i].key;
        if self.in_header(i, rel) {
          self.blocks.toggle(key);
          if self.mode == ShellMode::Blocks {
            self.blocks.set_focus(Some(key));
          }
          return crate::CursorKind::Pointer;
        }

        let collapsed = self.blocks.list()[i].collapsed;
        if !collapsed
          && self.geom.rows[i] > 0
          && matches!(self.blocks.list()[i].kind, BlockKind::Job(_))
        {
          let cell = self.output_cell(i, rel);
          self.drag = Some(Selection { key, anchor: cell, head: cell });
        }
        crate::CursorKind::Beam
      }

      MouseEvent::Scroll { delta, .. } => {
        self.scroll_by(-delta.y);
        crate::CursorKind::Default
      }

      MouseEvent::Leave => {
        self.mouse_pos = None;
        self.drag = None;
        crate::CursorKind::Default
      }

      _ => crate::CursorKind::Default,
    }
  }

  /// Mouse events for a job that fills the view, which go to the program if
  /// it asked for them. There is no selection or scrollback here.
  fn on_fullscreen_mouse(&mut self, id: JobId, ev: &MouseEvent) -> crate::CursorKind {
    let Some(modes) = self.shell.job(id).map(|j| j.emulator.mouse_modes()) else {
      return crate::CursorKind::Default;
    };
    let ch = self.geom.char_size;

    let bytes = match *ev {
      MouseEvent::Move { pos } => {
        self.mouse_pos = Some(pos);
        let (col, row) = mouse_cell(pos, ch);
        mouse::motion(modes, col, row, self.mouse_held)
      }
      MouseEvent::Button { pos, pressed, button } => {
        let code = match button {
          MouseButton::Left => 0,
          MouseButton::Middle => 1,
          MouseButton::Right => 2,
        };
        self.mouse_held = pressed.then_some(code);
        let (col, row) = mouse_cell(pos, ch);
        mouse::button(modes, col, row, code, pressed)
      }
      MouseEvent::Scroll { pos, delta } => {
        // The same step as the terminal view.
        let lines = (delta.y / 20.0) as i32;
        let (col, row) = mouse_cell(pos, ch);
        if lines == 0 { None } else { mouse::scroll(modes, col, row, lines) }
      }
      MouseEvent::Leave => {
        self.mouse_held = None;
        self.mouse_pos = None;
        None
      }
      _ => None,
    };
    if let Some(bytes) = bytes {
      self.shell.send_input(id, &bytes);
    }
    crate::CursorKind::Beam
  }

  fn copy(&self) {
    let Some(selection) = self.selection else { return };
    let Some(BlockKind::Job(id)) = self.blocks.get(selection.key).map(|b| &b.kind) else { return };
    let Some(job) = self.shell.job(*id) else { return };
    let (start, end) = selection.range();

    let state = job.emulator.state();
    let mut buf = String::new();
    for row in start.row..=end.row {
      let Some(line) = state.absolute_line(row) else { continue };
      let slice = if row == start.row && row == end.row {
        line.slice_to_string(start.col..end.col)
      } else if row == start.row {
        line.slice_to_string(start.col..)
      } else if row == end.row {
        line.slice_to_string(..end.col)
      } else {
        line.slice_to_string(..)
      };

      if !buf.is_empty() {
        buf.push('\n');
      }
      buf.push_str(slice.trim_end());
    }

    self.clipboard.copy(&buf);
  }

  // Layout.

  pub fn layout(&mut self, layout: &mut Layout) {
    puffin::profile_function!();

    if !self.set_waker {
      self.set_waker = true;
      let waker = layout.notifier();
      // The host throttles how often this is called.
      self.shell.set_wake(move || waker.wake());
      // The first prompt was captured without a wakeup for its git status.
      self.prompt = self.shell.prompt();
    }

    let events = self.shell.poll();
    for event in events {
      self.handle_event(event);
    }
    if self.should_close {
      layout.close_view();
      return;
    }

    self.auto_collapse = layout.store.config.borrow().settings.shell.auto_collapse_lines as usize;

    let size = layout.size();
    let char_size = character_size(layout.store);
    let ui_line = layout.store.text.font_metrics(Font::Ui).line_height;

    // The pty is as wide as the text area, and as tall as the view; or the
    // whole view, for a program on the alternate screen.
    let fullscreen =
      self.running().filter(|j| is_fullscreen(j.is_running(), &j.emulator)).map(|j| j.id);
    let grid = pty_grid(size, char_size, PAD_X, fullscreen.is_some());
    // The startup output is run at the size the view has at first.
    self.shell.start(grid);
    if grid != self.grid {
      self.grid = grid;
      if let Some(id) = self.running().map(|j| j.id) {
        self.shell.resize(id, grid);
      }
      if self.shell.startup_running() {
        self.shell.resize(STARTUP_ID, grid);
      }
    }

    self.geom.size = size;
    self.geom.char_size = char_size;
    self.geom.ui_line = ui_line;
    self.geom.cols = grid.cols;

    if fullscreen != self.fullscreen {
      // A job that left the alternate screen is resized by the running path
      // above. One that died in it is not running any more, so put its
      // emulator back to the normal width here.
      if let Some(prev) = self.fullscreen
        && self.shell.job(prev).is_some_and(|j| !j.is_running())
      {
        self.shell.resize(prev, grid);
      }
      self.fullscreen = fullscreen;
      self.mouse_held = None;
      self.drag = None;
      self.selection = None;
      // Back in the block layout, the prompt is where the user wants to be.
      self.scroll.follow_bottom();
    }
    if fullscreen.is_some() {
      if self.shell.needs_poll() {
        layout.notifier().wake();
      }
      return;
    }

    let mut rows = vec![];
    let mut header_rows = vec![];
    let mut heights = vec![];
    for block in self.blocks.list() {
      let (output_rows, header, height) = match &block.kind {
        BlockKind::Job(id) => match self.shell.job(*id) {
          Some(job) => {
            let header = header_rows_for(&job.line, grid.cols);
            let rows = output_rows(job);
            // A collapsed block keeps one row for its `+N lines` hint.
            let shown = if block.collapsed { rows.min(1) } else { rows };
            (rows, header, header + shown)
          }
          None => (0, 0, 0),
        },
        BlockKind::Notice(lines) => (0, 0, lines.len()),
        BlockKind::Startup(id) => {
          let rows = self.shell.job(*id).map_or(0, output_rows);
          (rows, 0, rows)
        }
      };
      rows.push(output_rows);
      header_rows.push(header);
      heights.push((block.key, height as f64 * char_size.height));
    }
    let prompt_height = if self.running().is_some() || self.shell.startup_running() {
      0.0
    } else {
      header_rows_for(self.input.text(), grid.cols) as f64 * char_size.height
    };
    heights.push((PROMPT_KEY, prompt_height));

    let (items, total) = stack(heights);
    self.geom.items = items;
    self.geom.total = total;
    self.geom.rows = rows;
    self.geom.header_rows = header_rows;

    self.scroll.update(&self.geom.items, total, size.height);
    if let Some(key) = self.reveal.take() {
      self.scroll.reveal(key, &self.geom.items, total, size.height);
    }

    // There is no wakeup for output left in the pty to keep this frame short.
    if self.shell.needs_poll() {
      layout.notifier().wake();
    }
  }

  // Drawing.

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    let size = render.size();
    render.fill(&Rect::from_origin_size(Point::ZERO, size), render.theme().background);

    if let Some(id) = self.fullscreen
      && let Some(job) = self.shell.job(id)
    {
      // The program has the whole view, like in the terminal view.
      let state = job.emulator.state();
      let mut options =
        DrawOptions::screen(state, Rect::from_origin_size(Point::ZERO, size), self.grid.rows);
      options.cursor = state
        .cursor
        .visible
        .then(|| CursorDraw { pos: state.cursor.pos, focused: self.focused });
      self.painters.entry(id).or_default().draw(render, state, &options);
      return;
    }

    let offset = self.scroll.offset();
    let hover = self.mouse_pos.and_then(|p| self.block_at(p));
    let mut drawn = vec![];
    let mut tooltip = None;

    for (i, block) in self.blocks.list().iter().enumerate() {
      let item = self.geom.items[i];
      let y = item.top - offset;
      if y + item.height < 0.0 || y > size.height {
        continue;
      }

      let focused = self.mode == ShellMode::Blocks && self.blocks.focus() == Some(block.key);
      if focused {
        // A faint band behind the header (or the whole notice).
        let height = match block.kind {
          BlockKind::Job(_) => self.header_height(i),
          BlockKind::Notice(_) => item.height,
          BlockKind::Startup(_) => 0.0,
        };
        let tint = render.theme().mode_normal.multiply_alpha(FOCUS_ALPHA);
        render.fill(&Rect::new(0.0, y, size.width, y + height), tint);
      }

      match &block.kind {
        BlockKind::Notice(lines) => self.draw_notice(render, y, lines),
        BlockKind::Startup(id) => {
          let rows = self.geom.rows[i];
          if rows > 0
            && let Some(job) = self.shell.job(*id)
          {
            drawn.push(*id);
            let painter = self.painters.entry(*id).or_default();
            draw_output(render, painter, job, y, rows, None, self.focused, self.geom.char_size);
          }
        }
        BlockKind::Job(id) => {
          let Some(job) = self.shell.job(*id) else { continue };
          let over = hover.is_some_and(|(j, _)| j == i);
          let over_header = hover.is_some_and(|(j, rel)| j == i && self.in_header(i, rel));

          let chevron = if block.collapsed { Chevron::Collapsed } else { Chevron::Expanded };
          let hot = focused || over;
          self.draw_prompt(
            render,
            y,
            &job.context,
            &job.line,
            job_result(job),
            (chevron, hot),
            None,
          );
          if over_header {
            tooltip = Some(job);
          }

          let rows = self.geom.rows[i];
          let top = y + self.header_height(i);
          if block.collapsed {
            if rows > 0 {
              let hint = if rows == 1 { "+1 line".to_string() } else { format!("+{rows} lines") };
              let layout = render.layout_text(Font::Terminal, &hint, render.theme().text_dim);
              render.draw_text(&layout, (0.0, top));
            }
          } else if rows > 0 {
            drawn.push(*id);
            let selection = self.selection.filter(|s| s.key == block.key).map(|s| s.range());
            let painter = self.painters.entry(*id).or_default();
            draw_output(
              render,
              painter,
              job,
              top,
              rows,
              selection,
              self.focused,
              self.geom.char_size,
            );
          }
        }
      }
    }
    // Painters keep layouts for every line they drew, so forget the ones that
    // are off screen.
    self.painters.retain(|id, _| drawn.contains(id));

    let prompt = *self.geom.items.last().unwrap();
    if prompt.height > 0.0 {
      let focused = self.focused && self.mode == ShellMode::Input;
      self.draw_prompt(
        render,
        prompt.top - offset,
        &self.prompt,
        self.input.text(),
        BlockResult::Live,
        (Chevron::Hidden, false),
        Some((self.input.cursor(), focused)),
      );
    }

    if let (Some(job), Some(pos)) = (tooltip, self.mouse_pos) {
      self.draw_tooltip(render, job, pos);
    }
  }

  fn draw_notice(&self, render: &mut Render, y: f64, lines: &[String]) {
    let dim = render.theme().text_dim;
    for (i, line) in lines.iter().enumerate() {
      let layout = render.layout_text(Font::Terminal, line, dim);
      render.draw_text(&layout, (0.0, y + i as f64 * self.geom.char_size.height));
    }
  }

  /// Draws a prompt (blank row, info line, symbol line) with `command` typed
  /// after it, starting at `y`. A `cursor` is a byte offset into `command`
  /// and whether it is focused.
  fn draw_prompt(
    &self,
    render: &mut Render,
    y: f64,
    ctx: &PromptContext,
    command: &str,
    result: BlockResult,
    (chevron, hot): (Chevron, bool),
    cursor: Option<(usize, bool)>,
  ) {
    let ch = self.geom.char_size;
    let x = 0.0;

    let info_y = y + ch.height;
    let width = render.size().width;
    let elements = prompt_elements(ctx, &result, chevron, self.geom.cols);
    // `clipped` moves the origin to the top left of the clip, so the row is
    // drawn at y = 0 inside it.
    render.clipped(Rect::new(0.0, info_y, width, info_y + ch.height), |render| {
      for e in &elements {
        self.draw_element(render, e, 0.0, hot);
      }
    });

    let sym = symbol(ctx);
    let wrapped = Wrapped::new(SYMBOL_COLS, command, self.geom.cols);
    for (i, text) in wrapped.rows.iter().enumerate() {
      let mut line = vec![];
      if i == 0 {
        line.push(Span { text: sym, color: symbol_color(&result).color(), bold: true });
        line.push(Span { text: " ", color: None, bold: false });
      }
      line.push(Span { text, color: None, bold: false });
      let layout = styled_line(render, &line);
      render.draw_text(&layout, (x, y + (2 + i) as f64 * ch.height));
    }

    // The same cursor as the terminal: a filled cell with the character
    // inverted while focused, an outline otherwise.
    if let Some((byte, focused)) = cursor {
      let (row, col) = wrapped.cursor(byte);
      let cell = Rect::from_origin_size(
        (x + (col as f64 * ch.width).round(), y + ((2 + row) as f64 * ch.height).round()),
        (ch.width.ceil(), ch.height.ceil()),
      );
      if focused {
        render.fill(&cell.ceil(), render.theme().text);
        if let Some(c) = command[byte.min(command.len())..].chars().next()
          && !c.is_whitespace()
        {
          let layout =
            render.layout_text(Font::Terminal, &c.to_string(), render.theme().background);
          render.draw_text(&layout, (cell.x0, cell.y0));
        }
      } else {
        render.stroke(&cell.inset(-0.5 * render.scale()), render.theme().text, Stroke::new(1.0));
      }
    }
  }

  /// Draws one element of the info line, whose row starts at `y`. The text
  /// sits exactly on its cells; a pill's background reaches a little past
  /// them, into the gap to its neighbours.
  fn draw_element(&self, render: &mut Render, e: &Element, y: f64, hot: bool) {
    let ch = self.geom.char_size;
    let x = e.col_start as f64 * ch.width;
    let w = e.col_len as f64 * ch.width;
    let fg = e.color.color().unwrap_or(render.theme().text);

    if e.kind == Kind::Chevron {
      // Centered on the boundary between its two cells.
      let theme = render.theme();
      let color = if hot { theme.text } else { theme.text_dim.multiply_alpha(0.5) };
      let size = (ch.height * 0.6).round();
      let icon = match e.chevron {
        Chevron::Collapsed => &*icon::CHEVRON_RIGHT,
        _ => &*icon::CHEVRON_DOWN,
      };
      icon.stroke(
        Point::new((x + ch.width - size / 2.0).max(0.0), y + (ch.height - size) / 2.0),
        size,
        color,
        render,
      );
      return;
    }
    if e.kind == Kind::Spinner {
      self.spinner.draw_sized(
        render,
        Point::new(x + w / 2.0, y + ch.height / 2.0),
        (ch.height * 0.6).round(),
      );
      return;
    }
    if e.kind.is_pill() {
      let pad = PILL_PAD_X * ch.width;
      let rect = Rect::new(x - pad, y + PILL_INSET_Y, x + w + pad, y + ch.height - PILL_INSET_Y);
      render.fill(&rect.to_rounded_rect(rect.height() / 2.0), fg.multiply_alpha(PILL_ALPHA));
    }

    let bold = e.kind == Kind::Cwd;
    let mut spans = vec![];
    match e.tail {
      Some((chars, tail)) => {
        let at = e.text.char_indices().nth(chars).map_or(e.text.len(), |(i, _)| i);
        spans.push(Span { text: &e.text[..at], color: Some(fg), bold });
        spans.push(Span { text: &e.text[at..], color: tail.color(), bold });
      }
      None => spans.push(Span { text: &e.text, color: Some(fg), bold }),
    }
    let layout = styled_line(render, &spans);
    render.draw_text(&layout, (x, y));
  }

  /// Full details of a job, near the mouse.
  fn draw_tooltip(&self, render: &mut Render, job: &Job, pos: Point) {
    let size = render.size();
    let theme = render.theme();
    let (text, dim, bg, outline) =
      (theme.text, theme.text_dim, theme.background_lower, theme.background_raised_outline);

    let mut lines = vec![];
    lines.push(match job.state {
      JobState::Running => "still running".to_string(),
      JobState::Done { exit } => format!("exit code {exit}"),
    });
    lines.push(format!(
      "took {}",
      format_duration(job.elapsed()).unwrap_or_else(|| "under 1s".to_string())
    ));
    lines.push(format!("started {}", format_local_time(job.started_at)));
    if let Ok(since) = SystemTime::now().duration_since(job.context.time)
      && since > Duration::from_secs(60 * 60 * 24)
    {
      lines.push(format!("prompted {} days ago", since.as_secs() / 86400));
    }
    lines.push(job.context.cwd.display().to_string());
    match &job.context.git {
      Some(GitHead::Branch(branch)) => lines.push(format!("git branch {branch}")),
      Some(GitHead::Detached(sha)) => lines.push(format!("git commit {sha}")),
      None => {}
    }

    let layouts: Vec<_> = lines
      .iter()
      .enumerate()
      .map(|(i, l)| ui_text(render, l, if i == 0 { text } else { dim }))
      .collect();
    let line_height = self.geom.ui_line;
    let w = layouts.iter().map(|l| l.size().width).fold(0.0, f64::max) + 2.0 * PAD_X;
    let h = layouts.len() as f64 * line_height + 2.0 * TIP_PAD_Y;

    let x = (pos.x + 12.0).min(size.width - w).max(0.0);
    let y = if pos.y + 18.0 + h > size.height { (pos.y - h - 4.0).max(0.0) } else { pos.y + 18.0 };
    let rect = RoundedRect::new(x, y, x + w, y + h, 4.0);
    render.fill(&rect, bg);
    render.stroke(&rect, outline, Stroke::new(1.0));
    for (i, layout) in layouts.iter().enumerate() {
      render.draw_text(layout, (x + PAD_X, y + TIP_PAD_Y + i as f64 * line_height));
    }
  }
}

/// Rows of a prompt with `command` typed after it: the blank row, the info
/// line, and the rows of the command.
fn header_rows_for(command: &str, cols: usize) -> usize {
  2 + Wrapped::new(SYMBOL_COLS, command, cols).height()
}

/// Draws the visible part of a job's output, which starts at `top`.
#[allow(clippy::too_many_arguments)]
fn draw_output(
  render: &mut Render,
  painter: &mut TerminalPainter,
  job: &Job,
  top: f64,
  rows: usize,
  selection: Option<(Position, Position)>,
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
  let selection = selection.map(|(a, b)| {
    let shift = |p: Position| Position { row: p.row.saturating_sub(first), col: p.col };
    // Rows above the first drawn one aren't drawn, so only clip the start.
    if a.row < first { (Position { row: 0, col: 0 }, shift(b)) } else { (shift(a), shift(b)) }
  });

  let options = DrawOptions {
    rect: Rect::from_origin_size(
      (0.0, top + first as f64 * char_size.height),
      (render.size().width, (last - first) as f64 * char_size.height),
    ),
    first_row: first,
    rows: last - first,
    pixel_offset: 0.0,
    selection,
    cursor,
  };
  painter.draw(render, state, &options);
}

#[cfg(test)]
mod tests {
  use super::*;

  fn insert(c: char) -> Action { Action::Edit { count: None, e: Edit::Insert(c) } }
  fn arrow(dir: Direction) -> Action { Action::Move { count: None, m: Move::Single(dir) } }
  fn control(c: char) -> Action { Action::Control { char: c } }

  #[test]
  fn keys_for_a_running_job() {
    let normal = InputModes::default();
    let app = InputModes { application_cursor: true, ..normal };

    assert_eq!(encode_action(&insert('x'), normal), Some(b"x".to_vec()));
    assert_eq!(encode_action(&insert('\n'), normal), Some(b"\r".to_vec()));
    assert_eq!(encode_action(&control('c'), normal), Some(vec![3]));
    assert_eq!(encode_action(&control('d'), normal), Some(vec![4]));
    assert_eq!(encode_action(&control('['), normal), Some(vec![0x1b]));
    assert_eq!(encode_action(&arrow(Direction::Up), normal), Some(b"\x1b[A".to_vec()));
    assert_eq!(encode_action(&arrow(Direction::Left), app), Some(b"\x1bOD".to_vec()));
    assert_eq!(
      encode_action(&Action::Edit { count: None, e: Edit::Backspace }, normal),
      Some(vec![0x7f])
    );
    assert_eq!(
      encode_action(&Action::SetMode { mode: Mode::Normal, delta: -1 }, normal),
      Some(vec![0x1b])
    );
    assert_eq!(encode_action(&Action::Tab { shift: false }, normal), Some(vec![b'\t']));
    assert_eq!(encode_action(&Action::Tab { shift: true }, normal), Some(b"\x1b[Z".to_vec()));
    assert_eq!(encode_action(&control('\n'), normal), Some(b"\r".to_vec()));
    assert_eq!(encode_action(&Action::Yank, normal), None);
  }

  #[test]
  fn editing_the_line() {
    let mut line = InputLine::default();
    for c in "git status".chars() {
      assert_eq!(edit_line(&mut line, &insert(c)), LineCmd::Edited);
    }
    assert_eq!(edit_line(&mut line, &control('w')), LineCmd::Edited);
    assert_eq!(line.text(), "git ");
    assert_eq!(edit_line(&mut line, &control('a')), LineCmd::None);
    assert_eq!(line.cursor(), 0);
    assert_eq!(edit_line(&mut line, &control('e')), LineCmd::None);
    assert_eq!(edit_line(&mut line, &arrow(Direction::Up)), LineCmd::HistoryPrev);
    assert_eq!(edit_line(&mut line, &insert('\n')), LineCmd::Submit);
    assert_eq!(edit_line(&mut line, &control('l')), LineCmd::ClearScreen);
    assert_eq!(
      edit_line(&mut line, &Action::SetMode { mode: Mode::Normal, delta: -1 }),
      LineCmd::Blocks
    );
    assert_eq!(edit_line(&mut line, &control('c')), LineCmd::Edited);
    assert!(line.is_empty());
  }

  #[test]
  fn selections_are_ordered() {
    let a = Position { row: 3, col: 9 };
    let b = Position { row: 1, col: 2 };
    let (min, max) = ordered(a, b);
    assert_eq!((min.row, min.col, max.row, max.col), (1, 2, 3, 9));
    let (min, max) = ordered(Position { row: 2, col: 9 }, Position { row: 2, col: 4 });
    assert_eq!((min.col, max.col), (4, 9));
  }
}
