use std::{collections::VecDeque, path::PathBuf};

use btree_slab::BTreeMap;

#[derive(Default, Clone, PartialEq, Eq)]
pub struct DirectoryChanges {
  changes: BTreeMap<PathBuf, ()>,
}

pub(crate) struct WorkspaceState {
  versions: VecDeque<Version>,
}

pub(crate) struct Version {
  pub id:      usize,
  pub changes: DirectoryChanges,
}

impl DirectoryChanges {
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

impl WorkspaceState {
  pub fn versions_since(&self, version: usize) -> impl Iterator<Item = &Version> {
    if self.versions.is_empty() {
      return self.versions.iter().skip(0); // empty iterator
    }

    if version < self.versions[0].id {
      panic!("version is too old");
    }
    let index = version - self.versions[0].id;
    self.versions.iter().skip(index)
  }

  pub fn latest_version(&self) -> usize {
    if self.versions.is_empty() { 0 } else { self.versions[self.versions.len() - 1].id }
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
