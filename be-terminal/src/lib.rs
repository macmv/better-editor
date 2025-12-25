use std::os::fd::BorrowedFd;

use anstyle_parse::{Parser, Utf8Parser};
use polling::Events;

use crate::{
  grid::{Grid, Line, OwnedLine},
  pty::Pty,
};

mod control;
mod grid;
mod pty;

pub struct Terminal {
  pty:   Pty,
  state: TerminalState,

  parser: Parser,
}

pub struct TerminalState {
  grid:       Grid,
  pub cursor: Cursor,

  scrollback: Vec<OwnedLine>,
  size:       Size,
  style:      Style,

  pub cursor_visible: bool,

  pending_writes: Vec<u8>,
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

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Cursor {
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
  pub fn perform_up(&mut self) { self.pty.input_str("\x1b[A"); }
  pub fn perform_down(&mut self) { self.pty.input_str("\x1b[B"); }
  pub fn perform_left(&mut self) { self.pty.input_str("\x1b[D"); }
  pub fn perform_right(&mut self) { self.pty.input_str("\x1b[C"); }

  pub fn line(&self, index: usize) -> Option<Line<'_>> { self.state.grid.line(index) }

  pub fn update(&mut self) {
    loop {
      let mut buf = [0u8; 1024];

      match self.pty.read(&mut buf) {
        Ok(0) => break,
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
        Err(e) => println!("{}", e),
      }
    }
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

impl TerminalState {
  fn new(size: Size) -> Self {
    TerminalState {
      grid: Grid::new(size),
      cursor: Cursor { row: 0, col: 0 },
      scrollback: vec![],
      size,
      style: Style::default(),
      cursor_visible: true,
      pending_writes: vec![],
    }
  }

  fn resize(&mut self, size: Size) {
    self.size = size;
    self.grid.resize(size);
    self.cursor.row = self.cursor.row.clamp(0, size.rows - 1);
    self.cursor.col = self.cursor.col.clamp(0, size.cols - 1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn terminal_works() {
    let mut terminal = Terminal::new(Size { rows: 40, cols: 80 });

    std::thread::sleep(std::time::Duration::from_millis(100));

    terminal.update();
    println!("===");
    /*
    for line in terminal.state.grid.lines() {
      println!("{}", line);
    }
    */
    println!("===");

    panic!();
  }
}
