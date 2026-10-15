use be_rope::{Find, RFind};

use crate::Document;

pub struct FindIter<'a> {
  needle: &'a str,
  /// Added to each match, as the search runs on a sub-slice of the document.
  base:   usize,
  inner:  FindIterImpl<'a>,
}

enum FindIterImpl<'a> {
  Empty,
  Forward(Find<'a, 'a>),
  Reverse(RFind<'a, 'a>),
}

impl Document {
  pub fn find<'a>(&'a self, pattern: &'a str) -> FindIter<'a> { self.find_from(0, pattern) }
  pub fn rfind<'a>(&'a self, pattern: &'a str) -> FindIter<'a> {
    self.rfind_from(self.rope.byte_len(), pattern)
  }

  pub fn find_from<'a>(&'a self, start: usize, pattern: &'a str) -> FindIter<'a> {
    let inner = if pattern.is_empty() {
      FindIterImpl::Empty
    } else {
      FindIterImpl::Forward(self.rope.byte_slice(start..).find(pattern))
    };
    FindIter { needle: pattern, base: start, inner }
  }

  pub fn rfind_from<'a>(&'a self, start: usize, pattern: &'a str) -> FindIter<'a> {
    let inner = if pattern.is_empty() {
      FindIterImpl::Empty
    } else {
      FindIterImpl::Reverse(self.rope.byte_slice(..start).rfind(pattern))
    };
    FindIter { needle: pattern, base: 0, inner }
  }
}

impl<'a> FindIter<'a> {
  pub fn needle(&self) -> &'a str { self.needle }
}

impl Iterator for FindIter<'_> {
  type Item = usize;

  fn next(&mut self) -> Option<usize> {
    match &mut self.inner {
      FindIterImpl::Empty => None,
      FindIterImpl::Forward(it) => it.next(),
      FindIterImpl::Reverse(it) => it.next(),
    }
    .map(|i| i + self.base)
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
  fn find_from_offsets() {
    let doc = Document::from("foo bar baz ooo quoox");

    assert_eq!(doc.find_from(2, "oo").collect::<Vec<_>>(), &[12, 18]);
  }

  #[test]
  fn rfind_works() {
    let doc = Document::from("foo bar baz ooo quoox");
    assert_eq!(doc.rfind("oo").collect::<Vec<_>>(), &[18, 13, 1]);

    let doc = Document::from("fob bar baz obo quobx");
    assert_eq!(doc.rfind("ob").collect::<Vec<_>>(), &[18, 12, 1]);
  }
}
