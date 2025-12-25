use std::ops::Range;

use crate::{Position, Size, Style};

pub struct Grid {
  lines: Vec<Vec<Cell>>,
  size:  Size,
}

#[derive(Clone, Copy)]
struct Cell {
  c:     char,
  style: Style,
}

pub struct OwnedLine {
  // TODO: Scrollback!
  #[allow(unused)]
  cells: Vec<Cell>,
}

pub struct Line<'a> {
  line: &'a [Cell],
}

pub struct LineMut<'a> {
  line: &'a mut [Cell],
}

pub struct StyleIter<'a> {
  line:   &'a [Cell],
  prev:   Style,
  index:  usize,
  offset: usize,
}

impl Default for Cell {
  fn default() -> Self { Cell { c: ' ', style: Style::default() } }
}

impl Grid {
  pub fn new(size: Size) -> Self {
    Grid { lines: vec![vec![Cell::default(); size.cols]; size.rows], size }
  }

  pub fn put(&mut self, pos: Position, c: char, style: Style) {
    if pos.row >= self.lines.len() {
      return;
    }
    if pos.col >= self.lines[pos.row].len() {
      return;
    }

    self.lines[pos.row][pos.col].c = c;
    self.lines[pos.row][pos.col].style = style;
  }

  pub fn line(&self, index: usize) -> Option<Line<'_>> {
    self.lines.get(index).map(|line| Line { line })
  }

  pub fn line_mut(&mut self, index: usize) -> LineMut<'_> {
    LineMut { line: self.lines.get_mut(index).expect("line out of bounds") }
  }

  pub fn resize(&mut self, size: Size) {
    self.lines.resize(size.rows, vec![]);

    for line in &mut self.lines {
      line.resize(size.cols, Cell::default());
    }

    self.size = size;
  }

  pub fn clear(&mut self, style: Style) {
    for line in &mut self.lines {
      LineMut { line }.clear(style);
    }
  }

  pub fn scroll_down(&mut self, range: Range<usize>) {
    self.lines.insert(range.start, vec![Cell::default(); self.size.cols]);
    self.lines.remove(range.end - 1);
  }

  pub fn scroll_up(&mut self, range: Range<usize>) -> OwnedLine {
    let cells = self.lines.remove(range.start);
    if range.end < self.lines.len() {
      self.lines.insert(range.end, vec![Cell::default(); self.size.cols]);
    } else {
      self.lines.push(vec![Cell::default(); self.size.cols]);
    }
    OwnedLine { cells }
  }
}

impl<'a> LineMut<'a> {
  pub fn clear(&mut self, style: Style) {
    let cell = Cell { c: ' ', style };
    self.line.fill(cell);
  }

  pub fn clear_range(&mut self, range: std::ops::RangeInclusive<usize>, style: Style) {
    for i in range {
      self.line[i].c = ' ';
      self.line[i].style = style;
    }
  }

  pub(crate) fn shift_right_from(&mut self, col: usize) {
    for i in (col + 1..self.line.len()).rev() {
      self.line.swap(i - 1, i);
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

  pub fn styles(&self) -> StyleIter<'_> {
    StyleIter { line: self.line, prev: Style::default(), index: 0, offset: 0 }
  }
}

impl Iterator for StyleIter<'_> {
  type Item = (Style, usize);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      let cell = self.line.get(self.index)?;
      let style = self.prev;
      let offset = self.offset;
      self.index += 1;
      self.offset += cell.c.len_utf8();
      if cell.style != self.prev {
        self.prev = cell.style;
        return Some((style, offset));
      }
    }
  }
}
