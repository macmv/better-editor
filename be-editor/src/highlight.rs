use crate::EditorState;

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

impl EditorState {
  pub fn highlights(&self) -> impl Iterator<Item = Highlight<'_>> { std::iter::empty() }
}
