#![allow(dead_code)]

use std::path::{Path, PathBuf};

use be_doc::Document;
use git2::Repository;

pub use git2::Oid;

pub struct GitRepo {
  root: PathBuf,
  repo: Repository,
}

impl GitRepo {
  pub fn open(path: &Path) -> Result<Self, git2::Error> {
    let path = path.canonicalize().unwrap();
    Ok(GitRepo { repo: Repository::open(&path)?, root: path })
  }

  pub fn head(&self) -> git2::Oid {
    self.repo.head().unwrap().peel_to_tree().unwrap().as_object().id()
  }

  pub fn lookup_in_head(&self, path: &Path) -> Option<Document> {
    let rel = if path.is_absolute() { path.strip_prefix(&self.root).ok()? } else { path };

    let head = self.repo.head().unwrap().peel_to_tree().unwrap();
    let entry = head.get_path(rel).ok()?;
    let blob = self.repo.find_blob(entry.id()).unwrap();

    Some(Document { rope: be_doc::crop::Rope::from(String::from_utf8_lossy(blob.content())) })
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
