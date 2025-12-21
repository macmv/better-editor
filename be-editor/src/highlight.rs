use std::ops::Range;

use crate::{EditorState, treesitter::CapturesIter};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Highlight<'a> {
  pub start: usize,
  pub end:   usize,
  pub key:   HighlightKey<'a>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HighlightKey<'a> {
  TreeSitter(&'a str),
  SemanticToken(&'a str),
}

enum HighlightIter<'a> {
  TreeSitter(CapturesIter<'a>),
}

struct MergeIterator<'a> {
  iterators: Vec<HighlightIter<'a>>,
}

impl EditorState {
  pub fn highlights(&self, range: Range<usize>) -> impl Iterator<Item = Highlight<'_>> {
    let mut iterators = vec![];

    if let Some(highlighter) = &self.highligher
      && let Some(highlights) = highlighter.highlights(&self.doc, range)
    {
      iterators.push(HighlightIter::TreeSitter(highlights));
    }

    MergeIterator { iterators }
  }
}

impl<'a> Iterator for HighlightIter<'a> {
  type Item = Highlight<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      HighlightIter::TreeSitter(iter) => iter.next(),
    }
  }
}

impl<'a> Iterator for MergeIterator<'a> {
  type Item = Highlight<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    // TODO
    if let Some(first) = self.iterators.first_mut() { first.next() } else { None }
  }
}
