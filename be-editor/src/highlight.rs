use std::{
  cmp::Reverse,
  collections::{BTreeMap, BinaryHeap},
  ops::Range,
};

use crate::{EditorState, treesitter::CapturesIter};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Highlight<'a> {
  pub start: usize,
  pub end:   usize,
  pub key:   HighlightKey<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HighlightStack<'a> {
  pub pos:        usize,
  pub highlights: Vec<HighlightKey<'a>>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum HighlightKey<'a> {
  TreeSitter(&'a str),
  SemanticToken(&'a str),
}

enum HighlightIter<'a> {
  TreeSitter(CapturesIter<'a>),

  #[cfg(test)]
  Slice(std::slice::Iter<'a, Highlight<'a>>),
}

#[derive(PartialEq, Eq)]
struct StartNode<'a> {
  highlight: Highlight<'a>,
  src:       usize,
}

impl Ord for StartNode<'_> {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    self
      .highlight
      .start
      .cmp(&other.highlight.start)
      .then(self.src.cmp(&other.src))
      .then(self.highlight.end.cmp(&other.highlight.end))
      .then(self.highlight.key.cmp(&other.highlight.key))
  }
}
impl PartialOrd for StartNode<'_> {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

struct MergeIterator<'a> {
  iters: Vec<HighlightIter<'a>>,

  // min-heap of next-start spans across sources
  starts: BinaryHeap<Reverse<StartNode<'a>>>,

  // min-heap of active ends: (end_pos, key)
  ends: BinaryHeap<Reverse<(usize, HighlightKey<'a>)>>,

  // active key multiset (refcounted)
  active_counts: BTreeMap<HighlightKey<'a>, usize>,

  prev:    usize,
  base:    usize,
  started: bool,
}

impl EditorState {
  pub fn highlights(&self, range: Range<usize>) -> impl Iterator<Item = HighlightStack<'_>> {
    let mut iterators = vec![];

    if let Some(highlighter) = &self.highligher
      && let Some(highlights) = highlighter.highlights(&self.doc, range.clone())
    {
      iterators.push(HighlightIter::TreeSitter(highlights));
    }

    MergeIterator::new(iterators, range.start)
  }
}

impl<'a> Iterator for HighlightIter<'a> {
  type Item = Highlight<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      HighlightIter::TreeSitter(iter) => iter.next(),

      #[cfg(test)]
      HighlightIter::Slice(iter) => iter.next().copied(),
    }
  }
}

impl<'a> MergeIterator<'a> {
  fn new(mut sources: Vec<HighlightIter<'a>>, base: usize) -> Self {
    let mut starts = BinaryHeap::new();

    // Prime the heap with the first item from each iterator.
    for (src, it) in sources.iter_mut().enumerate() {
      if let Some(highlight) = it.next() {
        if highlight.start < highlight.end {
          starts.push(Reverse(StartNode { highlight, src }));
        }
      }
    }

    Self {
      iters: sources,
      starts,
      ends: BinaryHeap::new(),
      active_counts: BTreeMap::new(),
      prev: base,
      base,
      started: false,
    }
  }

  fn snapshot_active(&self) -> Vec<HighlightKey<'a>> {
    self.active_counts.keys().cloned().collect()
  }

  fn add_start(&mut self, highlight: Highlight<'a>) {
    if highlight.start >= highlight.end {
      return;
    }

    *self.active_counts.entry(highlight.key).or_insert(0) += 1;
    self.ends.push(Reverse((highlight.end, highlight.key)));
  }

  fn refill_src(&mut self, src: usize) {
    if let Some(highlight) = self.iters[src].next() {
      if highlight.start < highlight.end {
        self.starts.push(Reverse(StartNode { highlight, src }));
      }
    }
  }

  fn next_start_pos(&self) -> Option<usize> {
    self.starts.peek().map(|Reverse(n)| n.highlight.start)
  }

  fn next_end_pos(&self) -> Option<usize> { self.ends.peek().map(|Reverse((end, _))| *end) }

  fn apply_all_ends_at(&mut self, pos: usize) {
    while let Some(Reverse((end, _))) = self.ends.peek() {
      if *end != pos {
        break;
      }
      let Reverse((_end, key)) = self.ends.pop().unwrap();
      if let Some(c) = self.active_counts.get_mut(&key) {
        *c -= 1;
        if *c == 0 {
          self.active_counts.remove(&key);
        }
      }
    }
  }

  fn apply_all_starts_at(&mut self, pos: usize) {
    // Pop all starts with start == pos, and for each, pull the next span from that
    // source.
    while let Some(Reverse(n)) = self.starts.peek() {
      if n.highlight.start != pos {
        break;
      }
      let Reverse(n) = self.starts.pop().unwrap();
      let src = n.src;

      self.add_start(n.highlight);
      self.refill_src(src);
    }
  }
}

impl<'a> Iterator for MergeIterator<'a> {
  type Item = HighlightStack<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if !self.started {
      self.started = true;
      // Spans that start exactly at base should be active for [base, next_change).
      self.apply_all_starts_at(self.base);
    }

    let ns = self.next_start_pos();
    let ne = self.next_end_pos();

    if ns.is_none() && ne.is_none() {
      return None;
    }

    let next_pos = match (ns, ne) {
      (Some(s), Some(e)) => s.min(e),
      (Some(s), None) => s,
      (None, Some(e)) => e,
      (None, None) => unreachable!(),
    };

    // Emit segment [prev, next_pos) with current actives (before applying at
    // next_pos).
    if next_pos > self.prev {
      let out = HighlightStack { pos: next_pos, highlights: self.snapshot_active() };

      // Apply changes at next_pos for the following segment.
      // Ends first, then starts (so a span ending and starting at pos behaves
      // nicely).
      self.apply_all_ends_at(next_pos);
      self.apply_all_starts_at(next_pos);

      self.prev = next_pos;
      return Some(out);
    }

    // next_pos == prev: zero-length, just advance state and continue.
    self.apply_all_ends_at(next_pos);
    self.apply_all_starts_at(next_pos);
    self.prev = next_pos;
    self.next()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn merge_iter<'a>(iters: &'a [&[Highlight]]) -> MergeIterator<'a> {
    MergeIterator::new(iters.iter().map(|slice| HighlightIter::Slice(slice.iter())).collect(), 0)
  }

  const fn hl(range: Range<usize>, key: &str) -> Highlight<'_> {
    Highlight { start: range.start, end: range.end, key: HighlightKey::TreeSitter(key) }
  }

  fn stack(pos: usize, keys: impl IntoIterator<Item = &'static str>) -> HighlightStack<'static> {
    HighlightStack {
      pos,
      highlights: keys.into_iter().map(|s| HighlightKey::TreeSitter(s)).collect(),
    }
  }

  #[test]
  fn merge_iterator_works() {
    let highlights: &[&[Highlight]] = &[&[hl(0..3, "long"), hl(1..2, "a"), hl(2..4, "b")]];
    let iter = merge_iter(highlights);

    assert_eq!(
      iter.collect::<Vec<HighlightStack>>(),
      &[stack(1, ["long"]), stack(2, ["a", "long"]), stack(3, ["b", "long"]), stack(4, ["b"])],
    );
  }
}
