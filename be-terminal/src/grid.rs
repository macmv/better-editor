pub struct Grid {
  lines: Vec<String>,
}

impl Grid {
  pub fn new() -> Self { Grid { lines: Vec::new() } }

  pub fn write(&mut self, bytes: &[u8]) {
    self.lines.push(String::from_utf8_lossy(bytes).to_string());
  }

  pub fn lines(&self) -> impl Iterator<Item = &str> { self.lines.iter().map(|s| s.as_str()) }
  pub fn line(&self, index: usize) -> Option<&str> { self.lines.get(index).map(|s| s.as_str()) }
}
