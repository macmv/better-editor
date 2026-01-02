#![allow(dead_code)]

mod diff;
mod git;

use git::GitRepo;

pub struct Repo {
  git: Option<GitRepo>,
}
