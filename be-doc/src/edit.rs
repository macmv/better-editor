use std::ops::Range;

use crate::Document;

pub struct Edit {
  forward:  Vec<Change>,
  backward: Vec<Change>,
}

pub struct Change {
  pub range: Range<usize>,
  pub text:  String,
}

impl Edit {
  pub const fn empty() -> Self { Edit { forward: vec![], backward: vec![] } }

  pub fn new(change: Change, doc: &Document) -> Self {
    Edit { backward: vec![change.reverse(doc)], forward: vec![change] }
  }
}

impl Change {
  pub fn insert(at: usize, text: &str) -> Self { Change { range: at..at, text: text.to_string() } }
  pub fn remove(range: Range<usize>) -> Self { Change { range, text: String::new() } }
  pub fn replace(range: Range<usize>, text: &str) -> Self {
    Change { range, text: text.to_string() }
  }

  pub fn reverse(&self, doc: &Document) -> Change {
    Change {
      range: self.range.start..self.text.len() + self.range.start,
      text:  doc.rope.byte_slice(self.range.clone()).to_string(),
    }
  }
}

impl Document {
  pub fn apply(&mut self, change: &Change) {
    self.rope.replace(change.range.clone(), &change.text);
  }
}
