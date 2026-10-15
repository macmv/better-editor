//! Iterators over a [`RopeSlice`](crate::RopeSlice). They all walk the tree
//! with cursors, so starting one costs O(log n) and each step is O(1) (apart
//! from reverse grapheme iteration, which costs O(log n) per step).

use be_sumtree::{Bias, Cursor};

use crate::{
  RopeSlice,
  grapheme::{self, State},
  rope::Chunk,
  summary::{ByteOffset, Offsets},
};

/// The pieces of text a slice is stored in, in order.
#[derive(Clone)]
pub struct Chunks<'a> {
  slice: RopeSlice<'a>,
  front: Cursor<'a, Chunk, Offsets>,
  /// Only positioned once the iterator is used from the back.
  back:  Option<Cursor<'a, Chunk, Offsets>>,
  /// The part of the slice not yet yielded, as absolute offsets.
  fpos:  usize,
  bpos:  usize,
}

impl<'a> Chunks<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> Chunks<'a> {
    let mut front = slice.rope.tree.cursor::<Offsets>();
    if !slice.is_empty() {
      front.seek(&ByteOffset(slice.start), Bias::Right);
    }
    Chunks { slice, front, back: None, fpos: slice.start, bpos: slice.end }
  }
}

impl<'a> Iterator for Chunks<'a> {
  type Item = &'a str;

  fn next(&mut self) -> Option<&'a str> {
    if self.fpos >= self.bpos {
      return None;
    }

    let chunk: &'a Chunk = self.front.item()?;
    let start = self.front.start().bytes;
    let text: &'a str = &chunk.0;
    let lo = self.fpos - start;
    let hi = text.len().min(self.bpos - start);
    self.fpos = start + hi;
    self.front.advance();
    Some(&text[lo..hi])
  }
}

impl<'a> DoubleEndedIterator for Chunks<'a> {
  fn next_back(&mut self) -> Option<&'a str> {
    if self.fpos >= self.bpos {
      return None;
    }

    let back = self.back.get_or_insert_with(|| {
      let mut cursor = self.slice.rope.tree.cursor::<Offsets>();
      cursor.seek(&ByteOffset(self.bpos), Bias::Left);
      cursor
    });

    let chunk: &'a Chunk = back.item()?;
    let start = back.start().bytes;
    let text: &'a str = &chunk.0;
    let lo = self.fpos.max(start) - start;
    let hi = self.bpos - start;
    self.bpos = start + lo;
    back.prev();
    Some(&text[lo..hi])
  }
}

/// The characters of a slice.
#[derive(Clone)]
pub struct Chars<'a> {
  chunks: Chunks<'a>,
  front:  std::str::Chars<'a>,
  back:   std::str::Chars<'a>,
}

impl<'a> Chars<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> Chars<'a> {
    Chars { chunks: slice.chunks(), front: "".chars(), back: "".chars() }
  }
}

impl Iterator for Chars<'_> {
  type Item = char;

  fn next(&mut self) -> Option<char> {
    loop {
      if let Some(c) = self.front.next() {
        return Some(c);
      }
      match self.chunks.next() {
        Some(chunk) => self.front = chunk.chars(),
        None => return self.back.next(),
      }
    }
  }
}

impl DoubleEndedIterator for Chars<'_> {
  fn next_back(&mut self) -> Option<char> {
    loop {
      if let Some(c) = self.back.next_back() {
        return Some(c);
      }
      match self.chunks.next_back() {
        Some(chunk) => self.back = chunk.chars(),
        None => return self.front.next_back(),
      }
    }
  }
}

/// The bytes of a slice.
#[derive(Clone)]
pub struct Bytes<'a> {
  chunks: Chunks<'a>,
  front:  std::str::Bytes<'a>,
  back:   std::str::Bytes<'a>,
}

impl<'a> Bytes<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> Bytes<'a> {
    Bytes { chunks: slice.chunks(), front: "".bytes(), back: "".bytes() }
  }
}

impl Iterator for Bytes<'_> {
  type Item = u8;

  fn next(&mut self) -> Option<u8> {
    loop {
      if let Some(b) = self.front.next() {
        return Some(b);
      }
      match self.chunks.next() {
        Some(chunk) => self.front = chunk.bytes(),
        None => return self.back.next(),
      }
    }
  }
}

impl DoubleEndedIterator for Bytes<'_> {
  fn next_back(&mut self) -> Option<u8> {
    loop {
      if let Some(b) = self.back.next_back() {
        return Some(b);
      }
      match self.chunks.next_back() {
        Some(chunk) => self.back = chunk.bytes(),
        None => return self.front.next_back(),
      }
    }
  }
}

/// The characters of a slice, with the offset each starts at.
#[derive(Clone)]
struct CharOffsets<'a> {
  chunks: Chunks<'a>,
  cur:    std::str::CharIndices<'a>,
  /// The offset the current chunk starts at.
  base:   usize,
  /// The offset the next chunk starts at.
  loaded: usize,
}

impl<'a> CharOffsets<'a> {
  fn new(slice: RopeSlice<'a>) -> CharOffsets<'a> {
    CharOffsets { chunks: slice.chunks(), cur: "".char_indices(), base: 0, loaded: 0 }
  }
}

impl Iterator for CharOffsets<'_> {
  type Item = (usize, char);

  fn next(&mut self) -> Option<(usize, char)> {
    loop {
      if let Some((i, c)) = self.cur.next() {
        return Some((self.base + i, c));
      }
      let chunk = self.chunks.next()?;
      self.base = self.loaded;
      self.loaded += chunk.len();
      self.cur = chunk.char_indices();
    }
  }
}

/// The grapheme clusters of a slice.
///
/// Going forward is O(1) per cluster. Going backward is O(log n) per cluster,
/// because the segmentation only runs forward.
#[derive(Clone)]
pub struct Graphemes<'a> {
  slice:   RopeSlice<'a>,
  chars:   CharOffsets<'a>,
  /// The first character of the next cluster, already read from `chars`.
  pending: Option<(usize, char)>,
  /// The segmenter's state before `pending` (or the next character).
  state:   State,
  /// The number of clusters yielded from the front.
  front:   usize,
  /// Where the unyielded clusters end, and how many clusters come before that.
  /// Set once the iterator is used from the back.
  back:    Option<(usize, usize)>,
}

impl<'a> Graphemes<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> Graphemes<'a> {
    Graphemes {
      slice,
      chars: CharOffsets::new(slice),
      pending: None,
      state: State::START,
      front: 0,
      back: None,
    }
  }
}

impl<'a> Iterator for Graphemes<'a> {
  type Item = RopeSlice<'a>;

  fn next(&mut self) -> Option<RopeSlice<'a>> {
    if let Some((_, count)) = self.back
      && self.front >= count
    {
      return None;
    }

    let (start, first) = match self.pending.take() {
      Some(p) => p,
      None => self.chars.next()?,
    };
    // The first character always begins the cluster.
    let (state, begins) = grapheme::step(self.state, first);
    debug_assert!(begins);
    self.state = state;

    let mut end = self.slice.byte_len();
    while let Some((offset, c)) = self.chars.next() {
      let (state, begins) = grapheme::step(self.state, c);
      if begins {
        self.pending = Some((offset, c));
        end = offset;
        break;
      }
      self.state = state;
    }

    self.front += 1;
    Some(self.slice.sub(start, end))
  }
}

impl<'a> DoubleEndedIterator for Graphemes<'a> {
  fn next_back(&mut self) -> Option<RopeSlice<'a>> {
    let (end, count) = match self.back {
      Some(back) => back,
      None => (self.slice.byte_len(), self.slice.grapheme_len()),
    };
    if count <= self.front {
      self.back = Some((end, count));
      return None;
    }

    let index = count - 1;
    let start = self.slice.byte_of_grapheme(index);
    self.back = Some((start, index));
    Some(self.slice.sub(start, end))
  }
}

/// The lines of a slice, each with its line break.
#[derive(Clone)]
pub struct RawLines<'a> {
  slice: RopeSlice<'a>,
  /// The part of the slice not yet yielded.
  start: usize,
  end:   usize,
}

impl<'a> RawLines<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> RawLines<'a> {
    RawLines { slice, start: 0, end: slice.byte_len() }
  }
}

impl<'a> Iterator for RawLines<'a> {
  type Item = RopeSlice<'a>;

  fn next(&mut self) -> Option<RopeSlice<'a>> {
    if self.start >= self.end {
      return None;
    }

    // The line ends after the first newline.
    let rest = self.slice.sub(self.start, self.end);
    let mut offset = 0;
    let mut line_end = self.end;
    for chunk in rest.chunks() {
      if let Some(i) = chunk.find('\n') {
        line_end = self.start + offset + i + 1;
        break;
      }
      offset += chunk.len();
    }

    let line = self.slice.sub(self.start, line_end);
    self.start = line_end;
    Some(line)
  }
}

impl<'a> DoubleEndedIterator for RawLines<'a> {
  fn next_back(&mut self) -> Option<RopeSlice<'a>> {
    if self.start >= self.end {
      return None;
    }

    // The last line starts after the last newline that is not its own.
    let search_end =
      if self.slice.byte_at(self.end - 1) == b'\n' { self.end - 1 } else { self.end };

    let before = self.slice.sub(self.start, search_end);
    let mut end = search_end;
    let mut line_start = self.start;
    for chunk in before.chunks().rev() {
      if let Some(i) = chunk.rfind('\n') {
        line_start = end - chunk.len() + i + 1;
        break;
      }
      end -= chunk.len();
    }

    let line = self.slice.sub(line_start, self.end);
    self.end = line_start;
    Some(line)
  }
}

/// The lines of a slice, without their line breaks. A final line break does
/// not make an empty last line.
#[derive(Clone)]
pub struct Lines<'a> {
  raw: RawLines<'a>,
}

impl<'a> Lines<'a> {
  pub(crate) fn new(slice: RopeSlice<'a>) -> Lines<'a> { Lines { raw: RawLines::new(slice) } }
}

/// Removes a trailing `\n` or `\r\n`.
fn strip_line_break(line: RopeSlice<'_>) -> RopeSlice<'_> {
  let mut end = line.byte_len();
  if end > 0 && line.byte_at(end - 1) == b'\n' {
    end -= 1;
    if end > 0 && line.byte_at(end - 1) == b'\r' {
      end -= 1;
    }
  }
  line.sub(0, end)
}

impl<'a> Iterator for Lines<'a> {
  type Item = RopeSlice<'a>;

  fn next(&mut self) -> Option<RopeSlice<'a>> { self.raw.next().map(strip_line_break) }
}

impl<'a> DoubleEndedIterator for Lines<'a> {
  fn next_back(&mut self) -> Option<RopeSlice<'a>> { self.raw.next_back().map(strip_line_break) }
}
