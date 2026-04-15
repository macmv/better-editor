use std::{
  ops::{Deref, DerefMut},
  os::fd::BorrowedFd,
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
  pub style:          Style,
  pub active_charset: usize,
}

pub struct TerminalState {
  grid:       Grid,
  pub cursor: Cursor,

  scrollback:   Vec<OwnedLine>,
  size:         Size,
  scroll_start: usize,
  scroll_end:   usize,

  alt_grid:   Grid,
  alt_screen: bool,
  alt_cursor: Cursor,

  pub title:                   String,
  pub report_mouse:            bool,
  pub mouse_motion:            bool,
  pub mouse_all_motion:        bool,
  pub report_focus:            bool,
  pub mouse_utf8:              bool,
  pub cursor_keys:             bool,
  pub bracketed_paste:         bool,
  pub keypad_application_mode: bool,

  pending_writes: Vec<u8>,
  charsets:       [Charset; 4],
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
  pub fn new(size: Size) -> Self {
    Terminal {
      pty:    Pty::new(size),
      state:  TerminalState::new(size),
      parser: Parser::<Utf8Parser>::new(),
    }
  }

  pub fn state(&self) -> &TerminalState { &self.state }

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

  pub fn perform_input(&mut self, c: char) { self.pty.input(c); }
  pub fn perform_backspace(&mut self) { self.pty.input(control::C0::BS.into()); }
  pub fn perform_delete(&mut self) { self.pty.input(control::C0::DEL.into()); }
  pub fn perform_control(&mut self, b: u8) { self.pty.input_bytes(&[b]); }
  pub fn perform_escape(&mut self) { self.pty.input(control::C0::ESC.into()); }
  pub fn perform_tab(&mut self) { self.pty.input(control::C0::HT.into()); }
  pub fn perform_up(&mut self) {
    if self.state.cursor_keys {
      self.pty.input_str("\x1bOA");
    } else {
      self.pty.input_str("\x1b[A");
    }
  }
  pub fn perform_down(&mut self) {
    if self.state.cursor_keys {
      self.pty.input_str("\x1bOB");
    } else {
      self.pty.input_str("\x1b[B");
    }
  }
  pub fn perform_left(&mut self) {
    if self.state.cursor_keys {
      self.pty.input_str("\x1bOD");
    } else {
      self.pty.input_str("\x1b[D");
    }
  }
  pub fn perform_right(&mut self) {
    if self.state.cursor_keys {
      self.pty.input_str("\x1bOC");
    } else {
      self.pty.input_str("\x1b[C");
    }
  }
  /// Report a mouse button press or release to the PTY.
  ///
  /// `button` is 0=left, 1=middle, 2=right. Only sent when mode 1000 is active.
  pub fn perform_mouse_button(&mut self, col: usize, row: usize, button: u8, pressed: bool) {
    if self.state.report_mouse {
      let code = if pressed { button } else { 3 };
      self.send_mouse_report(col, row, code);
    }
  }

  /// Report a scroll-wheel event to the PTY.
  ///
  /// Only sent when mode 1000 is active.
  pub fn perform_mouse_scroll(&mut self, col: usize, row: usize, up: bool) {
    if self.state.report_mouse {
      // Scroll buttons: 64=wheel-up, 65=wheel-down (before the +32 offset in
      // send_mouse_report).
      let code = if up { 64 } else { 65 };
      self.send_mouse_report(col, row, code);
    }
  }

  /// Report mouse motion to the PTY.
  ///
  /// `held` is the button currently held (0=left, 1=middle, 2=right), or None.
  /// Sent when mode 1002 (held button) or 1003 (all motion) is active.
  pub fn perform_mouse_move(&mut self, col: usize, row: usize, held: Option<u8>) {
    let report = if self.state.mouse_all_motion {
      true
    } else if self.state.mouse_motion {
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
    if self.state.report_focus {
      if focus {
        self.pty.input_str("\x1b[I");
      } else {
        self.pty.input_str("\x1b[O");
      }
    }
  }

  fn send_mouse_report(&mut self, col: usize, row: usize, button_code: u8) {
    if self.state.mouse_utf8 {
      // Mode 1005: encode each of the three values as a UTF-8 codepoint.
      let mut buf = vec![0x1b, b'[', b'M'];
      push_utf8_coord(&mut buf, 32 + u32::from(button_code));
      push_utf8_coord(&mut buf, 32 + col as u32 + 1);
      push_utf8_coord(&mut buf, 32 + row as u32 + 1);
      self.pty.input_bytes(&buf);
    } else {
      // X10 encoding: coords are limited to < 224 (byte value 255-32+1).
      if col >= 224 || row >= 224 {
        return;
      }
      self.pty.input_bytes(&[
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
    if self.state.bracketed_paste {
      self.pty.input_str("\x1b[200~");

      // Write filtered escape sequences.
      //
      // We remove `\x1b` to ensure it's impossible for the pasted text to write the
      // bracketed paste end escape `\x1b[201~` and `\x03` since some shells
      // incorrectly terminate bracketed paste when they receive it.
      let filtered = s.replace(['\x1b', '\x03'], "");
      self.pty.input_str(&filtered);

      self.pty.input_str("\x1b[201~");
    } else {
      // In non-bracketed (ie: normal) mode, terminal applications cannot distinguish
      // pasted data from keystrokes.
      //
      // In theory, we should construct the keystrokes needed to produce the data we
      // are pasting... since that's neither practical nor sensible (and
      // probably an impossible task to solve in a general way), we'll just
      // replace line breaks (windows and unix style) with a single carriage
      // return (\r, which is what the Enter key produces).
      self.pty.input_str(&s.replace("\r\n", "\r").replace('\n', "\r"));
    }
  }

  pub fn line(&self, index: usize) -> Option<Line<'_>> { self.state.grid.line(index) }

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
              self.pty.input_bytes(&self.state.pending_writes);
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
      scrollback: vec![],
      scroll_start: 0,
      scroll_end: size.rows,
      size,

      alt_grid: Grid::new(size),
      alt_screen: false,
      alt_cursor: Cursor::default(),

      title: String::new(),
      report_mouse: false,
      mouse_motion: false,
      mouse_all_motion: false,
      report_focus: false,
      mouse_utf8: false,
      cursor_keys: false,
      bracketed_paste: false,
      keypad_application_mode: false,

      pending_writes: vec![],
      charsets: [Charset::Ascii; 4],
    }
  }

  fn resize(&mut self, size: Size) {
    if self.scroll_end == self.size.rows {
      self.scroll_end = size.rows;
    }
    self.size = size;
    self.grid.resize(size);
    self.alt_grid.resize(size);
    self.cursor.row = self.cursor.row.clamp(0, size.rows - 1);
    self.cursor.col = self.cursor.col.clamp(0, size.cols - 1);
  }
}

impl Default for Cursor {
  fn default() -> Self {
    Cursor {
      pos:       Position::default(),
      visible:   true,
      insert:    false,
      line_feed: false,
      blink:     false,
      style:     Default::default(),

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
