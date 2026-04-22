use crop::Rope;

use crate::Document;

/// An immutable copy of a Document at a particular point in time.
#[derive(Default, Clone)]
pub struct DocumentSnapshot {
  pub(crate) rope: Rope,
}

impl<T> From<T> for DocumentSnapshot
where
  Document: From<T>,
{
  fn from(v: T) -> Self { Document::from(v).snapshot() }
}

impl DocumentSnapshot {
  pub fn raw_lines(&self) -> crop::iter::RawLines<'_> { self.rope.raw_lines() }
}
