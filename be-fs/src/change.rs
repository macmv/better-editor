use std::fmt;

use btree_slab::BTreeMap;

use crate::WorkspacePathBuf;

#[derive(Default, Clone, PartialEq, Eq)]
pub struct DirectoryChanges {
  changes: BTreeMap<WorkspacePathBuf, ()>,
}

impl DirectoryChanges {
  pub fn for_path(path: WorkspacePathBuf) -> DirectoryChanges {
    DirectoryChanges { changes: BTreeMap::from_iter([(path, ())]) }
  }

  pub fn is_empty(&self) -> bool { self.changes.is_empty() }

  pub fn iter(&self) -> impl Iterator<Item = &WorkspacePathBuf> { self.changes.keys() }

  pub fn insert(&mut self, path: WorkspacePathBuf) {
    self.changes.insert(path, ());
    self.deduplicate();
  }

  pub fn merge_with(&mut self, other: &DirectoryChanges) {
    self.changes.extend(other.changes.iter().map(|(c, _)| (c.clone(), ())));
    self.deduplicate();
  }

  fn deduplicate(&mut self) {
    // BUG: Workaround for btree-slab panicking on an empty tree.
    if self.changes.is_empty() {
      return;
    }

    let mut entries = self.changes.entries_mut();
    while let Some((path, _)) = entries.next() {
      while entries.peek().is_some_and(|it| it.key().starts_with(path)) {
        entries.remove();
      }
    }
  }
}

impl fmt::Debug for DirectoryChanges {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_list().entries(self.changes.keys()).finish()
  }
}

#[cfg(test)]
mod tests {
  use expect_test::Expect;

  use super::*;

  fn changes(changes: &[&str]) -> DirectoryChanges {
    DirectoryChanges { changes: changes.iter().map(|c| (WorkspacePathBuf::from(c), ())).collect() }
  }

  fn merged(a: &[&str], b: &[&str]) -> DirectoryChanges {
    let mut c = changes(a);
    c.merge_with(&changes(b));
    c
  }

  fn check_merged(a: &[&str], b: &[&str], expected: Expect) {
    expected
      .assert_eq(&format!("{:?}", merged(a, b).changes.iter().map(|(c, _)| c).collect::<Vec<_>>()));
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
