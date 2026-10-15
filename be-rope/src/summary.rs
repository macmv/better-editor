//! The summary cached in every tree node, and the dimensions derived from it.

use std::cmp::Ordering;

use be_sumtree::{Dimension, SeekTarget, Summary};

use crate::grapheme::{self, GraphemeMap, State};

/// What a piece of text adds up to.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct TextSummary {
  pub bytes:     usize,
  pub chars:     usize,
  /// The length in UTF-16 code units.
  pub utf16:     usize,
  /// The number of `\n` bytes.
  pub newlines:  usize,
  pub graphemes: GraphemeMap,
}

impl TextSummary {
  pub fn of(text: &str) -> TextSummary {
    // Most text is simple ASCII, where everything but the newline count is
    // known from the length.
    let simple =
      text.as_bytes().iter().all(|&b| (0x20..0x7f).contains(&b) || b == b'\n' || b == b'\t');
    if simple && !text.is_empty() {
      let newlines = text.as_bytes().iter().filter(|&&b| b == b'\n').count();
      return TextSummary {
        bytes: text.len(),
        chars: text.len(),
        utf16: text.len(),
        newlines,
        graphemes: GraphemeMap::of_simple_ascii(text),
      };
    }

    let Counts { chars, utf16, newlines } = Counts::of(text);
    TextSummary { bytes: text.len(), chars, utf16, newlines, graphemes: GraphemeMap::of(text) }
  }
}

impl Summary for TextSummary {
  fn add_summary(&mut self, other: &Self) {
    self.bytes += other.bytes;
    self.chars += other.chars;
    self.utf16 += other.utf16;
    self.newlines += other.newlines;
    self.graphemes = self.graphemes.then(&other.graphemes);
  }
}

/// The additive parts of a summary.
struct Counts {
  chars:    usize,
  utf16:    usize,
  newlines: usize,
}

impl Counts {
  fn of(text: &str) -> Counts {
    let mut chars = 0;
    let mut utf16 = 0;
    let mut newlines = 0;
    for &b in text.as_bytes() {
      // Count each character once, at its first byte. Characters outside the
      // basic plane (4 bytes, so a first byte of at least 0xF0) are two UTF-16
      // units.
      let first = (b & 0xc0) != 0x80;
      chars += first as usize;
      utf16 += first as usize + (b >= 0xf0) as usize;
      newlines += (b == b'\n') as usize;
    }
    Counts { chars, utf16, newlines }
  }
}

/// A position in the text, as every offset except graphemes. This is all that
/// most seeks need, and it is much cheaper to accumulate than a [`Pos`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Offsets {
  pub bytes:    usize,
  pub chars:    usize,
  pub utf16:    usize,
  pub newlines: usize,
}

impl Dimension<TextSummary> for Offsets {
  #[inline]
  fn add_summary(&mut self, s: &TextSummary) {
    self.bytes += s.bytes;
    self.chars += s.chars;
    self.utf16 += s.utf16;
    self.newlines += s.newlines;
  }
}

impl Offsets {
  /// Advances over `text`.
  pub fn add_text(&mut self, text: &str) {
    let Counts { chars, utf16, newlines } = Counts::of(text);
    self.bytes += text.len();
    self.chars += chars;
    self.utf16 += utf16;
    self.newlines += newlines;
  }
}

/// A position in the text, as every kind of offset at once, including
/// graphemes. Tracking graphemes costs extra at every step of a seek, so use
/// [`Offsets`] unless graphemes are needed.
///
/// The grapheme fields describe the text from the start of the whole rope, so
/// `graphemes` is the number of clusters begun before this position and
/// `state` is where the segmenter is.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Pos {
  pub bytes:     usize,
  pub chars:     usize,
  pub utf16:     usize,
  pub newlines:  usize,
  pub state:     State,
  pub graphemes: usize,
}

impl Dimension<TextSummary> for Pos {
  fn add_summary(&mut self, s: &TextSummary) {
    self.bytes += s.bytes;
    self.chars += s.chars;
    self.utf16 += s.utf16;
    self.newlines += s.newlines;
    let (state, count) = s.graphemes.apply(self.state);
    self.state = state;
    self.graphemes += count;
  }
}

impl Pos {
  /// Advances over `text`.
  pub fn add_text(&mut self, text: &str) {
    let Counts { chars, utf16, newlines } = Counts::of(text);
    self.bytes += text.len();
    self.chars += chars;
    self.utf16 += utf16;
    self.newlines += newlines;
    let (state, count) = grapheme::advance(self.state, text);
    self.state = state;
    self.graphemes += count;
  }
}

macro_rules! target {
  ($(#[$meta:meta])* $name:ident, $field:ident) => {
    target!(@define $(#[$meta])* $name, $field);

    impl SeekTarget<Offsets> for $name {
      #[inline]
      fn seek_cmp(&self, location: &Offsets) -> Ordering { self.0.cmp(&location.$field) }
    }
  };
  (@define $(#[$meta:meta])* $name:ident, $field:ident) => {
    $(#[$meta])*
    #[derive(Clone, Copy, Debug)]
    pub struct $name(pub usize);

    impl SeekTarget<Pos> for $name {
      #[inline]
      fn seek_cmp(&self, location: &Pos) -> Ordering { self.0.cmp(&location.$field) }
    }
  };
}

target!(
  /// A byte offset.
  ByteOffset,
  bytes
);
target!(
  /// A count of `\n` bytes.
  NewlineCount,
  newlines
);
target!(
  /// An offset in UTF-16 code units.
  Utf16Offset,
  utf16
);
// Graphemes are only tracked by `Pos`.
target!(
  @define
  /// A count of grapheme clusters begun.
  GraphemeCount,
  graphemes
);
