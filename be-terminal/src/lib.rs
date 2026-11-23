use std::{
  collections::VecDeque,
  io,
  ops::{Deref, DerefMut},
  path::{Path, PathBuf},
  sync::Arc,
  time::{Duration, Instant},
};

use anstyle_parse::Parser;

pub use crate::{
  emulator::{Emulator, MouseMode},
  grid::{Line, OwnedLine},
  keys::{Arrow, InputModes},
  mouse::MouseModes,
  pty::{Pty, SpawnSpec},
  screen::{Lines, Screen},
};
use crate::{
  grid::Grid,
  session::{Core, Shared},
};

mod control;
mod emulator;
mod grid;
pub mod keys;
pub mod mouse;
mod pty;
mod screen;
mod session;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[macro_use]
extern crate expect_test;

#[macro_use]
extern crate log;

#[macro_use]
extern crate be_macros;

/// The most lines kept in scrollback. Older lines are dropped.
const SCROLLBACK_LIMIT: usize = 10_000;

/// How many bytes to read from the pty at once.
const READ_SIZE: usize = 64 * 1024;

/// The longest `Terminal::update` spends parsing before it gives the caller a
/// turn. Output that is still queued is picked up by the next call (see
/// `has_pending_output`), so that a program that prints a lot can't freeze the
/// ui.
const UPDATE_BUDGET: Duration = Duration::from_millis(3);

/// How long output is held back after a program starts a synchronized update
/// (mode 2026) without finishing it. Programs that crash mid-update shouldn't
/// freeze the screen.
pub(crate) const SYNC_TIMEOUT: Duration = Duration::from_millis(150);

/// The most output held back for a synchronized update, before it is shown
/// anyway.
pub(crate) const SYNC_MAX_HELD: usize = 4 * 1024 * 1024;

/// The sequence that ends a synchronized update.
pub(crate) const SYNC_END: &[u8] = b"\x1b[?2026l";

/// A source of terminal bytes, and a sink for input. This is the process (or
/// remote process) on the other side of the terminal.
pub trait PtyHost {
  /// Reads available output without blocking. Returns `WouldBlock` when there
  /// is nothing to read. A closed pty is reported as `Ok(0)` or an `EIO` error.
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

  /// Queues bytes to be sent to the pty. They are sent on `flush`.
  fn write_bytes(&mut self, bytes: &[u8]);
  fn has_pending_writes(&self) -> bool;
  /// Sends as many queued bytes as possible, without blocking.
  fn flush(&mut self);

  fn resize(&mut self, size: Size);
  fn current_dir(&self) -> PathBuf;

  /// Sets a function to call (from any thread) when output may be available.
  /// Implementations should not call it more often than needed, and must stop
  /// calling it when dropped.
  fn set_wake(&mut self, wake: Box<dyn Fn() + Send>);

  /// The exit status of the process, if it has exited. Does not block. Signals
  /// are reported as `128 + signal`. Hosts without a process report `None`.
  fn exit_status(&mut self) -> Option<i32> { None }

  /// Sends a signal to the process (group). Does nothing by default.
  fn signal(&mut self, _sig: i32) {}
}

pub struct Terminal {
  shared: Arc<Shared>,
  // The emulator, and the way its screen gets published. See `session`.
  core:   Core,
  host:   Box<dyn PtyHost>,

  read_buf:       Box<[u8]>,
  // Set when `update` stopped reading before the host ran dry.
  pending_output: bool,

  // The colors last given to `set_default_colors`, so that they are only passed on when they
  // change.
  colors: Option<([u8; 3], [u8; 3])>,
}

#[derive(Clone, Copy)]
pub struct Cursor {
  pub pos:            Position,
  pub visible:        bool,
  pub insert:         bool,
  pub line_feed:      bool,
  pub blink:          bool,
  pub auto_wrap:      bool,
  pub pending_wrap:   bool,
  pub style:          Style,
  pub active_charset: usize,
}

pub struct TerminalState {
  grid:         Grid,
  pub cursor:   Cursor,
  saved_cursor: Option<Cursor>,
  size:         Size,

  scrollback:      VecDeque<OwnedLine>,
  viewport_offset: usize, // 0 is live, >0 is scrolled back.

  // A line to reuse the allocation of the next time the screen scrolls.
  spare: Option<OwnedLine>,
  // Bumped before each batch of changes, so that renderers can tell which lines
  // changed. See `OwnedLine::stamp`.
  epoch: u64,

  // VT100 "scroll region" concept. Used by htop.
  scroll_start: usize,
  scroll_end:   usize,

  // Alternate screen. Used by most fullscreen apps. Disables scrolling while
  // active.
  alt_grid:         Grid,
  alt_screen:       bool,
  alt_cursor:       Cursor,
  alt_saved_cursor: Option<Cursor>,

  pub title: String,
  flags:     TerminalFlags,

  // Kitty keyboard protocol: the active flags, and the flags saved by `CSI >
  // flags u` pushes.
  kitty_flags: u8,
  kitty_stack: Vec<u8>,

  pending_writes:     Vec<u8>,
  default_foreground: [u8; 3],
  default_background: [u8; 3],
  charsets:           [Charset; 4],

  // Whether the parser is known to be between sequences, so printable ASCII can
  // skip it. This is conservative: it is false whenever we can't be sure. The
  // parser doesn't expose its state, so `Perform` callbacks that always leave
  // it in the ground state set this, and `advance` clears it for every byte
  // that goes through the parser.
  ground:        bool,
  ground_before: bool,

  // Mode 2026: the program is drawing a frame, and wants it shown all at once.
  synchronized: bool,
  // Set when that mode was just turned on, so `Terminal` can stop parsing and
  // hold the frame.
  sync_started: bool,
}

bitflags::bitflags! {
  pub(crate) struct TerminalFlags: u8 {
    const REPORT_MOUSE            = 1 << 0;
    const MOUSE_MOTION            = 1 << 1;
    const MOUSE_ALL_MOTION        = 1 << 2;
    const REPORT_FOCUS            = 1 << 3;
    const MOUSE_UTF8              = 1 << 4;
    const CURSOR_KEYS             = 1 << 5;
    const BRACKETED_PASTE         = 1 << 6;
    const KEYPAD_APPLICATION_MODE = 1 << 7;
  }
}

/// Kitty keyboard protocol flag: report keys like Ctrl+Enter with `CSI u`
/// escape codes.
pub(crate) const KITTY_DISAMBIGUATE: u8 = 1;
/// Kitty keyboard protocol flag: report key release and repeat events. We only
/// ever see presses, so this changes nothing.
#[allow(dead_code)]
pub(crate) const KITTY_REPORT_EVENTS: u8 = 2;
/// Kitty keyboard protocol flag: report the shifted key as an alternate.
pub(crate) const KITTY_ALTERNATE_KEYS: u8 = 4;
/// Kitty keyboard protocol flag: report every key as `CSI u`, even plain text.
pub(crate) const KITTY_ALL_AS_ESCAPES: u8 = 8;
/// Kitty keyboard protocol flag: include the typed text in `CSI u` reports.
pub(crate) const KITTY_ASSOCIATED_TEXT: u8 = 16;
/// The most keyboard mode pushes we remember, so a misbehaving program can't
/// grow the stack forever.
const KITTY_STACK_LIMIT: usize = 16;

#[derive(Clone, Copy)]
enum Charset {
  Ascii,
  LineDrawing,
}

impl Charset {
  pub fn map(self, c: char) -> char {
    match self {
      Charset::Ascii => c,
      Charset::LineDrawing => match c {
        '_' => ' ',
        '`' => '◆',
        'a' => '▒',
        'b' => '\u{2409}', // Symbol for horizontal tabulation
        'c' => '\u{240c}', // Symbol for form feed
        'd' => '\u{240d}', // Symbol for carriage return
        'e' => '\u{240a}', // Symbol for line feed
        'f' => '°',
        'g' => '±',
        'h' => '\u{2424}', // Symbol for newline
        'i' => '\u{240b}', // Symbol for vertical tabulation
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
      },
    }
  }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
  pub flags:      StyleFlags,
  pub foreground: Option<TerminalColor>,
  pub background: Option<TerminalColor>,
}

bitflags::bitflags! {
  #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
  pub struct StyleFlags: u8 {
    const BOLD          = 1 << 0;
    const DIM           = 1 << 1;
    const ITALIC        = 1 << 2;
    const UNDERLINE     = 1 << 3;
    const BLINK         = 1 << 4;
    const INVERSE       = 1 << 5;
    const HIDDEN        = 1 << 6;
    const STRIKETHROUGH = 1 << 7;
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalColor {
  Builtin { color: BuiltinColor, bright: bool },
  Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinColor {
  Black,
  Red,
  Green,
  Yellow,
  Blue,
  Magenta,
  Cyan,
  White,
}

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct Position {
  pub row: usize,
  pub col: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Size {
  pub rows: usize,
  pub cols: usize,
}

impl Terminal {
  /// Starts a shell on this machine, in `path`. Fails if the shell cannot be
  /// started, for example because `path` does not exist here.
  pub fn new_local(size: Size, path: &Path) -> io::Result<Self> {
    Ok(Terminal::with_host(size, Box::new(Pty::new(size, path)?)))
  }

  pub fn with_host(size: Size, host: Box<dyn PtyHost>) -> Self {
    let shared = Shared::new();
    let core = Core::new(size, shared.clone());

    Terminal {
      shared,
      core,
      host,
      read_buf: vec![0; READ_SIZE].into_boxed_slice(),
      pending_output: false,
      colors: None,
    }
  }

  #[cfg(test)]
  fn emu(&self) -> &Emulator { &self.core.emu }

  #[cfg(test)]
  fn emu_mut(&mut self) -> &mut Emulator { &mut self.core.emu }

  /// Applies what has been asked of the emulator (a new size, a scroll
  /// position), and publishes the result.
  fn sync(&mut self) {
    if let Some(size) = self.core.apply_control() {
      self.host.resize(size);
    }
    self.core.publish();
  }

  /// Copies what is on screen into `screen`. Call this once per frame, before
  /// drawing. Lines that haven't changed since the last snapshot keep their
  /// `stamp`, and lines that have are marked as changed.
  pub fn snapshot(&mut self, screen: &mut Screen) {
    self.sync();
    self.shared.snapshot(screen);
  }

  /// Sets the rendered default colors reported to applications through OSC
  /// 10/11.
  pub fn set_default_colors(&mut self, foreground: [u8; 3], background: [u8; 3]) {
    // This is called every frame, so only pass it on if it changed.
    if self.colors == Some((foreground, background)) {
      return;
    }
    self.colors = Some((foreground, background));

    self.shared.request_colors(foreground, background);
    self.sync();
  }

  pub fn has_pending_writes(&self) -> bool { self.host.has_pending_writes() }

  pub fn current_dir(&self) -> PathBuf { self.host.current_dir() }

  /// Sets a function to call when the terminal may have new output.
  pub fn set_wake(&mut self, wake: Box<dyn Fn() + Send>) { self.host.set_wake(wake); }

  pub fn set_size(&mut self, size: Size) {
    self.shared.request_resize(size);
    self.sync();
  }

  fn input_modes(&self) -> InputModes { self.shared.modes().input }
  fn mouse_modes(&self) -> MouseModes { self.shared.modes().mouse }

  fn write_keys(&mut self, bytes: Vec<u8>) { self.host.write_bytes(&bytes); }

  pub fn perform_input(&mut self, c: char) {
    self.write_keys(keys::input_with(c, self.input_modes()));
  }
  pub fn perform_backspace(&mut self) {
    self.write_keys(keys::encode(keys::Key::Backspace, 0, self.input_modes()));
  }
  pub fn perform_delete(&mut self) { self.write_keys(keys::delete()); }
  pub fn perform_control(&mut self, b: u8) {
    self.write_keys(keys::control_with(b, self.input_modes()));
  }
  /// Sends Ctrl+Enter. Plain terminals can't tell this apart from Enter, so it
  /// only gets a distinct encoding if the program asked for it with the kitty
  /// keyboard protocol.
  pub fn perform_ctrl_enter(&mut self) { self.write_keys(keys::ctrl_enter(self.input_modes())); }
  pub fn perform_escape(&mut self) {
    self.write_keys(keys::encode(keys::Key::Escape, 0, self.input_modes()));
  }
  pub fn perform_tab(&mut self) {
    self.write_keys(keys::encode(keys::Key::Tab, 0, self.input_modes()));
  }
  pub fn perform_shift_tab(&mut self) {
    self.write_keys(keys::encode(keys::Key::Tab, keys::mods::SHIFT, self.input_modes()));
  }
  pub fn perform_up(&mut self) { self.write_keys(keys::arrow(Arrow::Up, self.input_modes())); }
  pub fn perform_down(&mut self) { self.write_keys(keys::arrow(Arrow::Down, self.input_modes())); }
  pub fn perform_left(&mut self) { self.write_keys(keys::arrow(Arrow::Left, self.input_modes())); }
  pub fn perform_right(&mut self) {
    self.write_keys(keys::arrow(Arrow::Right, self.input_modes()));
  }
  /// Report a mouse button press or release to the PTY.
  ///
  /// `button` is 0=left, 1=middle, 2=right. Only sent when mode 1000 is active.
  pub fn perform_mouse_button(&mut self, col: usize, row: usize, button: u8, pressed: bool) {
    if let Some(bytes) = mouse::button(self.mouse_modes(), col, row, button, pressed) {
      self.write_keys(bytes);
    }
  }

  /// Report a scroll-wheel event to the PTY.
  ///
  /// Only sent when mode 1000 is active.
  pub fn perform_mouse_scroll(&mut self, col: usize, row: usize, dy: i32) {
    let modes = self.shared.modes();
    if modes.mouse.reporting {
      if let Some(bytes) = mouse::scroll(modes.mouse, col, row, dy) {
        self.write_keys(bytes);
      }
    } else if !modes.alt_screen {
      self.shared.scroll(dy);
      self.sync();
    }
  }

  pub fn perform_paste(&mut self, s: &str) { self.write_keys(keys::paste(s, self.input_modes())); }

  pub fn maximum_offset(&self) -> usize { self.shared.scrollback_len() }
  pub fn viewport_offset(&self) -> usize { self.shared.viewport_offset() }

  pub fn is_fractional_scrolling(&self) -> bool {
    let modes = self.shared.modes();
    !modes.mouse.reporting && !modes.alt_screen
  }

  /// Report mouse motion to the PTY.
  ///
  /// `held` is the button currently held (0=left, 1=middle, 2=right), or None.
  /// Sent when mode 1002 (held button) or 1003 (all motion) is active.
  pub fn perform_mouse_move(&mut self, col: usize, row: usize, held: Option<u8>) {
    if let Some(bytes) = mouse::motion(self.mouse_modes(), col, row, held) {
      self.write_keys(bytes);
    }
  }

  /// Report a focus-in or focus-out event to the PTY.
  ///
  /// Only sent when mode 1004 is active.
  pub fn perform_focus_event(&mut self, focus: bool) {
    if let Some(bytes) = mouse::focus(self.mouse_modes(), focus) {
      self.write_keys(bytes);
    }
  }

  /// Whether the last `update` ran out of time before it read everything the
  /// host had, or is holding output back. The caller should call `update`
  /// again soon, as there will be no wakeup for that.
  pub fn has_pending_output(&self) -> bool { self.pending_output }

  /// Reads and parses output from the host, for at most `UPDATE_BUDGET`.
  /// Returns true once the pty has closed.
  pub fn update(&mut self) -> bool {
    self.pending_output = false;

    let start = Instant::now();
    loop {
      match self.host.read(&mut self.read_buf) {
        // Ok(0) -> pty closed on macos
        Ok(0) => {
          self.core.close();
          return true;
        }
        // EIO error -> pty closed on linux
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => {
          self.core.close();
          return true;
        }

        Ok(n) => {
          self.host.write_bytes(&self.core.feed(&self.read_buf[..n]));

          if start.elapsed() >= UPDATE_BUDGET {
            self.pending_output = true;
            break;
          }
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,

        Err(e) => {
          error!("failed to read from pty: {e}");
          break;
        }
      }
    }

    // Releases held output that is due.
    self.host.write_bytes(&self.core.tick());

    // Held output is released by a timeout, which nothing will wake us for.
    if self.core.deadline().is_some() {
      self.pending_output = true;
    }

    self.host.flush();
    self.core.publish();

    false
  }
}

/// Feeds `bytes` to the parser. Runs of printable ASCII in the ground state
/// skip the parser, and are written straight into the grid.
///
/// Stops early after a byte that starts a synchronized update (`sync_started`),
/// and returns how many bytes were used.
pub(crate) fn advance(parser: &mut Parser, state: &mut TerminalState, bytes: &[u8]) -> usize {
  let mut i = 0;
  while i < bytes.len() {
    if state.ground {
      let run =
        bytes[i..].iter().position(|b| !(0x20..0x7f).contains(b)).unwrap_or(bytes.len() - i);
      if run > 0 {
        state.print_ascii(&bytes[i..i + run]);
        i += run;
        continue;
      }
    }

    advance_byte(parser, state, bytes[i]);
    i += 1;

    if state.sync_started {
      break;
    }
  }

  i
}

fn advance_byte(parser: &mut Parser, state: &mut TerminalState, byte: u8) {
  state.ground_before = state.ground;
  state.ground = false;
  parser.advance(state, byte);
}

impl TerminalState {
  /// Returns the line at `index` of scrollback followed by the screen, or
  /// `None` if out of range. Index 0 is the oldest line of scrollback.
  pub fn absolute_line(&self, index: usize) -> Option<&OwnedLine> {
    if index < self.scrollback.len() {
      Some(&self.scrollback[index])
    } else {
      self.grid.line(index - self.scrollback.len())
    }
  }

  /// The number of lines of scrollback.
  pub fn scrollback_len(&self) -> usize { self.scrollback.len() }

  /// How far the viewport is scrolled back. 0 is live.
  pub fn viewport_offset(&self) -> usize { self.viewport_offset }

  pub(crate) fn new(size: Size) -> Self {
    TerminalState {
      grid: Grid::new(size),
      cursor: Cursor::default(),
      saved_cursor: None,
      scrollback: VecDeque::new(),
      viewport_offset: 0,
      spare: None,
      epoch: 0,
      scroll_start: 0,
      scroll_end: size.rows,
      size,

      alt_grid: Grid::new(size),
      alt_screen: false,
      alt_cursor: Cursor::default(),
      alt_saved_cursor: None,

      title: String::new(),
      flags: TerminalFlags::empty(),

      kitty_flags: 0,
      kitty_stack: vec![],

      pending_writes: vec![],
      default_foreground: [255; 3],
      default_background: [0; 3],
      charsets: [Charset::Ascii; 4],

      ground: true,
      ground_before: true,

      synchronized: false,
      sync_started: false,
    }
  }

  pub(crate) fn has_replies(&self) -> bool { !self.pending_writes.is_empty() }

  /// Starts a new batch of changes. Lines written after this get a new stamp.
  pub(crate) fn begin_frame(&mut self) {
    self.epoch += 1;
    self.grid.set_epoch(self.epoch);
    self.alt_grid.set_epoch(self.epoch);
  }

  pub(crate) fn resize(&mut self, size: Size) {
    if self.scroll_end == self.size.rows {
      self.scroll_end = size.rows;
    }
    self.size = size;
    // TODO: Evict lines to `scrollback` when shrinking.
    self.grid.resize(size);
    self.alt_grid.resize(size);
    self.cursor.row = self.cursor.row.clamp(0, size.rows - 1);
    self.cursor.col = self.cursor.col.clamp(0, size.cols - 1);
  }
}

impl Default for Cursor {
  fn default() -> Self {
    Cursor {
      pos:          Position::default(),
      visible:      true,
      insert:       false,
      line_feed:    false,
      blink:        false,
      auto_wrap:    true,
      pending_wrap: false,
      style:        Default::default(),

      active_charset: 0,
    }
  }
}

impl Deref for Cursor {
  type Target = Position;
  fn deref(&self) -> &Self::Target { &self.pos }
}
impl DerefMut for Cursor {
  fn deref_mut(&mut self) -> &mut Self::Target { &mut self.pos }
}
