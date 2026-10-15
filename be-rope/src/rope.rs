use std::{
  borrow::Cow,
  fmt,
  ops::{Range, RangeBounds},
  sync::Arc,
};

use be_sumtree::{Bias, Item, SumTree};

use crate::{
  RopeSlice,
  iter::{Bytes, Chars, Chunks, Graphemes, Lines, RawLines},
  summary::{ByteOffset, Offsets, Pos, TextSummary},
};

/// The most bytes in a chunk. Chunks are not cut inside a character, so this
/// must leave room for rounding (see `split_even`).
#[cfg(not(any(test, feature = "small-chunks")))]
pub(crate) const MAX_CHUNK: usize = 1024;
// Tiny chunks make the tests build deep trees from short strings.
#[cfg(any(test, feature = "small-chunks"))]
pub(crate) const MAX_CHUNK: usize = 16;

/// Edits merge a chunk with its neighbors if it would otherwise end up smaller
/// than this, so the tree does not fill up with tiny chunks.
pub(crate) const MIN_CHUNK: usize = MAX_CHUNK / 2;

/// A piece of text. Chunks are never empty.
#[derive(Clone)]
pub(crate) struct Chunk(pub(crate) Arc<str>);

impl Item for Chunk {
  type Summary = TextSummary;

  fn summary(&self) -> TextSummary { TextSummary::of(&self.0) }
}

/// Cuts `text` into pieces of at most `MAX_CHUNK` bytes, as even as possible,
/// without splitting a character. Short text is a single piece.
fn split_even(text: &str) -> Vec<&str> {
  if text.is_empty() {
    return vec![];
  }
  if text.len() <= MAX_CHUNK {
    return vec![text];
  }

  // Cut points are rounded down to a character boundary, which makes a piece
  // up to 3 bytes longer than its share.
  let n = text.len().div_ceil(MAX_CHUNK - 3);
  let mut pieces = Vec::with_capacity(n);
  let mut start = 0;
  for i in 1..=n {
    let mut end = if i == n { text.len() } else { text.len() * i / n };
    while !text.is_char_boundary(end) {
      end -= 1;
    }
    pieces.push(&text[start..end]);
    start = end;
  }
  pieces
}

/// Bulk loads with at least this many chunks compute their summaries on several
/// threads.
#[cfg(not(test))]
const PARALLEL_MIN: usize = 2048;
// Low enough for the tests to cover the parallel path.
#[cfg(test)]
const PARALLEL_MIN: usize = 8;

/// Builds a tree from chunks. A chunk's summary means scanning all of its
/// text, which is most of the cost of loading a large file, and the chunks are
/// independent, so big loads split that work across threads.
fn tree_from_chunks(chunks: Vec<Chunk>) -> SumTree<Chunk> {
  let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
  if chunks.len() < PARALLEL_MIN || threads == 1 {
    return chunks.into_iter().collect();
  }

  let per_thread = chunks.len().div_ceil(threads);
  let mut summaries = Vec::with_capacity(chunks.len());
  std::thread::scope(|scope| {
    let handles: Vec<_> = chunks
      .chunks(per_thread)
      .map(|part| scope.spawn(move || part.iter().map(Item::summary).collect::<Vec<_>>()))
      .collect();
    for handle in handles {
      summaries.extend(handle.join().expect("summary thread panicked"));
    }
  });
  SumTree::from_parts(chunks, summaries)
}

fn chunk_tree(text: &str) -> SumTree<Chunk> {
  tree_from_chunks(split_even(text).into_iter().map(|piece| Chunk(Arc::from(piece))).collect())
}

/// A persistent text buffer.
///
/// Cloning is O(1), and a clone is an immutable snapshot: editing one copy
/// never changes another. All offsets are in bytes unless a method says
/// otherwise, and must be on character boundaries.
#[derive(Clone, Default)]
pub struct Rope {
  pub(crate) tree: SumTree<Chunk>,
}

impl Rope {
  pub fn new() -> Rope { Rope::default() }

  /// The slice covering the whole rope.
  pub fn as_slice(&self) -> RopeSlice<'_> {
    RopeSlice { rope: self, start: 0, end: self.byte_len() }
  }

  pub fn byte_len(&self) -> usize { self.tree.dimension::<Offsets>().bytes }

  pub fn is_empty(&self) -> bool { self.tree.is_empty() }

  /// Replaces the bytes in `range` with `text`.
  ///
  /// # Panics
  ///
  /// Panics if the range is out of bounds, or does not start and end on
  /// character boundaries.
  #[track_caller]
  pub fn replace(&mut self, range: Range<usize>, text: &str) {
    let len = self.byte_len();
    assert!(range.start <= range.end, "range start {} is after its end {}", range.start, range.end);
    assert!(range.end <= len, "range end {} is out of bounds (length {len})", range.end);

    if self.tree.is_empty() {
      self.tree = chunk_tree(text);
      return;
    }

    // Find the chunks holding either end of the range. A boundary between two
    // chunks belongs to the earlier one, so inserting at the end of a chunk
    // appends to it.
    let mut first = self.tree.cursor::<Offsets>();
    first.seek(&ByteOffset(range.start), Bias::Left);
    let mut last = self.tree.cursor::<Offsets>();
    last.seek(&ByteOffset(range.end), Bias::Left);

    let first_chunk = first.item().expect("range start is in bounds");
    let last_chunk = last.item().expect("range end is in bounds");
    let mut lo = first.start().bytes;
    let mut hi = last.start().bytes + last_chunk.0.len();

    let before = &first_chunk.0[..range.start - lo];
    let after = &last_chunk.0[range.end - last.start().bytes..];
    let mut middle = String::with_capacity(before.len() + text.len() + after.len());
    middle.push_str(before);
    middle.push_str(text);
    middle.push_str(after);

    // Absorb neighbors rather than leave a tiny chunk behind.
    while middle.len() < MIN_CHUNK {
      if first.prev() {
        let chunk = first.item().unwrap();
        middle.insert_str(0, &chunk.0);
        lo = first.start().bytes;
      } else {
        last.advance();
        let Some(chunk) = last.item() else { break };
        middle.push_str(&chunk.0);
        hi += chunk.0.len();
      }
    }

    // Everything from `lo` to `hi` is rewritten as `middle`.
    let (head, tail) = self.tree.split_at::<Offsets, _>(&ByteOffset(hi), Bias::Right);
    let (mut new, _) = head.split_at::<Offsets, _>(&ByteOffset(lo), Bias::Right);
    new.append(chunk_tree(&middle));
    new.append(tail);
    self.tree = new;
  }

  #[track_caller]
  pub fn insert(&mut self, at: usize, text: &str) { self.replace(at..at, text) }

  #[track_caller]
  pub fn delete(&mut self, range: Range<usize>) { self.replace(range, "") }

  /// The additive offsets (everything except graphemes) at a byte.
  #[track_caller]
  pub(crate) fn offsets_at(&self, byte: usize) -> Offsets {
    let mut cursor = self.tree.cursor::<Offsets>();
    cursor.seek(&ByteOffset(byte), Bias::Left);

    let mut pos = *cursor.start();
    match cursor.item() {
      Some(chunk) => pos.add_text(&chunk.0[..byte - pos.bytes]),
      None => assert!(pos.bytes == byte, "byte index {byte} is out of bounds"),
    }
    pos
  }

  /// Every offset at a byte, including the grapheme count and state.
  #[track_caller]
  pub(crate) fn grapheme_pos_at(&self, byte: usize) -> Pos {
    let mut cursor = self.tree.cursor::<Pos>();
    cursor.seek(&ByteOffset(byte), Bias::Left);

    let mut pos = *cursor.start();
    match cursor.item() {
      Some(chunk) => pos.add_text(&chunk.0[..byte - pos.bytes]),
      None => assert!(pos.bytes == byte, "byte index {byte} is out of bounds"),
    }
    pos
  }
}

// The rope answers queries by looking at all of itself.
impl Rope {
  pub fn char_len(&self) -> usize { self.as_slice().char_len() }
  pub fn utf16_len(&self) -> usize { self.as_slice().utf16_len() }
  pub fn grapheme_len(&self) -> usize { self.as_slice().grapheme_len() }
  pub fn line_len(&self) -> usize { self.as_slice().line_len() }

  #[track_caller]
  pub fn byte_of_line(&self, line: usize) -> usize { self.as_slice().byte_of_line(line) }
  #[track_caller]
  pub fn line_of_byte(&self, byte: usize) -> usize { self.as_slice().line_of_byte(byte) }
  #[track_caller]
  pub fn byte_of_grapheme(&self, index: usize) -> usize { self.as_slice().byte_of_grapheme(index) }
  #[track_caller]
  pub fn grapheme_of_byte(&self, byte: usize) -> usize { self.as_slice().grapheme_of_byte(byte) }
  #[track_caller]
  pub fn byte_of_utf16(&self, utf16: usize) -> usize { self.as_slice().byte_of_utf16(utf16) }
  #[track_caller]
  pub fn utf16_of_byte(&self, byte: usize) -> usize { self.as_slice().utf16_of_byte(byte) }

  pub fn is_char_boundary(&self, byte: usize) -> bool { self.as_slice().is_char_boundary(byte) }

  #[track_caller]
  pub fn byte_slice(&self, range: impl RangeBounds<usize>) -> RopeSlice<'_> {
    self.as_slice().byte_slice(range)
  }

  #[track_caller]
  pub fn line(&self, line: usize) -> RopeSlice<'_> { self.as_slice().line(line) }

  #[track_caller]
  pub fn line_slice(&self, range: impl RangeBounds<usize>) -> RopeSlice<'_> {
    self.as_slice().line_slice(range)
  }

  pub fn chunks(&self) -> Chunks<'_> { self.as_slice().chunks() }
  pub fn chars(&self) -> Chars<'_> { self.as_slice().chars() }
  pub fn bytes(&self) -> Bytes<'_> { self.as_slice().bytes() }
  pub fn graphemes(&self) -> Graphemes<'_> { self.as_slice().graphemes() }
  pub fn lines(&self) -> Lines<'_> { self.as_slice().lines() }
  pub fn raw_lines(&self) -> RawLines<'_> { self.as_slice().raw_lines() }
}

impl From<&str> for Rope {
  fn from(text: &str) -> Rope { Rope { tree: chunk_tree(text) } }
}

impl From<String> for Rope {
  fn from(text: String) -> Rope { Rope::from(text.as_str()) }
}

impl From<Cow<'_, str>> for Rope {
  fn from(text: Cow<'_, str>) -> Rope { Rope::from(&*text) }
}

impl From<RopeSlice<'_>> for Rope {
  fn from(slice: RopeSlice<'_>) -> Rope {
    let mut builder = RopeBuilder::new();
    for chunk in slice.chunks() {
      builder.append(chunk);
    }
    builder.build()
  }
}

impl fmt::Display for Rope {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.as_slice(), f)
  }
}

impl fmt::Debug for Rope {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Debug::fmt(&self.as_slice(), f) }
}

impl PartialEq for Rope {
  fn eq(&self, other: &Rope) -> bool { self.as_slice() == other.as_slice() }
}
impl Eq for Rope {}

impl PartialEq<str> for Rope {
  fn eq(&self, other: &str) -> bool { self.as_slice() == other }
}
impl PartialEq<&str> for Rope {
  fn eq(&self, other: &&str) -> bool { self.as_slice() == *other }
}
impl PartialEq<String> for Rope {
  fn eq(&self, other: &String) -> bool { self.as_slice() == other.as_str() }
}

/// Builds a [`Rope`] from text pushed in order, much faster than inserting it
/// piece by piece.
#[derive(Default)]
pub struct RopeBuilder {
  chunks:  Vec<Chunk>,
  /// Text not yet turned into a chunk. Always shorter than `PENDING_MAX`.
  pending: String,
}

/// A full chunk is cut off once at least this much is pending, which leaves at
/// least `MIN_CHUNK` behind.
const PENDING_MAX: usize = MAX_CHUNK + MIN_CHUNK;

impl RopeBuilder {
  pub fn new() -> RopeBuilder { RopeBuilder::default() }

  pub fn append(&mut self, mut text: &str) {
    while !text.is_empty() {
      let room = PENDING_MAX - self.pending.len();
      let mut n = room.min(text.len());
      while !text.is_char_boundary(n) {
        n -= 1;
      }
      if n == 0 {
        // Not even one character fits. Cut a chunk to make room.
        self.cut();
        continue;
      }

      self.pending.push_str(&text[..n]);
      text = &text[n..];
      if self.pending.len() >= PENDING_MAX {
        self.cut();
      }
    }
  }

  /// Turns the start of the pending text into a full chunk.
  fn cut(&mut self) {
    let mut at = self.pending.len().min(MAX_CHUNK);
    while !self.pending.is_char_boundary(at) {
      at -= 1;
    }
    self.chunks.push(Chunk(Arc::from(&self.pending[..at])));
    self.pending.drain(..at);
  }

  pub fn build(mut self) -> Rope {
    // The pending text is shorter than `PENDING_MAX`, so it is one chunk or
    // two even ones.
    for piece in split_even(&self.pending) {
      self.chunks.push(Chunk(Arc::from(piece)));
    }
    Rope { tree: tree_from_chunks(self.chunks) }
  }
}
