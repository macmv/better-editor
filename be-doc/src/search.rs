use crop::{Rope, RopeSlice};

use crate::Document;
use std::{
  cmp,
  ops::{Index, RangeBounds},
};

pub struct FindIter<'a>(FindIterImpl<'a>);

enum FindIterImpl<'a> {
  Empty,
  TwoWay { rope: &'a Rope, offset: usize, two_way: TwoWay<'a>, reversed: bool },
}

struct RopeAccess<'a> {
  slice:    RopeSlice<'a>,
  reversed: bool,
}

#[derive(Clone, Copy, Debug)]
struct ByteAccess<'a> {
  str:      &'a str,
  reversed: bool,
}

impl Document {
  pub fn find<'a>(&'a self, pattern: &'a str) -> FindIter<'a> { self.find_from(0, pattern) }
  pub fn rfind<'a>(&'a self, pattern: &'a str) -> FindIter<'a> {
    self.rfind_from(self.rope.byte_len(), pattern)
  }

  pub fn find_from<'a>(&'a self, start: usize, pattern: &'a str) -> FindIter<'a> {
    if pattern.is_empty() {
      FindIter(FindIterImpl::Empty)
    } else {
      FindIter(FindIterImpl::TwoWay {
        rope:     &self.rope,
        offset:   start,
        two_way:  TwoWay::new(ByteAccess { str: pattern, reversed: false }),
        reversed: false,
      })
    }
  }

  pub fn rfind_from<'a>(&'a self, start: usize, pattern: &'a str) -> FindIter<'a> {
    if pattern.is_empty() {
      FindIter(FindIterImpl::Empty)
    } else {
      FindIter(FindIterImpl::TwoWay {
        rope:     &self.rope,
        offset:   start,
        two_way:  TwoWay::new(ByteAccess { str: pattern, reversed: true }),
        reversed: true,
      })
    }
  }
}

impl<'a> FindIter<'a> {
  pub fn needle(&self) -> &'a str {
    match self {
      FindIter(FindIterImpl::Empty) => "",
      FindIter(FindIterImpl::TwoWay { two_way, .. }) => two_way.needle.str,
    }
  }
}

impl Iterator for FindIter<'_> {
  type Item = usize;

  fn next(&mut self) -> Option<Self::Item> {
    match *self {
      FindIter(FindIterImpl::Empty) => None,
      FindIter(FindIterImpl::TwoWay { rope, ref mut offset, two_way, reversed }) => {
        let haystack = RopeAccess {
          slice: if reversed { rope.byte_slice(..*offset) } else { rope.byte_slice(*offset..) },
          reversed,
        };

        if let Some(advance) = two_way.find_in(haystack) {
          if reversed {
            *offset -= advance + two_way.needle.len();
            Some(*offset)
          } else {
            let ret = *offset + advance;
            *offset += advance + two_way.needle.len();
            Some(ret)
          }
        } else {
          None
        }
      }
    }
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    match self {
      FindIter(FindIterImpl::Empty) => (0, Some(0)),
      FindIter(FindIterImpl::TwoWay { .. }) => (0, None),
    }
  }
}

impl RopeAccess<'_> {
  fn byte(&self, pos: usize) -> u8 {
    if self.reversed {
      self.slice.byte(self.slice.byte_len() - pos - 1)
    } else {
      self.slice.byte(pos)
    }
  }

  fn byte_len(&self) -> usize { self.slice.byte_len() }
}

impl ByteAccess<'_> {
  fn len(&self) -> usize { self.str.len() }

  fn split_at(&self, critical_pos: usize) -> (ByteAccess<'_>, ByteAccess<'_>) {
    if self.reversed {
      (
        ByteAccess { str: &self.str[self.len() - critical_pos..], reversed: true },
        ByteAccess { str: &self.str[..self.len() - critical_pos], reversed: true },
      )
    } else {
      (
        ByteAccess { str: &self.str[..critical_pos], reversed: false },
        ByteAccess { str: &self.str[critical_pos..], reversed: false },
      )
    }
  }

  fn range(&self, range: impl RangeBounds<usize>) -> ByteAccess<'_> {
    if self.reversed {
      let start = match range.start_bound() {
        std::ops::Bound::Included(&n) => self.len() - n,
        std::ops::Bound::Excluded(&n) => self.len() - n - 1,
        std::ops::Bound::Unbounded => self.len(),
      };
      let end = match range.end_bound() {
        std::ops::Bound::Included(&n) => self.len() - n - 1,
        std::ops::Bound::Excluded(&n) => self.len() - n,
        std::ops::Bound::Unbounded => 0,
      };

      ByteAccess { str: &self.str[end..start], reversed: true }
    } else {
      let start = match range.start_bound() {
        std::ops::Bound::Included(&n) => n,
        std::ops::Bound::Excluded(&n) => n + 1,
        std::ops::Bound::Unbounded => 0,
      };
      let end = match range.end_bound() {
        std::ops::Bound::Included(&n) => n + 1,
        std::ops::Bound::Excluded(&n) => n,
        std::ops::Bound::Unbounded => self.str.len(),
      };

      ByteAccess { str: &self.str[start..end], reversed: false }
    }
  }

  #[cfg(test)]
  fn rev(&self) -> ByteAccess<'_> { ByteAccess { str: self.str, reversed: !self.reversed } }
}

impl Index<usize> for ByteAccess<'_> {
  type Output = u8;
  fn index(&self, index: usize) -> &Self::Output {
    if self.reversed {
      &self.str.as_bytes()[self.str.len() - index - 1]
    } else {
      &self.str.as_bytes()[index]
    }
  }
}

impl PartialEq<&str> for ByteAccess<'_> {
  fn eq(&self, other: &&str) -> bool {
    if self.reversed {
      self.str.len() == other.len() && self.str.bytes().rev().eq(other.bytes())
    } else {
      self.str == *other
    }
  }
}

impl PartialEq for ByteAccess<'_> {
  fn eq(&self, other: &Self) -> bool {
    if self.reversed == other.reversed {
      self.str == other.str
    } else {
      self.len() == other.len() && self.str.bytes().rev().eq(other.str.bytes())
    }
  }
}

#[derive(Clone, Copy, Debug)]
struct TwoWay<'a> {
  needle:       ByteAccess<'a>,
  critical_pos: usize,
  shift:        Shift,
}

#[derive(Clone, Copy, Debug)]
enum SuffixKind {
  Minimal,
  Maximal,
}

#[derive(Clone, Copy, Debug)]
enum SuffixOrdering {
  Accept,
  Skip,
  Push,
}

#[derive(Debug)]
struct Suffix {
  pos:    usize,
  period: usize,
}

#[derive(Clone, Copy, Debug)]
enum Shift {
  Small { period: usize },
  Large { shift: usize },
}

fn is_suffix(s: ByteAccess, suffix: ByteAccess) -> bool {
  suffix.len() <= s.len() && s.range(s.len() - suffix.len()..) == suffix
}

impl<'a> TwoWay<'a> {
  fn new(needle: ByteAccess<'a>) -> Self {
    let min_suffix = Suffix::forward(needle, SuffixKind::Minimal);
    let max_suffix = Suffix::forward(needle, SuffixKind::Maximal);

    let (period_lower_bound, critical_pos) = if min_suffix.pos > max_suffix.pos {
      (min_suffix.period, min_suffix.pos)
    } else {
      (max_suffix.period, max_suffix.pos)
    };

    let shift = Shift::forward(needle, period_lower_bound, critical_pos);
    TwoWay { needle, critical_pos, shift }
  }

  fn find_in(&self, haystack: RopeAccess) -> Option<usize> {
    match self.shift {
      Shift::Small { period } => self.find_small(haystack, period),
      Shift::Large { shift } => self.find_large(haystack, shift),
    }
  }

  // "Small period" (periodic) case.
  fn find_small(&self, haystack: RopeAccess, period: usize) -> Option<usize> {
    let mut pos = 0usize;
    let mut mem = 0usize; // called `shift` in some references: how much of the left part we can skip

    while pos + self.needle.len() <= haystack.byte_len() {
      let mut i = cmp::max(self.critical_pos, mem);
      while i < self.needle.len() && self.needle[i] == haystack.byte(pos + i) {
        i += 1;
      }

      if i < self.needle.len() {
        // mismatch in right half
        pos += i - self.critical_pos + 1;
        mem = 0;
        continue;
      }

      // right half matched; verify left half backwards
      let mut j = self.critical_pos;
      while j > mem && self.needle[j] == haystack.byte(pos + j) {
        j -= 1;
      }
      if j <= mem && self.needle[mem] == haystack.byte(pos + mem) {
        return Some(pos);
      }

      // shift by period and remember overlap
      pos += period;
      mem = self.needle.len() - period;
    }
    None
  }

  // "Large period" (non-periodic / fallback) case.
  fn find_large(&self, haystack: RopeAccess, shift: usize) -> Option<usize> {
    let mut pos = 0usize;

    'outer: while pos + self.needle.len() <= haystack.byte_len() {
      // scan right half forward
      let mut i = self.critical_pos;
      while i < self.needle.len() && self.needle[i] == haystack.byte(pos + i) {
        i += 1;
      }
      if i < self.needle.len() {
        pos += i - self.critical_pos + 1;
        continue;
      }

      // verify left half backwards
      for j in (0..self.critical_pos).rev() {
        if self.needle[j] != haystack.byte(pos + j) {
          pos += shift;
          continue 'outer;
        }
      }
      return Some(pos);
    }
    None
  }
}

impl Shift {
  fn forward(needle: ByteAccess, period_lower_bound: usize, critical_pos: usize) -> Shift {
    let large = cmp::max(critical_pos, needle.len() - critical_pos);

    // If the critical factorization is too far right, just use the large shift.
    if critical_pos * 2 >= needle.len() {
      return Shift::Large { shift: large };
    }

    // Check the "small period" condition:
    // u = needle[..critical_pos], v = needle[critical_pos..]
    let (u, v) = needle.split_at(critical_pos);
    if !is_suffix(v.range(..period_lower_bound), u) {
      return Shift::Large { shift: large };
    }

    Shift::Small { period: period_lower_bound }
  }
}

impl Suffix {
  fn forward(needle: ByteAccess, kind: SuffixKind) -> Suffix {
    let mut suffix = Suffix { pos: 0, period: 1 };
    let mut candidate_start = 1usize;
    let mut offset = 0usize;

    while candidate_start + offset < needle.len() {
      let current = needle[suffix.pos + offset];
      let candidate = needle[candidate_start + offset];

      match kind.cmp(current, candidate) {
        SuffixOrdering::Accept => {
          suffix = Suffix { pos: candidate_start, period: 1 };
          candidate_start += 1;
          offset = 0;
        }
        SuffixOrdering::Skip => {
          candidate_start += offset + 1;
          offset = 0;
          suffix.period = candidate_start - suffix.pos;
        }
        SuffixOrdering::Push => {
          if offset + 1 == suffix.period {
            candidate_start += suffix.period;
            offset = 0;
          } else {
            offset += 1;
          }
        }
      }
    }
    suffix
  }
}

impl SuffixKind {
  fn cmp(self, current: u8, candidate: u8) -> SuffixOrdering {
    use SuffixOrdering::*;
    match self {
      SuffixKind::Minimal if candidate < current => Accept,
      SuffixKind::Minimal if candidate > current => Skip,
      SuffixKind::Minimal => Push,
      SuffixKind::Maximal if candidate > current => Accept,
      SuffixKind::Maximal if candidate < current => Skip,
      SuffixKind::Maximal => Push,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn find_works() {
    let doc = Document::from("foo bar baz ooo quoox");

    assert_eq!(doc.find("oo").collect::<Vec<_>>(), &[1, 12, 18]);
  }

  #[test]
  fn find_nothing_for_empty() {
    let doc = Document::from("foo bar baz ooo quoox");

    assert_eq!(doc.find("").collect::<Vec<_>>(), &[]);
  }

  #[test]
  fn rfind_works() {
    let doc = Document::from("foo bar baz ooo quoox");
    assert_eq!(doc.rfind("oo").collect::<Vec<_>>(), &[18, 13, 1]);

    let doc = Document::from("fob bar baz obo quobx");
    assert_eq!(doc.rfind("ob").collect::<Vec<_>>(), &[18, 12, 1]);
  }

  #[test]
  fn byte_access_works() {
    let acc = ByteAccess { str: "hello", reversed: false };
    assert_eq!(acc.len(), 5);
    assert_eq!(acc, "hello");

    assert_eq!(acc.rev().len(), 5);
    assert_eq!(acc.rev(), "olleh");
  }

  #[test]
  fn byte_access_range() {
    let acc = ByteAccess { str: "hello", reversed: false };
    assert_eq!(acc.range(1..3), "el");
    assert_eq!(acc.rev().range(..3), "oll");

    let acc = ByteAccess { str: "hello", reversed: false };
    assert_eq!(acc.range(1..=3), "ell");
    assert_eq!(acc.rev().range(..=3), "olle");
  }

  #[test]
  fn byte_access_split() {
    let acc = ByteAccess { str: "hello", reversed: false };
    assert_eq!(acc.split_at(2).0, "he");
    assert_eq!(acc.split_at(2).1, "llo");

    assert_eq!(acc.rev().split_at(1).0, "o");
    assert_eq!(acc.rev().split_at(1).1, "lleh");
    assert_eq!(acc.rev().split_at(2).0, "ol");
    assert_eq!(acc.rev().split_at(2).1, "leh");
  }
}
