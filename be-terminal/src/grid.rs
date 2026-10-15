use std::ops::{Deref, DerefMut, Range};

use crate::{Position, Size, Style};

pub struct Grid {
  lines: Vec<OwnedLine>,
  size:  Size,
}

#[derive(Clone, Copy)]
pub struct Cell {
  c:     char,
  style: Style,
}

#[derive(Clone)]
pub struct OwnedLine {
  cells: Vec<Cell>,
}

#[repr(transparent)]
pub struct Line {
  line: [Cell],
}

pub struct StyleIter<'a> {
  line:   &'a [Cell],
  prev:   Style,
  index:  usize,
  offset: usize,
  done:   bool,
}

pub struct SpecificStyleIter<'a, F, T> {
  line:   &'a [Cell],
  prev:   Option<T>,
  index:  usize,
  offset: usize,
  func:   F,
  done:   bool,
}

impl Default for Cell {
  fn default() -> Self { Cell { c: ' ', style: Style::default() } }
}

impl Grid {
  pub fn new(size: Size) -> Self {
    Grid { lines: vec![OwnedLine { cells: vec![Cell::default(); size.cols] }; size.rows], size }
  }

  pub fn put(&mut self, pos: Position, c: char, style: Style) {
    if pos.row >= self.lines.len() {
      return;
    }
    if pos.col >= self.lines[pos.row].cells.len() {
      return;
    }

    self.lines[pos.row].cells[pos.col].c = c;
    self.lines[pos.row].cells[pos.col].style = style;
  }

  pub fn line(&self, index: usize) -> Option<&Line> {
    self.lines.get(index).map(|line| Line::from_slice(&line.cells))
  }

  pub fn line_mut(&mut self, index: usize) -> &mut Line {
    Line::from_mut(&mut self.lines.get_mut(index).expect("line out of bounds").cells)
  }

  pub fn resize(&mut self, size: Size) {
    self.lines.resize(size.rows, OwnedLine { cells: vec![] });

    for line in &mut self.lines {
      line.cells.resize(size.cols, Cell::default());
    }

    self.size = size;
  }

  pub fn clear(&mut self, style: Style) {
    for line in &mut self.lines {
      Line::from_mut(&mut line.cells).clear(style);
    }
  }

  pub fn swap(&mut self, a: usize, b: usize) { self.lines.swap(a, b); }

  pub fn scroll_down(&mut self, range: Range<usize>, style: Style) {
    for line in (range.start + 1..range.end).rev() {
      self.lines.swap(line, line - 1);
    }
    self.line_mut(range.start).clear(style);
  }

  pub fn scroll_up(&mut self, range: Range<usize>, style: Style) -> OwnedLine {
    let line = self.lines[range.start].clone();

    for line in range.start..range.end - 1 {
      self.lines.swap(line, line + 1);
    }
    self.line_mut(range.end - 1).clear(style);

    line
  }
}

impl Line {
  pub fn from_slice<'a>(line: &'a [Cell]) -> &'a Self {
    // SAFETY: `Line` is repr(transparent)
    unsafe { std::mem::transmute(line) }
  }

  pub fn from_mut<'a>(line: &'a mut [Cell]) -> &'a mut Self {
    // SAFETY: `Line` is repr(transparent)
    unsafe { std::mem::transmute(line) }
  }

  pub fn to_string(&self) -> String { self.slice_to_string(..) }

  pub fn slice_to_string<R>(&self, range: R) -> String
  where
    R: std::slice::SliceIndex<[crate::grid::Cell], Output = [crate::grid::Cell]>,
  {
    let mut slice = String::new();
    for c in &self.line[range] {
      if c.c != '\0' {
        slice.push(c.c);
      }
    }
    slice
  }

  pub fn styles(&self) -> StyleIter<'_> {
    StyleIter { line: &self.line, prev: Style::default(), index: 0, offset: 0, done: false }
  }

  pub fn specific_styles<T, F>(&self, f: F) -> SpecificStyleIter<'_, F, T>
  where
    F: Fn(usize, Style) -> T,
  {
    SpecificStyleIter {
      line:   &self.line,
      prev:   None,
      index:  0,
      offset: 0,
      func:   f,
      done:   false,
    }
  }

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

  pub(crate) fn delete_chars_at(&mut self, col: usize, count: usize, style: Style) {
    let len = self.line.len();
    let count = count.min(len.saturating_sub(col));
    for i in col..len - count {
      self.line[i] = self.line[i + count];
    }
    let fill = Cell { c: ' ', style };
    for i in len - count..len {
      self.line[i] = fill;
    }
  }
}

impl Deref for OwnedLine {
  type Target = Line;

  fn deref(&self) -> &Line { Line::from_slice(&self.cells) }
}

impl DerefMut for OwnedLine {
  fn deref_mut(&mut self) -> &mut Line { Line::from_mut(&mut self.cells) }
}

impl Iterator for StyleIter<'_> {
  type Item = (Style, usize);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      let Some(cell) = self.line.get(self.index) else {
        if !self.done {
          self.done = true;
          return Some((self.prev, self.offset));
        } else {
          return None;
        }
      };
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
  F: Fn(usize, Style) -> T,
  T: Clone + PartialEq,
{
  type Item = (T, usize);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      let Some(cell) = self.line.get(self.index) else {
        if !self.done {
          self.done = true;
          return self.prev.take().map(|v| (v, self.offset));
        } else {
          return None;
        }
      };
      let offset = self.offset;
      self.index += 1;
      self.offset += cell.c.len_utf8();

      if self.index == 1 {
        self.prev = Some((self.func)(self.index - 1, cell.style));
        continue;
      }

      let v = (self.func)(self.index - 1, cell.style);
      if self.prev.as_ref() != Some(&v) {
        let ret = self.prev.clone();
        self.prev = Some(v);
        return Some((ret.unwrap(), offset));
      }
    }
  }
}
