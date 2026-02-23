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
  original: Option<Document>,
  current:  Document,
}

impl Repo {
  pub fn open(root: &Path) -> Self {
    // TODO: Handle `root` not existing
    let root = root.canonicalize().unwrap();
    Repo { git: GitRepo::open(&root).ok(), head: None, root, files: HashMap::new() }
  }

  pub fn update(&mut self) {
    if let Some(git) = &self.git {
      let head = git.head();
      if self.head != Some(head) {
        self.head = Some(head);
        for (path, file) in &mut self.files {
          file.original = git.lookup_in_head(&path);
        }
      }
    }
  }

  pub fn open_file(&mut self, path: &Path) {
    let Ok(path) = path.canonicalize() else { return };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      let file = if let Some(git) = &self.git {
        ChangedFile {
          original: git.lookup_in_head(&path),
          current:  Document::read(&path).unwrap(),
        }
      } else {
        ChangedFile::new(Document::read(&path).unwrap())
      };

      self.files.insert(rel.to_path_buf(), file);
    }
  }

  pub fn update_file(&mut self, path: &Path, doc: &Document) {
    let Ok(path) = path.canonicalize() else {
      error!("unknown path: {}", path.display());
      return;
    };

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
    let path = path.canonicalize().ok()?;

    if let Ok(rel) = path.strip_prefix(&self.root)
      && let Some(file) = self.files.get(rel)
    {
      return Some(file.changes());
    }

    None
  }

  pub fn is_added(&self, path: &Path) -> bool {
    let Some(path) = path.canonicalize().ok() else { return false };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get(rel) {
        return file.is_added();
      } else if let Some(git) = &self.git {
        return git.is_added(&path).unwrap_or(false);
      }
    }

    false
  }

  pub fn is_modified(&self, path: &Path) -> bool {
    let Some(path) = path.canonicalize().ok() else { return false };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get(rel) {
        return file.is_modified();
      } else if let Some(git) = &self.git {
        return git.is_modified(&path).unwrap_or(false);
      }
    }

    false
  }

  pub fn is_ignored(&self, path: &Path) -> bool {
    let Some(git) = &self.git else { return false };

    git.is_ignored(path).unwrap_or(false)
  }
}

impl ChangedFile {
  fn new(doc: Document) -> Self {
    ChangedFile { original: Some(be_doc::Document { rope: doc.rope.clone() }), current: doc }
  }

  fn changes(&self) -> diff::LineDiffSimilarity {
    if let Some(original) = &self.original {
      diff::line_diff_similarity(original, &self.current)
    } else {
      // NB: This kinda sucks. However, diffing is slow, so this probably doesn't
      // incur much cost. This should be cached at a higher level honestly.
      diff::line_diff_similarity(&be_doc::Document::new(), &self.current)
    }
  }

  fn is_modified(&self) -> bool { self.changes().hunks().next().is_some() }
  fn is_added(&self) -> bool { self.original.is_none() }
}
