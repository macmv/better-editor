use std::ops::Range;

use crate::Document;

#[derive(Clone)]
pub struct Edit {
  forward:  Vec<Change>,
  backward: Vec<Change>,
}

#[derive(Debug, Clone)]
pub struct Change {
  pub range: Range<usize>,
  pub text:  String,
}

impl Edit {
  pub const fn empty() -> Self { Edit { forward: vec![], backward: vec![] } }

  pub fn new(change: &Change, doc: &Document) -> Self {
    Edit { backward: vec![change.reverse(doc)], forward: vec![change.clone()] }
  }

  pub fn push(&mut self, change: &Change, doc: &Document) {
    self.backward.push(change.reverse(doc));
    self.forward.push(change.clone());
  }

  pub fn redo(&self) -> impl Iterator<Item = &Change> { self.forward.iter() }

  pub fn undo(&self) -> impl Iterator<Item = &Change> { self.backward.iter().rev() }
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn edit_works() {
    let mut doc = Document::new();

    let change = Change::insert(0, "hello");
    let edit_1 = Edit::new(&change, &doc);
    doc.apply(&change);

    let change = Change::replace(1..2, "a");
    let edit_2 = Edit::new(&change, &doc);
    doc.apply(&change);
    assert_eq!(doc, "hallo");

    edit_2.undo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "hello");

    edit_1.undo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "");

    edit_1.redo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "hello");

    edit_2.redo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "hallo");
  }

  #[test]
  fn multiple_changes() {
    let mut doc = Document::new();

    let change = Change::insert(0, "hello");
    let mut edit = Edit::new(&change, &doc);
    doc.apply(&change);

    let change = Change::replace(1..2, "a");
    edit.push(&change, &doc);
    doc.apply(&change);
    assert_eq!(doc, "hallo");

    edit.undo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "");

    edit.redo().for_each(|c| doc.apply(c));
    assert_eq!(doc, "hallo");
  }
}
