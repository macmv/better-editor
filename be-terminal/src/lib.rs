use std::{
  ops::{Deref, DerefMut},
  os::fd::BorrowedFd,
  path::{Path, PathBuf},
};

use anstyle_parse::{Parser, Utf8Parser};
use polling::Events;

use crate::{
  grid::{Grid, Line, OwnedLine},
  pty::Pty,
};

mod control;
mod grid;
mod pty;

#[macro_use]
extern crate log;

#[macro_use]
extern crate be_macros;

pub struct Terminal {
  pty:   Pty,
  state: TerminalState,

  parser: Parser,
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

  scrollback:      Vec<OwnedLine>,
  viewport_offset: usize, // 0 is live, >0 is scrolled back.

  // VT100 "scroll region" concept. Used by htop.
  scroll_start: usize,
  scroll_end:   usize,

  // Alternate screen. Used by most fullscreen apps. Disables scrolling while active.
  alt_grid:         Grid,
  alt_screen:       bool,
  alt_cursor:       Cursor,
  alt_saved_cursor: Option<Cursor>,

  pub title: String,
  flags:     TerminalFlags,

  pending_writes: Vec<u8>,
  charsets:       [Charset; 4],
}

bitflags::bitflags! {
  struct TerminalFlags: u8 {
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

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Size {
  pub rows: usize,
  pub cols: usize,
}

pub struct Poller {
  poller: polling::Poller,
  fd:     BorrowedFd<'static>,
}

impl Terminal {
  pub fn new(size: Size, path: &Path) -> Self {
    Terminal {
      pty:    Pty::new(size, path),
      state:  TerminalState::new(size),
      parser: Parser::<Utf8Parser>::new(),
    }
  }

  pub fn state(&self) -> &TerminalState { &self.state }
  pub fn has_pending_writes(&self) -> bool { self.pty.has_pending_writes() }

  pub fn current_dir(&self) -> PathBuf { self.pty.current_dir() }

  /// # Safety
  ///
  /// The `Poller` must not outlive the `Terminal`.
  pub unsafe fn make_poller(&self) -> Poller {
    let poller = polling::Poller::new().unwrap();
    unsafe {
      poller.add(&self.pty.fd(), polling::Event::readable(0)).unwrap();
    }
    Poller { fd: unsafe { std::mem::transmute(self.pty.fd()) }, poller }
  }

  pub fn set_size(&mut self, size: Size) {
    if size != self.state.size {
      self.state.resize(size);
      self.pty.resize(size);
    }
  }

  pub fn perform_input(&mut self, c: char) { self.pty.write(format_args!("{c}")); }
  pub fn perform_backspace(&mut self) { self.pty.write_bytes(&[control::C0::BS]); }
  pub fn perform_delete(&mut self) { self.pty.write_bytes(&[control::C0::DEL]); }
  pub fn perform_control(&mut self, b: u8) { self.pty.write_bytes(&[b]); }
  pub fn perform_escape(&mut self) { self.pty.write_bytes(&[control::C0::ESC]); }
  pub fn perform_tab(&mut self) { self.pty.write_bytes(&[control::C0::HT]); }
  pub fn perform_shift_tab(&mut self) { self.pty.write_bytes(b"\x1b[Z"); }
  pub fn perform_up(&mut self) {
    if self.state.flags.contains(TerminalFlags::CURSOR_KEYS) {
      self.pty.write_bytes(b"\x1bOA");
    } else {
      self.pty.write_bytes(b"\x1b[A");
    }
  }
  pub fn perform_down(&mut self) {
    if self.state.flags.contains(TerminalFlags::CURSOR_KEYS) {
      self.pty.write_bytes(b"\x1bOB");
    } else {
      self.pty.write_bytes(b"\x1b[B");
    }
  }
  pub fn perform_left(&mut self) {
    if self.state.flags.contains(TerminalFlags::CURSOR_KEYS) {
      self.pty.write_bytes(b"\x1bOD");
    } else {
      self.pty.write_bytes(b"\x1b[D");
    }
  }
  pub fn perform_right(&mut self) {
    if self.state.flags.contains(TerminalFlags::CURSOR_KEYS) {
      self.pty.write_bytes(b"\x1bOC");
    } else {
      self.pty.write_bytes(b"\x1b[C");
    }
  }
  /// Report a mouse button press or release to the PTY.
  ///
  /// `button` is 0=left, 1=middle, 2=right. Only sent when mode 1000 is active.
  pub fn perform_mouse_button(&mut self, col: usize, row: usize, button: u8, pressed: bool) {
    if self.state.flags.contains(TerminalFlags::REPORT_MOUSE) {
      let code = if pressed { button } else { 3 };
      self.send_mouse_report(col, row, code);
    }
  }

  /// Report a scroll-wheel event to the PTY.
  ///
  /// Only sent when mode 1000 is active.
  pub fn perform_mouse_scroll(&mut self, col: usize, row: usize, dy: i32) {
    if self.state.flags.contains(TerminalFlags::REPORT_MOUSE) {
      // Scroll buttons: 64=wheel-up, 65=wheel-down (before the +32 offset in
      // send_mouse_report).
      let code = if dy > 0 { 64 } else { 65 };
      self.send_mouse_report(col, row, code);
    } else if !self.state.alt_screen {
      self.state.viewport_offset = self.state.viewport_offset.saturating_add_signed(dy as isize);

      self.state.viewport_offset = self.state.viewport_offset.clamp(0, self.state.scrollback.len());
    }
  }

  pub fn maximum_offset(&self) -> usize { self.state.scrollback.len() }
  pub fn viewport_offset(&self) -> usize { self.state.viewport_offset }

  pub fn is_fractional_scrolling(&self) -> bool {
    !self.state.flags.contains(TerminalFlags::REPORT_MOUSE) && !self.state.alt_screen
  }

  /// Report mouse motion to the PTY.
  ///
  /// `held` is the button currently held (0=left, 1=middle, 2=right), or None.
  /// Sent when mode 1002 (held button) or 1003 (all motion) is active.
  pub fn perform_mouse_move(&mut self, col: usize, row: usize, held: Option<u8>) {
    let report = if self.state.flags.contains(TerminalFlags::MOUSE_ALL_MOTION) {
      true
    } else if self.state.flags.contains(TerminalFlags::MOUSE_MOTION) {
      held.is_some()
    } else {
      false
    };
    if report {
      // Add the motion bit (0x20 / 32) to the button code.
      // When no button is held the code is 3 (release), giving 32|3 = 35.
      let code = held.map(|b| b | 32).unwrap_or(32 | 3);
      self.send_mouse_report(col, row, code);
    }
  }

  /// Report a focus-in or focus-out event to the PTY.
  ///
  /// Only sent when mode 1004 is active.
  pub fn perform_focus_event(&mut self, focus: bool) {
    if self.state.flags.contains(TerminalFlags::REPORT_FOCUS) {
      if focus {
        self.pty.write_bytes(b"\x1b[I");
      } else {
        self.pty.write_bytes(b"\x1b[O");
      }
    }
  }

  fn send_mouse_report(&mut self, col: usize, row: usize, button_code: u8) {
    if self.state.flags.contains(TerminalFlags::MOUSE_UTF8) {
      // Mode 1005: encode each of the three values as a UTF-8 codepoint.
      let mut buf = vec![0x1b, b'[', b'M'];
      push_utf8_coord(&mut buf, 32 + u32::from(button_code));
      push_utf8_coord(&mut buf, 32 + col as u32 + 1);
      push_utf8_coord(&mut buf, 32 + row as u32 + 1);
      self.pty.write_bytes(&buf);
    } else {
      // X10 encoding: coords are limited to < 224 (byte value 255-32+1).
      if col >= 224 || row >= 224 {
        return;
      }
      self.pty.write_bytes(&[
        0x1b,
        b'[',
        b'M',
        32 + button_code,
        (32 + col + 1) as u8,
        (32 + row + 1) as u8,
      ]);
    }
  }

  pub fn perform_paste(&mut self, s: &str) {
    // Source: alacritty
    if self.state.flags.contains(TerminalFlags::BRACKETED_PASTE) {
      self.pty.write_bytes(b"\x1b[200~");

      // Write filtered escape sequences.
      //
      // We remove `\x1b` to ensure it's impossible for the pasted text to write the
      // bracketed paste end escape `\x1b[201~` and `\x03` since some shells
      // incorrectly terminate bracketed paste when they receive it.
      let filtered = s.replace(['\x1b', '\x03'], "");
      self.pty.write_bytes(filtered.as_bytes());

      self.pty.write_bytes(b"\x1b[201~");
    } else {
      // In non-bracketed (ie: normal) mode, terminal applications cannot distinguish
      // pasted data from keystrokes.
      //
      // In theory, we should construct the keystrokes needed to produce the data we
      // are pasting... since that's neither practical nor sensible (and
      // probably an impossible task to solve in a general way), we'll just
      // replace line breaks (windows and unix style) with a single carriage
      // return (\r, which is what the Enter key produces).
      self.pty.write_bytes(s.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
    }
  }

  pub fn visible_line(&self, index: usize) -> Option<&Line> {
    let abs_line = self.state.scrollback.len() + index - self.state.viewport_offset;
    if abs_line < self.state.scrollback.len() {
      Some(&*self.state.scrollback[abs_line])
    } else {
      self.state.grid.line(abs_line - self.state.scrollback.len())
    }
  }

  /// Returns a line from the scrollback. The most recent line is index 0. Used
  /// to render partial lines.
  pub fn scrollback_line(&self, index: usize) -> Option<&Line> {
    if index < self.state.scrollback.len() {
      Some(&*self.state.scrollback[self.state.scrollback.len() - index - 1])
    } else {
      None
    }
  }

  pub fn update(&mut self) -> bool {
    loop {
      let mut buf = [0u8; 1024];

      match self.pty.read(&mut buf) {
        // Ok(0) -> pty closed on macos
        Ok(0) => return true,
        // EIO error -> pty closed on linux
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => return true,

        Ok(n) => {
          for &b in &buf[..n] {
            self.parser.advance(&mut self.state, b);

            if !self.state.pending_writes.is_empty() {
              self.pty.write_bytes(&self.state.pending_writes);
              self.state.pending_writes.clear();
            }
          }
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,

        Err(e) => {
          println!("{}", e);
          break;
        }
      }
    }

    self.pty.flush();

    false
  }
}

impl Poller {
  pub fn poll(&self) {
    self.poller.wait(&mut Events::new(), None).unwrap();
    self.poller.modify(self.fd, polling::Event::readable(0)).unwrap();
  }
}

impl Drop for Poller {
  fn drop(&mut self) { self.poller.delete(self.fd).unwrap(); }
}

fn push_utf8_coord(buf: &mut Vec<u8>, codepoint: u32) {
  if let Some(c) = char::from_u32(codepoint) {
    let mut tmp = [0u8; 4];
    buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
  }
}

impl TerminalState {
  fn new(size: Size) -> Self {
    TerminalState {
      grid: Grid::new(size),
      cursor: Cursor::default(),
      saved_cursor: None,
      scrollback: vec![],
      viewport_offset: 0,
      scroll_start: 0,
      scroll_end: size.rows,
      size,

      alt_grid: Grid::new(size),
      alt_screen: false,
      alt_cursor: Cursor::default(),
      alt_saved_cursor: None,

      title: String::new(),
      flags: TerminalFlags::empty(),

      pending_writes: vec![],
      charsets: [Charset::Ascii; 4],
    }
  }

  fn resize(&mut self, size: Size) {
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
