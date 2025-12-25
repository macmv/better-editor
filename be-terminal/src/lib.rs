use crate::{grid::Grid, pty::Pty};

mod grid;
mod pty;

pub struct Terminal {
  pty:  Pty,
  grid: Grid,
}

impl Terminal {
  pub fn new() -> Self { Terminal { pty: Pty::new(), grid: Grid::new() } }

  pub fn perform_input(&mut self, c: char) { self.pty.input(c); }

  pub fn update(&mut self) {
    loop {
      let mut buf = [0u8; 1024];
      match self.pty.read(&mut buf) {
        Ok(0) => break,
        Ok(n) => self.grid.write(&buf[..n]),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
        Err(e) => println!("{}", e),
      }
    }
  }
}
