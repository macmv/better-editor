use std::{
  fmt,
  ops::{Bound, RangeBounds},
};

use be_sumtree::Bias;

use crate::{
  Find, RFind, Rope,
  grapheme::{self, State},
  iter::{Bytes, Chars, Chunks, Graphemes, Lines, RawLines},
  summary::{GraphemeCount, NewlineCount, Offsets, Pos, Utf16Offset},
};

/// A borrowed piece of a [`Rope`]. Cheap to copy.
///
/// A slice is its own text: line and grapheme boundaries are found as if the
/// slice were all there is, so a slice that begins in the middle of a grapheme
/// starts a new one there.
#[derive(Clone, Copy)]
pub struct RopeSlice<'a> {
  pub(crate) rope:  &'a Rope,
  /// Absolute byte offsets into the rope.
  pub(crate) start: usize,
  pub(crate) end:   usize,
}

impl<'a> RopeSlice<'a> {
  pub fn byte_len(&self) -> usize { self.end - self.start }

  pub fn is_empty(&self) -> bool { self.start == self.end }

  pub fn is_char_boundary(&self, byte: usize) -> bool {
    if byte == 0 || byte == self.byte_len() {
      true
    } else if byte > self.byte_len() {
      false
    } else {
      // Bytes in the middle of a character look like `10xxxxxx`.
      self.byte_at(byte) & 0xc0 != 0x80
    }
  }

  /// The byte at a position in the slice.
  #[track_caller]
  pub(crate) fn byte_at(&self, byte: usize) -> u8 {
    assert!(byte < self.byte_len(), "byte index {byte} is out of bounds");
    let abs = self.start + byte;
    let mut cursor = self.rope.tree.cursor::<Offsets>();
    cursor.seek(&crate::summary::ByteOffset(abs), Bias::Right);
    let chunk = cursor.item().expect("byte is in bounds");
    chunk.0.as_bytes()[abs - cursor.start().bytes]
  }

  /// A sub-slice between two offsets that are known to be valid.
  pub(crate) fn sub(&self, start: usize, end: usize) -> RopeSlice<'a> {
    debug_assert!(start <= end && end <= self.byte_len());
    RopeSlice { rope: self.rope, start: self.start + start, end: self.start + end }
  }

  /// A sub-slice. Offsets are relative to this slice.
  #[track_caller]
  pub fn byte_slice(&self, range: impl RangeBounds<usize>) -> RopeSlice<'a> {
    let len = self.byte_len();
    let start = match range.start_bound() {
      Bound::Included(&s) => s,
      Bound::Excluded(&s) => s + 1,
      Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
      Bound::Included(&e) => e + 1,
      Bound::Excluded(&e) => e,
      Bound::Unbounded => len,
    };
    assert!(start <= end, "range start {start} is after its end {end}");
    assert!(end <= len, "range end {end} is out of bounds (length {len})");
    assert!(self.is_char_boundary(start), "byte index {start} is not a char boundary");
    assert!(self.is_char_boundary(end), "byte index {end} is not a char boundary");
    RopeSlice { rope: self.rope, start: self.start + start, end: self.start + end }
  }

  pub fn chunks(&self) -> Chunks<'a> { Chunks::new(*self) }
  pub fn chars(&self) -> Chars<'a> { Chars::new(*self) }
  pub fn bytes(&self) -> Bytes<'a> { Bytes::new(*self) }
  pub fn graphemes(&self) -> Graphemes<'a> { Graphemes::new(*self) }
  pub fn raw_lines(&self) -> RawLines<'a> { RawLines::new(*self) }
  pub fn lines(&self) -> Lines<'a> { Lines::new(*self) }

  /// The byte offsets, relative to this slice, of the non-overlapping matches
  /// of `pattern`, front to back. An empty pattern matches nothing.
  pub fn find<'p>(&self, pattern: &'p str) -> Find<'a, 'p> { Find::new(*self, pattern) }

  /// Like [`find`](Self::find), but back to front, and the matches don't
  /// overlap when counted from the end.
  pub fn rfind<'p>(&self, pattern: &'p str) -> RFind<'a, 'p> { RFind::new(*self, pattern) }

  /// The additive offsets (everything but graphemes) of the start and end.
  fn bounds(&self) -> (Offsets, Offsets) {
    (self.rope.offsets_at(self.start), self.rope.offsets_at(self.end))
  }

  pub fn char_len(&self) -> usize {
    let (a, b) = self.bounds();
    b.chars - a.chars
  }

  /// The length in UTF-16 code units.
  pub fn utf16_len(&self) -> usize {
    let (a, b) = self.bounds();
    b.utf16 - a.utf16
  }

  /// The UTF-16 offset of a byte.
  #[track_caller]
  pub fn utf16_of_byte(&self, byte: usize) -> usize { self.byte_slice(..byte).utf16_len() }

  /// The byte of a UTF-16 offset. An offset in the middle of a surrogate pair
  /// gives the byte the character starts at.
  #[track_caller]
  pub fn byte_of_utf16(&self, utf16: usize) -> usize {
    let (a, b) = self.bounds();
    let len = b.utf16 - a.utf16;
    assert!(utf16 <= len, "UTF-16 offset {utf16} is out of bounds (length {len})");
    if utf16 == len {
      return self.byte_len();
    }

    let target = a.utf16 + utf16;
    let mut cursor = self.rope.tree.cursor::<Offsets>();
    cursor.seek(&Utf16Offset(target), Bias::Right);
    let chunk = cursor.item().expect("offset is in bounds");

    let mut at = cursor.start().utf16;
    for (i, c) in chunk.0.char_indices() {
      at += c.len_utf16();
      if at > target {
        return cursor.start().bytes + i - self.start;
      }
    }
    unreachable!("the chunk reaches the target")
  }

  // Lines.
  //
  // A line ends at a `\n`, or at the end of the text. The line break is `\n` or
  // `\r\n`. Text ending in a line break does not have an empty last line.

  /// The number of newlines before the slice, in the rope, the number in the
  /// slice, and the number of lines.
  fn line_info(&self) -> (usize, usize, usize) {
    let (a, b) = self.bounds();
    let newlines = b.newlines - a.newlines;
    let lines = if self.is_empty() {
      0
    } else if self.byte_at(self.byte_len() - 1) == b'\n' {
      newlines
    } else {
      newlines + 1
    };
    (a.newlines, newlines, lines)
  }

  pub fn line_len(&self) -> usize { self.line_info().2 }

  /// The byte a line starts at. `line_len()` is allowed, and gives the length.
  #[track_caller]
  pub fn byte_of_line(&self, line: usize) -> usize {
    let (before, _, lines) = self.line_info();
    assert!(line <= lines, "line index {line} is out of bounds (lines: {lines})");
    if line == 0 {
      return 0;
    }
    if line == lines {
      return self.byte_len();
    }

    // The start of the line is just after the `line`th newline of the slice.
    let target = before + line;
    let mut cursor = self.rope.tree.cursor::<Offsets>();
    cursor.seek(&NewlineCount(target), Bias::Left);
    let chunk = cursor.item().expect("the newline exists");

    let nth = target - cursor.start().newlines;
    let index = chunk
      .0
      .bytes()
      .enumerate()
      .filter(|&(_, b)| b == b'\n')
      .nth(nth - 1)
      .expect("the chunk holds the newline")
      .0;
    cursor.start().bytes + index + 1 - self.start
  }

  /// The line a byte is on. The end of the slice is on the line after the last
  /// one if the text ends with a line break.
  #[track_caller]
  pub fn line_of_byte(&self, byte: usize) -> usize {
    assert!(byte <= self.byte_len(), "byte index {byte} is out of bounds");
    assert!(self.is_char_boundary(byte), "byte index {byte} is not a char boundary");
    self.rope.offsets_at(self.start + byte).newlines - self.rope.offsets_at(self.start).newlines
  }

  /// A line, without its line break.
  #[track_caller]
  pub fn line(&self, line: usize) -> RopeSlice<'a> {
    let lines = self.line_len();
    assert!(line < lines, "line index {line} is out of bounds (lines: {lines})");

    let start = self.byte_of_line(line);
    let mut end = self.byte_of_line(line + 1);
    if end > start && self.byte_at(end - 1) == b'\n' {
      end -= 1;
      if end > start && self.byte_at(end - 1) == b'\r' {
        end -= 1;
      }
    }
    self.sub(start, end)
  }

  /// A range of lines, with their line breaks.
  #[track_caller]
  pub fn line_slice(&self, range: impl RangeBounds<usize>) -> RopeSlice<'a> {
    let lines = self.line_len();
    let start = match range.start_bound() {
      Bound::Included(&s) => s,
      Bound::Excluded(&s) => s + 1,
      Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
      Bound::Included(&e) => e + 1,
      Bound::Excluded(&e) => e,
      Bound::Unbounded => lines,
    };
    assert!(start <= end, "line range start {start} is after its end {end}");
    assert!(end <= lines, "line range end {end} is out of bounds (lines: {lines})");
    self.byte_slice(self.byte_of_line(start)..self.byte_of_line(end))
  }

  // Graphemes.
  //
  // The rope's summaries segment the whole rope from its start. A slice is
  // segmented from its own start, which only differs from the rope's
  // segmentation near the start. So the start of the slice is walked until the
  // two agree, and the rest comes from the summaries.

  /// Walks the start of the slice until it is segmented the same as in the
  /// rope. If `find` is `Some(k)`, stops early if the `k`th cluster begins on
  /// the way.
  fn walk_head(&self, find: Option<usize>) -> Head {
    let global = self.rope.grapheme_pos_at(self.start);
    if global.state == State::START {
      return Head { converged: 0, count: 0, graphemes: global.graphemes, found: None };
    }

    let mut ours = State::START;
    let mut theirs = global.state;
    let mut count = 0;
    let mut graphemes = global.graphemes;
    let mut offset = 0;
    for chunk in self.chunks() {
      for (i, c) in chunk.char_indices() {
        let (next_ours, begins) = grapheme::step(ours, c);
        let (next_theirs, global_begins) = grapheme::step(theirs, c);
        if begins {
          if find == Some(count) {
            return Head { converged: 0, count, graphemes, found: Some(offset + i) };
          }
          count += 1;
        }
        graphemes += global_begins as usize;
        ours = next_ours;
        theirs = next_theirs;
        if ours == theirs {
          let converged = offset + i + c.len_utf8();
          return Head { converged, count, graphemes, found: None };
        }
      }
      offset += chunk.len();
    }

    Head { converged: self.byte_len(), count, graphemes, found: None }
  }

  /// The number of grapheme clusters.
  pub fn grapheme_len(&self) -> usize {
    let head = self.walk_head(None);
    if head.converged >= self.byte_len() {
      return head.count;
    }
    let end = self.rope.grapheme_pos_at(self.end).graphemes;
    head.count + (end - head.graphemes)
  }

  /// The byte the `index`th grapheme cluster starts at. `grapheme_len()` is
  /// allowed, and gives the length.
  #[track_caller]
  pub fn byte_of_grapheme(&self, index: usize) -> usize {
    let head = self.walk_head(Some(index));
    if let Some(found) = head.found {
      return found;
    }
    if head.converged >= self.byte_len() {
      assert!(
        index == head.count,
        "grapheme index {index} is out of bounds (graphemes: {})",
        head.count
      );
      return self.byte_len();
    }

    // The cluster begins at the `target`th begin event in the rope.
    let target = head.graphemes + (index - head.count) + 1;
    let mut cursor = self.rope.tree.cursor::<Pos>();
    cursor.seek(&GraphemeCount(target), Bias::Left);
    let Some(chunk) = cursor.item() else {
      assert!(index == self.grapheme_len(), "grapheme index {index} is out of bounds");
      return self.byte_len();
    };

    let mut state = cursor.start().state;
    let mut count = cursor.start().graphemes;
    for (i, c) in chunk.0.char_indices() {
      let (next, begins) = grapheme::step(state, c);
      state = next;
      if begins {
        count += 1;
        if count == target {
          let abs = cursor.start().bytes + i;
          if abs >= self.end {
            // The cluster is past the end of the slice.
            assert!(index == self.grapheme_len(), "grapheme index {index} is out of bounds");
            return self.byte_len();
          }
          return abs - self.start;
        }
      }
    }
    unreachable!("the chunk holds the begin event")
  }

  /// The index of the grapheme cluster a byte is in.
  #[track_caller]
  pub fn grapheme_of_byte(&self, byte: usize) -> usize { self.byte_slice(..byte).grapheme_len() }
}

/// See `RopeSlice::walk_head`.
struct Head {
  /// Where the slice's segmentation matches the rope's (relative to the slice).
  converged: usize,
  /// The clusters begun before `converged`, in the slice.
  count:     usize,
  /// The clusters begun before `converged`, in the rope.
  graphemes: usize,
  found:     Option<usize>,
}

impl fmt::Display for RopeSlice<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for chunk in self.chunks() {
      f.write_str(chunk)?;
    }
    Ok(())
  }
}

impl fmt::Debug for RopeSlice<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("\"")?;
    for chunk in self.chunks() {
      for c in chunk.chars() {
        fmt::Display::fmt(&c.escape_debug(), f)?;
      }
    }
    f.write_str("\"")
  }
}

/// Whether the chunks of two texts spell the same bytes.
fn eq_chunks<'a, 'b>(
  mut a: impl Iterator<Item = &'a str>,
  mut b: impl Iterator<Item = &'b str>,
) -> bool {
  let (mut x, mut y) = ("", "");
  loop {
    if x.is_empty() {
      x = a.next().unwrap_or("");
    }
    if y.is_empty() {
      y = b.next().unwrap_or("");
    }
    if x.is_empty() || y.is_empty() {
      return x.is_empty() && y.is_empty();
    }

    let n = x.len().min(y.len());
    if x.as_bytes()[..n] != y.as_bytes()[..n] {
      return false;
    }
    // Both sides are valid UTF-8 and equal so far, so `n` is a boundary in
    // both.
    x = &x[n..];
    y = &y[n..];
  }
}

impl PartialEq for RopeSlice<'_> {
  fn eq(&self, other: &RopeSlice<'_>) -> bool {
    self.byte_len() == other.byte_len() && eq_chunks(self.chunks(), other.chunks())
  }
}
impl Eq for RopeSlice<'_> {}

impl PartialEq<str> for RopeSlice<'_> {
  fn eq(&self, other: &str) -> bool {
    self.byte_len() == other.len() && eq_chunks(self.chunks(), std::iter::once(other))
  }
}
impl PartialEq<&str> for RopeSlice<'_> {
  fn eq(&self, other: &&str) -> bool { *self == **other }
}
impl PartialEq<String> for RopeSlice<'_> {
  fn eq(&self, other: &String) -> bool { *self == **other }
}
impl PartialEq<RopeSlice<'_>> for &str {
  fn eq(&self, other: &RopeSlice<'_>) -> bool { *other == **self }
}
impl PartialEq<RopeSlice<'_>> for str {
  fn eq(&self, other: &RopeSlice<'_>) -> bool { *other == *self }
}
impl PartialEq<Rope> for RopeSlice<'_> {
  fn eq(&self, other: &Rope) -> bool { *self == other.as_slice() }
}
