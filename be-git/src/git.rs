#![allow(dead_code)]

use std::path::{Path, PathBuf};

use be_doc::DocumentSnapshot;
use git2::Repository;

/// The questions the editor asks of a git repository. `GitRepo` answers them
/// with libgit2 on the local disk; a repository on another machine answers
/// them over the wire. Paths are absolute, in the namespace of whichever
/// machine holds the repository.
pub trait GitBackend {
  /// An identifier for the current HEAD tree, which changes when HEAD does.
  fn head(&self) -> Option<String>;
  /// The contents of `path` at HEAD, if it is tracked there.
  fn lookup_in_head(&self, path: &Path) -> Option<String>;
  fn is_ignored(&self, path: &Path) -> bool;
  fn is_added(&self, path: &Path) -> bool;
  fn is_modified(&self, path: &Path) -> bool;
}

impl GitBackend for GitRepo {
  fn head(&self) -> Option<String> {
    Some(self.repo.head().ok()?.peel_to_tree().ok()?.as_object().id().to_string())
  }

  fn lookup_in_head(&self, path: &Path) -> Option<String> {
    let rel = if path.is_absolute() { path.strip_prefix(&self.root).ok()? } else { path };

    let head = self.repo.head().ok()?.peel_to_tree().ok()?;
    let entry = head.get_path(rel).ok()?;
    let blob = self.repo.find_blob(entry.id()).ok()?;

    Some(String::from_utf8_lossy(blob.content()).into_owned())
  }

  fn is_ignored(&self, path: &Path) -> bool { GitRepo::is_ignored(self, path).unwrap_or(false) }
  fn is_added(&self, path: &Path) -> bool { GitRepo::is_added(self, path).unwrap_or(false) }
  fn is_modified(&self, path: &Path) -> bool { GitRepo::is_modified(self, path).unwrap_or(false) }
}

pub struct GitRepo {
  root: PathBuf,
  repo: Repository,
}

impl GitRepo {
  pub fn open(path: &Path) -> Result<Self, git2::Error> {
    let path = path.canonicalize().map_err(|e| git2::Error::from_str(&e.to_string()))?;
    Ok(GitRepo { repo: Repository::open(&path)?, root: path })
  }

  pub fn head(&self) -> git2::Oid {
    self.repo.head().unwrap().peel_to_tree().unwrap().as_object().id()
  }

  pub fn lookup_in_head(&self, path: &Path) -> Option<DocumentSnapshot> {
    let rel = if path.is_absolute() { path.strip_prefix(&self.root).ok()? } else { path };

    let head = self.repo.head().unwrap().peel_to_tree().unwrap();
    let entry = head.get_path(rel).ok()?;
    let blob = self.repo.find_blob(entry.id()).unwrap();

    Some(DocumentSnapshot::from(String::from_utf8_lossy(blob.content())))
  }

  pub fn is_ignored(&self, path: &Path) -> Result<bool, git2::Error> {
    let rel = if path.is_absolute() {
      let Ok(rel) = path.strip_prefix(&self.root) else { return Ok(false) };
      rel
    } else {
      path
    };

    self.repo.status_should_ignore(rel)
  }

  pub fn is_added(&self, path: &Path) -> Result<bool, git2::Error> {
    let rel = if path.is_absolute() {
      let Ok(rel) = path.strip_prefix(&self.root) else { return Ok(false) };
      rel
    } else {
      path
    };

    self.has_status_in_path(rel, git2::Status::INDEX_NEW | git2::Status::WT_NEW)
  }

  pub fn is_modified(&self, path: &Path) -> Result<bool, git2::Error> {
    let rel = if path.is_absolute() {
      let Ok(rel) = path.strip_prefix(&self.root) else { return Ok(false) };
      rel
    } else {
      path
    };

    self.has_status_in_path(rel, git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED)
  }

  fn has_status_in_path(&self, rel: &Path, mask: git2::Status) -> Result<bool, git2::Error> {
    let mut opts = git2::StatusOptions::new();
    opts
      .include_untracked(true)
      .recurse_untracked_dirs(true)
      .include_ignored(false)
      .include_unmodified(false)
      .pathspec(rel);

    let statuses = self.repo.statuses(Some(&mut opts))?;
    Ok(statuses.iter().any(|entry| entry.status().intersects(mask)))
  }

  pub fn changes_in(&self, path: &Path) -> Option<Changes> {
    let path = path.canonicalize().unwrap();
    let Ok(rel) = path.strip_prefix(&self.root) else { return None };

    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true).pathspec(&rel);

    let head = self.repo.head().unwrap().peel_to_tree().unwrap();
    let staged_diff = self.repo.diff_tree_to_index(Some(&head), None, Some(&mut opts)).unwrap();
    let unstaged_diff = self.repo.diff_index_to_workdir(None, Some(&mut opts)).unwrap();

    println!("staged:");
    print_diff(&staged_diff);
    println!("unstaged:");
    print_diff(&unstaged_diff);

    None
  }
}

fn print_diff(diff: &git2::Diff) {
  diff
    .foreach(
      &mut |_, _| true,
      None,
      Some(&mut |_, hunk| {
        println!(
          "HUNK: -{},{} +{},{}",
          hunk.old_start(),
          hunk.old_lines(),
          hunk.new_start(),
          hunk.new_lines()
        );
        true
      }),
      Some(&mut |_, _, line| {
        let prefix = match line.origin() {
          '+' => "+",
          '-' => "-",
          ' ' => " ",
          _ => "?",
        };
        print!("{}{}", prefix, std::str::from_utf8(line.content()).unwrap());
        true
      }),
    )
    .unwrap();
}

pub struct Changes {}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn foo() {
    let repo = GitRepo::open(Path::new("..")).unwrap();

    repo.changes_in(Path::new(".."));
  }
}
