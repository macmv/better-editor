pub struct Grid {
  lines: Vec<String>,
}

impl Grid {
  pub fn new() -> Self { Grid { lines: Vec::new() } }

  pub fn write(&mut self, bytes: &[u8]) {
    self.lines.push(String::from_utf8_lossy(bytes).to_string());
  }
}
