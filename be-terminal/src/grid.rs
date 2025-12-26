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

pub struct SpecificStyleIter<'a, F, T> {
  line:   &'a [Cell],
  prev:   Option<T>,
  index:  usize,
  offset: usize,
  func:   F,
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
    for line in (range.start + 1..range.end).rev() {
      self.lines.swap(line, line - 1);
    }
    self.line_mut(range.start).clear(Style::default());
  }

  pub fn scroll_up(&mut self, range: Range<usize>) -> OwnedLine {
    let line = OwnedLine { cells: self.lines[range.start].clone() };

    for line in range.start..range.end - 1 {
      self.lines.swap(line, line + 1);
    }
    self.line_mut(range.end - 1).clear(Style::default());

    line
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

  pub fn specific_styles<T, F>(&self, f: F) -> SpecificStyleIter<'_, F, T>
  where
    F: Fn(Style) -> T,
  {
    SpecificStyleIter { line: self.line, prev: None, index: 0, offset: 0, func: f }
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

impl<T, F> Iterator for SpecificStyleIter<'_, F, T>
where
  F: Fn(Style) -> T,
  T: Clone + PartialEq,
{
  type Item = (T, usize);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      let cell = self.line.get(self.index)?;
      let offset = self.offset;
      self.index += 1;
      self.offset += cell.c.len_utf8();

      if self.index == 1 {
        self.prev = Some((self.func)(cell.style));
        continue;
      }

      let v = (self.func)(cell.style);
      if self.prev.as_ref() != Some(&v) {
        let ret = self.prev.clone();
        self.prev = Some(v);
        return Some((ret.unwrap(), offset));
      }
    }
  }
}
