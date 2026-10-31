use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

use ignore::{
  Match,
  gitignore::{Gitignore, GitignoreBuilder},
};

use crate::{Component, WorkspacePath, WorkspacePathBuf};

/// Decides which paths in a workspace git ignores, so they can be left
/// unwatched. This follows git's rules: the global ignore file, then
/// `.git/info/exclude`, then each `.gitignore` from the root down, where later
/// ones win. A path under an ignored directory is ignored.
///
/// Each `.gitignore` is read the first time it's needed and then kept, until
/// `forget` says it changed.
pub(crate) struct Ignores {
  root:    PathBuf,
  global:  Gitignore,
  exclude: Gitignore,
  dirs:    HashMap<WorkspacePathBuf, Gitignore>,
}

impl Ignores {
  pub fn new(root: &Path) -> Self {
    let (global, _) = Gitignore::global();
    Ignores {
      root: root.to_path_buf(),
      global,
      exclude: load(root, &root.join(".git/info/exclude")),
      dirs: HashMap::new(),
    }
  }

  /// Drops what was read of the `.gitignore` in `dir`.
  pub fn forget(&mut self, dir: &WorkspacePath) { self.dirs.remove(dir); }

  /// True if git ignores `path`. `.git` is never ignored.
  pub fn is_ignored(&mut self, path: &WorkspacePath, is_dir: bool) -> bool {
    if path.is_empty() || path.starts_with(WorkspacePath::new(".git")) {
      return false;
    }

    let names: Vec<&str> = path
      .components()
      .filter_map(|c| match c {
        Component::Normal(n) => Some(n),
        _ => None,
      })
      .collect();

    let mut prefix = WorkspacePathBuf::from("");
    for (i, name) in names.iter().enumerate() {
      let parent = prefix.clone();
      prefix = prefix.join(name);

      let is_dir = is_dir || i + 1 < names.len();
      if self.ignored_here(&parent, &prefix, is_dir) {
        return true;
      }
    }

    false
  }

  /// Whether `path`, ignoring its parents, is ignored. `parent` is the
  /// directory it is in.
  fn ignored_here(&mut self, parent: &WorkspacePath, path: &WorkspacePath, is_dir: bool) -> bool {
    let abs = self.root.join(path.to_string());

    // `Some(true)` for ignored, `Some(false)` for whitelisted. Later matches
    // win.
    let mut verdict = None;

    fold(&mut verdict, self.global.matched(&abs, is_dir));
    fold(&mut verdict, self.exclude.matched(&abs, is_dir));

    // Every directory from the root down to `parent` can have a `.gitignore`.
    let mut dir = WorkspacePathBuf::from("");
    let mut dirs = vec![dir.clone()];
    for c in parent.components() {
      if let Component::Normal(n) = c {
        dir = dir.join(n);
        dirs.push(dir.clone());
      }
    }
    for dir in dirs {
      if !self.dirs.contains_key(&dir) {
        let base = self.root.join(dir.to_string());
        let gi = load(&base, &base.join(".gitignore"));
        self.dirs.insert(dir.clone(), gi);
      }
      fold(&mut verdict, self.dirs[&dir].matched(&abs, is_dir));
    }

    verdict == Some(true)
  }
}

/// Records what a match says. This is generic so that matches borrowed from
/// different places don't have to share a lifetime.
fn fold<T>(verdict: &mut Option<bool>, m: Match<T>) {
  match m {
    Match::Ignore(_) => *verdict = Some(true),
    Match::Whitelist(_) => *verdict = Some(false),
    Match::None => {}
  }
}

/// Reads `file`, whose patterns are relative to `base`. A missing or invalid
/// file ignores nothing.
fn load(base: &Path, file: &Path) -> Gitignore {
  let mut builder = GitignoreBuilder::new(base);
  // Errors are bad lines, or no file. Either way, use what was parsed.
  let _ = builder.add(file);
  builder.build().unwrap_or_else(|_| Gitignore::empty())
}

#[cfg(test)]
mod tests {
  use std::fs;

  use expect_test::temp_dir;

  use super::*;

  fn check(ig: &mut Ignores, path: &str, is_dir: bool) -> bool {
    ig.is_ignored(WorkspacePath::new(path), is_dir)
  }

  #[test]
  fn follows_gitignore_rules() {
    let temp = temp_dir!();
    let root = temp.path();
    fs::create_dir_all(root.join("sub/deep")).unwrap();
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n*.log\n!keep.log\n").unwrap();
    fs::write(root.join("sub/.gitignore"), "/local\n!*.log\n").unwrap();
    fs::write(root.join(".git/info/exclude"), "excluded\n").unwrap();

    let mut ig = Ignores::new(&root);

    // A directory-only pattern matches directories, not files.
    assert!(check(&mut ig, "target", true));
    assert!(!check(&mut ig, "target", false));
    // Everything under an ignored directory is ignored.
    assert!(check(&mut ig, "target/debug/build", true));
    assert!(check(&mut ig, "sub/target/x", false));

    // Globs, and negation.
    assert!(check(&mut ig, "a.log", false));
    assert!(!check(&mut ig, "keep.log", false));
    // A nested .gitignore overrides the one above it, and is anchored to its
    // own directory.
    assert!(!check(&mut ig, "sub/a.log", false));
    assert!(check(&mut ig, "sub/local", true));
    assert!(!check(&mut ig, "local", true));
    assert!(!check(&mut ig, "sub/deep/local", true));

    assert!(check(&mut ig, "excluded", false));
    assert!(!check(&mut ig, "src/main.rs", false));

    // `.git` and the root are never ignored.
    assert!(!check(&mut ig, ".git", true));
    assert!(!check(&mut ig, ".git/objects", true));
    assert!(!check(&mut ig, "", true));
  }

  #[test]
  fn forget_rereads_the_file() {
    let temp = temp_dir!();
    let root = temp.path();
    let mut ig = Ignores::new(&root);
    assert!(!check(&mut ig, "out", true));

    fs::write(root.join(".gitignore"), "out\n").unwrap();
    // Still cached.
    assert!(!check(&mut ig, "out", true));
    ig.forget(WorkspacePath::new(""));
    assert!(check(&mut ig, "out", true));
  }
}
