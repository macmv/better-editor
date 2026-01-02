#![allow(dead_code)]

mod diff;
mod git;

use std::path::Path;

use git::GitRepo;

/// This acts like a store for modified files in the editor.
///
/// If the editor is in a git repo, this will show changes since HEAD.
/// Otherwise, this will show changes since the editor was opened.
pub struct Repo {
  git: Option<GitRepo>,
}

impl Repo {
  pub fn open(path: &Path) -> Repo { Repo { git: GitRepo::open(path).ok() } }
}
