use anstyle_parse::{Parser, Perform, Utf8Parser};

use crate::{grid::Grid, pty::Pty};

mod grid;
mod pty;

pub struct Terminal {
  pty:  Pty,
  grid: Grid,

  parser: Parser,
}

impl Terminal {
  pub fn new() -> Self {
    Terminal { pty: Pty::new(), grid: Grid::new(), parser: Parser::<Utf8Parser>::new() }
  }

  pub fn perform_input(&mut self, c: char) { self.pty.input(c); }

  pub fn line(&self, index: usize) -> Option<&str> { self.grid.line(index) }

  pub fn update(&mut self) {
    loop {
      let mut buf = [0u8; 1024];

      match self.pty.read(&mut buf) {
        Ok(0) => break,
        Ok(n) => {
          for &b in &buf[..n] {
            self.parser.advance(&mut self.grid, b);
          }
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
        Err(e) => println!("{}", e),
      }
    }
  }
}

impl Perform for Grid {
  fn print(&mut self, _c: char) {}
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn terminal_works() {
    let mut terminal = Terminal::new();

    std::thread::sleep(std::time::Duration::from_millis(100));

    terminal.update();
    for line in terminal.grid.lines() {
      println!("{}", line);
    }

    panic!();
  }
}
