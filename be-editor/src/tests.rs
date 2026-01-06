use crate::EditorState;
use expect_test::Expect;
use std::{
  fmt,
  ops::{Deref, DerefMut},
};

pub struct TestEditor(EditorState);

pub fn editor(src: &str) -> TestEditor { TestEditor(EditorState::from(src)) }

impl TestEditor {
  fn state(&self) -> String {
    let mut s = self.0.doc.rope.to_string();
    s.insert(self.0.doc.cursor_offset(self.0.cursor) + 1, '⟧');
    s.insert(self.0.doc.cursor_offset(self.0.cursor), '⟦');
    s
  }

  pub fn check(&self, expect: Expect) { expect.assert_eq(&self.state()); }

  pub fn check_repeated(&mut self, f: impl Fn(&mut EditorState), expect: &[Expect]) {
    for expect in expect {
      f(&mut self.0);
      expect.assert_eq(&self.state());
    }
  }
}

impl Deref for TestEditor {
  type Target = EditorState;

  fn deref(&self) -> &Self::Target { &self.0 }
}

impl DerefMut for TestEditor {
  fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

impl fmt::Debug for TestEditor {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.state()) }
}

impl PartialEq<&str> for TestEditor {
  fn eq(&self, other: &&str) -> bool { self.state() == *other }
}

#[test]
fn move_col_works() {
  let mut state = EditorState::from("ab");

  state.move_col_rel(1);
  assert_eq!(state.cursor.line, 0);
  assert_eq!(state.cursor.column, 1);

  state.move_col_rel(1);
  assert_eq!(state.cursor.line, 0);
  assert_eq!(state.cursor.column, 1);
}

#[test]
fn move_graphemes_works() {
  let mut state = EditorState::from("abc\ndef");

  state.move_graphemes(1);
  assert_eq!(state.cursor.line, 0);
  assert_eq!(state.cursor.column, 1);

  state.move_graphemes(1);
  assert_eq!(state.cursor.line, 0);
  assert_eq!(state.cursor.column, 2);

  state.move_graphemes(1);
  assert_eq!(state.cursor.line, 0);
  assert_eq!(state.cursor.column, 3);

  state.move_graphemes(1);
  assert_eq!(state.cursor.line, 1);
  assert_eq!(state.cursor.column, 0);
}
