use std::path::PathBuf;

use btree_slab::BTreeMap;

#[derive(Default, Clone, PartialEq, Eq)]
pub struct DirectoryChanges {
  changes: BTreeMap<PathBuf, ()>,
}

impl DirectoryChanges {
  pub fn is_empty(&self) -> bool { self.changes.is_empty() }

  pub fn merge_with(&mut self, other: &DirectoryChanges) {
    self.changes.extend(other.changes.iter().map(|(c, _)| (c.clone(), ())));

    let mut entries = self.changes.entries_mut();
    while let Some((path, _)) = entries.next() {
      while entries.peek().is_some_and(|it| it.key().starts_with(path)) {
        entries.remove();
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use expect_test::{Expect, expect};

  use super::*;

  fn changes(changes: &[&str]) -> DirectoryChanges {
    DirectoryChanges { changes: changes.iter().map(|c| (PathBuf::from(c), ())).collect() }
  }

  fn merged(a: &[&str], b: &[&str]) -> DirectoryChanges {
    let mut c = changes(a);
    c.merge_with(&changes(b));
    c
  }

  fn check_merged(a: &[&str], b: &[&str], expected: Expect) {
    expected.assert_eq(&format!(
      "{:?}",
      merged(a, b).changes.iter().map(|(c, _)| c.to_str().unwrap()).collect::<Vec<_>>()
    ));
  }

  #[test]
  fn merge_works() {
    check_merged(&["a", "b", "c"], &["c", "d", "e"], expect![@r#"["a", "b", "c", "d", "e"]"#]);
  }

  #[test]
  fn merge_removes_children() {
    check_merged(
      &["foo/bar", "foo/baz", "qux"],
      &["foo", "qux/bar"],
      expect![@r#"["foo", "qux"]"#],
    );
  }
}
