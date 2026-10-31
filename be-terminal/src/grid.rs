use std::{
  ops::{Deref, Range},
  sync::atomic::{AtomicU64, Ordering},
};

use crate::{Position, Size, Style};

static NEXT_LINE_ID: AtomicU64 = AtomicU64::new(1);

pub struct Grid {
  lines: Vec<OwnedLine>,
  size:  Size,

  /// Stamped onto every line that gets mutated. See `OwnedLine::stamp`.
  epoch: u64,
}

#[derive(Clone, Copy)]
pub struct Cell {
  c:     char,
  style: Style,
}

/// A line of cells, with enough identity for a renderer to cache what it drew.
///
/// `id` follows the line as it moves around (scrolling, insert/delete line,
/// into scrollback), and `stamp` changes whenever the cells do. So `(id,
/// stamp)` equal to a previous value means the line is byte for byte what was
/// drawn last time.
pub struct OwnedLine {
  cells: Vec<Cell>,
  id:    u64,
  stamp: u64,
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

impl OwnedLine {
  fn blank(cols: usize, stamp: u64) -> Self {
    OwnedLine {
      cells: vec![Cell::default(); cols],
      id: NEXT_LINE_ID.fetch_add(1, Ordering::Relaxed),
      stamp,
    }
  }

  /// Identifies this line across scrolling. Never reused for a different
  /// allocation of the same content.
  pub fn id(&self) -> u64 { self.id }

  /// Changes whenever the cells in this line change.
  pub fn stamp(&self) -> u64 { self.stamp }

  /// Turns this line into a new blank one, keeping the allocation.
  fn recycle(&mut self, cols: usize, style: Style, stamp: u64) {
    self.cells.clear();
    self.cells.resize(cols, Cell { c: ' ', style });
    self.id = NEXT_LINE_ID.fetch_add(1, Ordering::Relaxed);
    self.stamp = stamp;
  }
}

impl Grid {
  pub fn new(size: Size) -> Self {
    Grid { lines: (0..size.rows).map(|_| OwnedLine::blank(size.cols, 0)).collect(), size, epoch: 0 }
  }

  /// Sets the stamp given to lines mutated from now on. Must change between a
  /// renderer reading a line and the next time the line may be written.
  pub fn set_epoch(&mut self, epoch: u64) { self.epoch = epoch; }

  pub fn put(&mut self, pos: Position, c: char, style: Style) {
    if pos.row >= self.lines.len() {
      return;
    }
    let line = &mut self.lines[pos.row];
    if pos.col >= line.cells.len() {
      return;
    }

    line.stamp = self.epoch;
    line.cells[pos.col].c = c;
    line.cells[pos.col].style = style;
  }

  /// Writes a run of printable ASCII, starting at `pos`. The run must fit in
  /// the line.
  pub fn put_ascii(&mut self, pos: Position, run: &[u8], style: Style) {
    let line = &mut self.lines[pos.row];
    line.stamp = self.epoch;
    for (cell, &b) in line.cells[pos.col..pos.col + run.len()].iter_mut().zip(run) {
      *cell = Cell { c: b as char, style };
    }
  }

  pub fn line(&self, index: usize) -> Option<&OwnedLine> { self.lines.get(index) }

  pub fn line_mut(&mut self, index: usize) -> &mut Line {
    let line = self.lines.get_mut(index).expect("line out of bounds");
    line.stamp = self.epoch;
    Line::from_mut(&mut line.cells)
  }

  pub fn resize(&mut self, size: Size) {
    let epoch = self.epoch;
    self.lines.resize_with(size.rows, || OwnedLine::blank(0, epoch));

    for line in &mut self.lines {
      line.cells.resize(size.cols, Cell::default());
      line.stamp = epoch;
    }

    self.size = size;
  }

  pub fn clear(&mut self, style: Style) {
    for line in &mut self.lines {
      line.stamp = self.epoch;
      Line::from_mut(&mut line.cells).clear(style);
    }
  }

  /// Swaps two lines. They keep their ids and stamps, as nothing in them
  /// changed.
  pub fn swap(&mut self, a: usize, b: usize) { self.lines.swap(a, b); }

  pub fn scroll_down(&mut self, range: Range<usize>, style: Style) {
    self.lines[range.clone()].rotate_right(1);
    self.line_mut(range.start).clear(style);
  }

  /// Scrolls `range` up one line, and returns the line that fell off the top.
  /// `spare` is a line to reuse the allocation of for the new blank line at the
  /// bottom.
  pub fn scroll_up(
    &mut self,
    range: Range<usize>,
    style: Style,
    spare: Option<OwnedLine>,
  ) -> OwnedLine {
    let mut fresh = spare.unwrap_or_else(|| OwnedLine::blank(0, self.epoch));
    fresh.recycle(self.size.cols, style, self.epoch);

    self.lines[range.clone()].rotate_left(1);
    std::mem::replace(&mut self.lines[range.end - 1], fresh)
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
