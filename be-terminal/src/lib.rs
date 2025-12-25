use anstyle_parse::{Parser, Utf8Parser};

use crate::{grid::Grid, pty::Pty};

mod control;
mod grid;
mod pty;

pub struct Terminal {
  pty:   Pty,
  state: TerminalState,

  size:   Size,
  parser: Parser,
}

struct TerminalState {
  grid:   Grid,
  cursor: Cursor,
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

impl Terminal {
  pub fn new(size: Size) -> Self {
    Terminal {
      pty: Pty::new(size),
      state: TerminalState::new(size),
      size,
      parser: Parser::<Utf8Parser>::new(),
    }
  }

  pub fn set_size(&mut self, size: Size) {
    if size != self.size {
      self.size = size;

      self.state.resize(size);
      self.pty.resize(size);
    }
  }

  pub fn perform_input(&mut self, c: char) { self.pty.input(c); }

  pub fn line(&self, index: usize) -> Option<&str> { self.state.grid.line(index) }

  pub fn update(&mut self) {
    loop {
      let mut buf = [0u8; 1024];

      match self.pty.read(&mut buf) {
        Ok(0) => break,
        Ok(n) => {
          for &b in &buf[..n] {
            self.parser.advance(&mut self.state, b);
          }
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
        Err(e) => println!("{}", e),
      }
    }
  }
}

impl TerminalState {
  fn new(size: Size) -> Self {
    TerminalState { grid: Grid::new(size), cursor: Cursor { row: 0, col: 0 } }
  }

  fn resize(&mut self, size: Size) {
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
    for line in terminal.state.grid.lines() {
      println!("{}", line);
    }
    println!("===");

    panic!();
  }
}
