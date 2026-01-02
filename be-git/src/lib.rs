#![allow(dead_code)]

mod diff;
mod git;

use std::path::Path;

use git::GitRepo;

pub struct Repo {
  git: Option<GitRepo>,
}

impl Repo {
  pub fn open(path: &Path) -> Repo { Repo { git: GitRepo::open(path).ok() } }
}
