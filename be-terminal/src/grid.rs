use crate::{Cursor, Size};
use unicode_width::UnicodeWidthChar;

pub struct Grid {
  lines: Vec<String>,
}

impl Grid {
  pub fn new(size: Size) -> Self { Grid { lines: vec![" ".repeat(size.cols); size.rows] } }

  pub fn put(&mut self, pos: Cursor, c: char) {
    if pos.row >= self.lines.len() {
      return;
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

  pub fn resize(&mut self, size: Size) {
    self.lines.resize(size.rows, " ".repeat(size.cols));

    for line in &mut self.lines {
      if line.len() < size.cols {
        line.push_str(&" ".repeat(size.cols - line.len()));
      } else {
        line.truncate(size.cols);
      }
    }
  }
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
