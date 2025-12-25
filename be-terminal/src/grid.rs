use crate::{Cursor, Size, Style};
use unicode_width::UnicodeWidthChar;

pub struct Grid {
  lines: Vec<Vec<Cell>>,
}

#[derive(Default, Clone, Copy)]
struct Cell {
  c:     char,
  style: Style,
}

pub struct Line<'a> {
  line: &'a [Cell],
}

impl Grid {
  pub fn new(size: Size) -> Self {
    Grid { lines: vec![vec![Cell::default(); size.cols]; size.rows] }
  }

  pub fn put(&mut self, pos: Cursor, c: char) {
    if pos.row >= self.lines.len() {
      return;
    }

    self.lines[pos.row][pos.col].c = c;
  }

  pub fn line(&self, index: usize) -> Option<Line<'_>> {
    self.lines.get(index).map(|line| Line { line })
  }

  pub fn resize(&mut self, size: Size) {
    self.lines.resize(size.rows, vec![]);

    for line in &mut self.lines {
      line.resize(size.cols, Cell::default());
    }
  }
}

impl Line<'_> {
  pub fn to_string(&self) -> String {
    let mut line = String::new();
    for c in self.line {
      if c.c != '\0' {
        line.push(c.c);
      }
    }
    line
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
