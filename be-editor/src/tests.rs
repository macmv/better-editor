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
    let cursor_offset = self.0.doc.cursor_offset(self.0.cursor);

    let c = s[cursor_offset..].chars().next().unwrap();

    if c == '\n' {
      s.insert_str(cursor_offset, "⟦ ⟧");
    } else {
      s.insert(cursor_offset + c.len_utf8(), '⟧');
      s.insert(cursor_offset, '⟦');
    }

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
  let mut editor = editor("ab");

  editor.move_col_rel(1);
  editor.check(expect![@"a⟦b⟧"]);

  editor.move_col_rel(1);
  editor.check(expect![@"a⟦b⟧"]);
}

#[test]
fn move_col_handles_emojis() {
  let mut editor = editor("f💖oo");
  editor.check(expect![@"⟦f⟧💖oo"]);

  editor.check_repeated(
    |e| e.move_col_rel(1),
    &[expect![@"f⟦💖⟧oo"], expect![@"f💖⟦o⟧o"], expect![@"f💖o⟦o⟧"], expect![@"f💖o⟦o⟧"]],
  );
}

#[test]
fn move_graphemes_works() {
  let mut editor = editor("abc\ndef");

  editor.move_graphemes(1);
  editor.check(expect![@r#"
    a⟦b⟧c
    def"#
  ]);

  editor.move_graphemes(1);
  editor.check(expect![@r#"
    ab⟦c⟧
    def"#
  ]);

  editor.move_graphemes(1);
  editor.check(expect![@r#"
    abc⟦ ⟧
    def"#
  ]);

  editor.move_graphemes(1);
  editor.check(expect![@r#"
    abc
    ⟦d⟧ef"#
  ]);
}
