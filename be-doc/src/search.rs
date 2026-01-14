use crop::{Rope, RopeSlice};

use crate::Document;
use std::cmp;

pub struct FindIter<'a>(FindIterImpl<'a>);

enum FindIterImpl<'a> {
  Empty,
  TwoWay { rope: &'a Rope, offset: usize, two_way: TwoWay<'a> },
}

impl Document {
  pub fn find<'a>(&'a self, pattern: &'a str) -> FindIter<'a> { self.find_from(0, pattern) }

  pub fn find_from<'a>(&'a self, start: usize, pattern: &'a str) -> FindIter<'a> {
    if pattern.is_empty() {
      FindIter(FindIterImpl::Empty)
    } else {
      FindIter(FindIterImpl::TwoWay {
        rope:    &self.rope,
        offset:  start,
        two_way: TwoWay::new(pattern),
      })
    }
  }
}

impl<'a> FindIter<'a> {
  pub fn needle(&self) -> &'a str {
    match self {
      FindIter(FindIterImpl::Empty) => "",
      FindIter(FindIterImpl::TwoWay { two_way, .. }) => two_way.needle,
    }
  }
}

impl Iterator for FindIter<'_> {
  type Item = usize;

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      FindIter(FindIterImpl::Empty) => None,
      FindIter(FindIterImpl::TwoWay { rope, offset, two_way }) => {
        if let Some(advance) = two_way.find_in(rope.byte_slice(*offset..)) {
          let ret = *offset + advance;
          *offset += advance + two_way.needle.len();
          Some(ret)
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

#[derive(Clone, Copy, Debug)]
struct TwoWay<'a> {
  needle:       &'a str,
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

fn is_suffix(s: &[u8], suffix: &[u8]) -> bool {
  suffix.len() <= s.len() && &s[s.len() - suffix.len()..] == suffix
}

impl<'a> TwoWay<'a> {
  fn new(needle: &'a str) -> Self {
    let min_suffix = Suffix::forward(needle.as_bytes(), SuffixKind::Minimal);
    let max_suffix = Suffix::forward(needle.as_bytes(), SuffixKind::Maximal);

    let (period_lower_bound, critical_pos) = if min_suffix.pos > max_suffix.pos {
      (min_suffix.period, min_suffix.pos)
    } else {
      (max_suffix.period, max_suffix.pos)
    };

    let shift = Shift::forward(needle.as_bytes(), period_lower_bound, critical_pos);
    TwoWay { needle, critical_pos, shift }
  }

  fn find_in(&self, haystack: RopeSlice<'_>) -> Option<usize> {
    match self.shift {
      Shift::Small { period } => self.find_small(haystack, period),
      Shift::Large { shift } => self.find_large(haystack, shift),
    }
  }

  // "Small period" (periodic) case.
  fn find_small(&self, haystack: RopeSlice<'_>, period: usize) -> Option<usize> {
    let mut pos = 0usize;
    let mut mem = 0usize; // called `shift` in some references: how much of the left part we can skip

    while pos + self.needle.len() <= haystack.byte_len() {
      let mut i = cmp::max(self.critical_pos, mem);
      while i < self.needle.len() && self.needle.as_bytes()[i] == haystack.byte(pos + i) {
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
      while j > mem && self.needle.as_bytes()[j] == haystack.byte(pos + j) {
        j -= 1;
      }
      if j <= mem && self.needle.as_bytes()[mem] == haystack.byte(pos + mem) {
        return Some(pos);
      }

      // shift by period and remember overlap
      pos += period;
      mem = self.needle.len() - period;
    }
    None
  }

  // "Large period" (non-periodic / fallback) case.
  fn find_large(&self, haystack: RopeSlice, shift: usize) -> Option<usize> {
    let mut pos = 0usize;

    'outer: while pos + self.needle.len() <= haystack.byte_len() {
      // scan right half forward
      let mut i = self.critical_pos;
      while i < self.needle.len() && self.needle.as_bytes()[i] == haystack.byte(pos + i) {
        i += 1;
      }
      if i < self.needle.len() {
        pos += i - self.critical_pos + 1;
        continue;
      }

      // verify left half backwards
      for j in (0..self.critical_pos).rev() {
        if self.needle.as_bytes()[j] != haystack.byte(pos + j) {
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
  fn forward(needle: &[u8], period_lower_bound: usize, critical_pos: usize) -> Shift {
    let large = cmp::max(critical_pos, needle.len() - critical_pos);

    // If the critical factorization is too far right, just use the large shift.
    if critical_pos * 2 >= needle.len() {
      return Shift::Large { shift: large };
    }

    // Check the "small period" condition:
    // u = needle[..critical_pos], v = needle[critical_pos..]
    let (u, v) = needle.split_at(critical_pos);
    if !is_suffix(&v[..period_lower_bound], u) {
      return Shift::Large { shift: large };
    }

    Shift::Small { period: period_lower_bound }
  }
}

impl Suffix {
  fn forward(needle: &[u8], kind: SuffixKind) -> Suffix {
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
}
