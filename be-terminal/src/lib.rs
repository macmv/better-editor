use anstyle_parse::{Parser, Perform, Utf8Parser};

use crate::{grid::Grid, pty::Pty};

mod grid;
mod pty;

pub struct Terminal {
  pty:   Pty,
  state: TerminalState,

  parser: Parser,
}

struct TerminalState {
  grid:   Grid,
  cursor: Cursor,
}

#[derive(Copy, Clone)]
struct Cursor {
  row: usize,
  col: usize,
}

impl Terminal {
  pub fn new() -> Self {
    Terminal {
      pty:    Pty::new(),
      state:  TerminalState::new(),
      parser: Parser::<Utf8Parser>::new(),
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
  fn new() -> Self { TerminalState { grid: Grid::new(), cursor: Cursor { row: 0, col: 0 } } }
}

impl Perform for TerminalState {
  fn print(&mut self, c: char) {
    self.grid.put(self.cursor, c);
    self.cursor.col += 1;
  }

  fn execute(&mut self, b: u8) {
    match b {
      b'\n' => self.cursor.row += 1,
      b'\r' => self.cursor.col = 0,
      _ => (),
    }
  }

  fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
    println!("unhandled ESC: {}", _byte);
  }

  fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
    println!("unhandled OSC: {:?}", _params);
  }

  fn csi_dispatch(
    &mut self,
    _params: &anstyle_parse::Params,
    _intermediates: &[u8],
    _ignore: bool,
    _action: u8,
  ) {
    println!("unhandled CSI: {}", _action);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn terminal_works() {
    let mut terminal = Terminal::new();

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
