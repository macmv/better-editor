#![allow(dead_code)]

mod diff;
mod git;

use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

use be_doc::Document;
use git::GitRepo;

/// This acts like a store for modified files in the editor.
///
/// If the editor is in a git repo, this will show changes since HEAD.
/// Otherwise, this will show changes since the editor was opened.
pub struct Repo {
  root: PathBuf,
  git:  Option<GitRepo>,

  files: HashMap<PathBuf, Document>,
}

impl Repo {
  pub fn open(root: &Path) -> Self {
    let root = root.canonicalize().unwrap();
    Repo { git: GitRepo::open(&root).ok(), root, files: HashMap::new() }
  }

  pub fn open_file(&mut self, path: &Path) {
    let path = path.canonicalize().unwrap();

    if let Ok(rel) = path.strip_prefix(&self.root) {
      let mut doc = Document::read(&path).unwrap();

      if let Some(git) = &self.git {
        git.apply_work_to_head(&path, &mut doc);
      }

      self.files.insert(rel.to_path_buf(), doc);
    }
  }
}
