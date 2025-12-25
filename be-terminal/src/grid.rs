use crate::Cursor;
use unicode_width::UnicodeWidthChar;

pub struct Grid {
  lines: Vec<String>,
}

impl Grid {
  pub fn new() -> Self { Grid { lines: Vec::new() } }

  pub fn put(&mut self, pos: Cursor, c: char) {
    if self.lines.len() <= pos.row {
      self.lines.resize(pos.row + 1, " ".repeat(80));
    }

    let line = &mut self.lines[pos.row];
    let range = column_offset(line, pos.col);

    let mut s = [0; 4];
    let s = c.encode_utf8(&mut s);
    line.replace_range(range, s);
  }

  #[cfg(test)]
  pub fn lines(&self) -> impl Iterator<Item = &str> { self.lines.iter().map(|s| s.as_str()) }
  pub fn line(&self, index: usize) -> Option<&str> { self.lines.get(index).map(|s| s.as_str()) }
}

fn column_offset(line: &str, column: usize) -> std::ops::Range<usize> {
  let mut col = 0;
  let mut offset = 0;

  for c in line.chars() {
    if col >= column {
      return offset..offset + c.len_utf8();
    }
    col += c.width().unwrap_or(0);
    offset += c.len_utf8();
  }

  0..line.chars().next().unwrap().len_utf8()
}
