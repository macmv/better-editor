use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

use be_doc::Document;
use git::GitRepo;

#[macro_use]
extern crate log;

mod diff;
mod git;

pub use diff::{Change, LineDiff, LineDiffSimilarity};

/// This acts like a store for modified files in the editor.
///
/// If the editor is in a git repo, this will show changes since HEAD.
/// Otherwise, this will show changes since the editor was opened.
pub struct Repo {
  root: PathBuf,
  git:  Option<GitRepo>,
  head: Option<git::Oid>,

  files: HashMap<PathBuf, ChangedFile>,
}

struct ChangedFile {
  original: Document,
  current:  Document,
}

impl Repo {
  pub fn open(root: &Path) -> Self {
    let root = root.canonicalize().unwrap();
    Repo { git: GitRepo::open(&root).ok(), head: None, root, files: HashMap::new() }
  }

  pub fn update(&mut self) {
    if let Some(git) = &self.git {
      let head = git.head();
      if self.head != Some(head) {
        self.head = Some(head);
        for (path, file) in &mut self.files {
          file.original = git.lookup_in_head(&path).unwrap_or_else(|| Document::new());
        }
      }
    }
  }

  pub fn open_file(&mut self, path: &Path) {
    let path = path.canonicalize().unwrap();

    if let Ok(rel) = path.strip_prefix(&self.root) {
      let file = if let Some(git) = &self.git {
        ChangedFile {
          original: git.lookup_in_head(&path).unwrap_or_else(|| Document::new()),
          current:  Document::read(&path).unwrap(),
        }
      } else {
        ChangedFile::new(Document::read(&path).unwrap())
      };

      self.files.insert(rel.to_path_buf(), file);
    }
  }

  pub fn update_file(&mut self, path: &Path, doc: &Document) {
    let path = path.canonicalize().unwrap();

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get_mut(rel) {
        file.current = be_doc::Document { rope: doc.rope.clone() };
      } else {
        error!("unknown path: {}", path.display());
      }
    } else {
      error!("unknown path: {}", path.display());
    }
  }

  pub fn changes_in(&self, path: &Path) -> Option<LineDiffSimilarity> {
    let path = path.canonicalize().unwrap();

    if let Ok(rel) = path.strip_prefix(&self.root)
      && let Some(file) = self.files.get(rel)
    {
      return Some(file.changes());
    }

    None
  }
}

impl ChangedFile {
  fn new(doc: Document) -> Self {
    ChangedFile { original: be_doc::Document { rope: doc.rope.clone() }, current: doc }
  }

  fn changes(&self) -> diff::LineDiffSimilarity {
    diff::line_diff_similarity(&self.original, &self.current)
  }
}
