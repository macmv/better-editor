use std::{
  collections::HashMap,
  path::{Path, PathBuf},
  rc::Rc,
};

use be_doc::{Document, DocumentSnapshot};
use be_fs::{Fs, LocalFs};
use git::GitRepo;

#[macro_use]
extern crate log;

mod diff;
mod git;

pub use diff::{Change, LineDiff, LineDiffSimilarity};
pub use git::GitBackend;

/// This acts like a store for modified files in the editor.
///
/// If the editor is in a git repo, this will show changes since HEAD.
/// Otherwise, this will show changes since the editor was opened.
pub struct Repo {
  root: PathBuf,
  fs:   Rc<dyn Fs>,
  git:  Option<Box<dyn GitBackend>>,
  head: Option<String>,

  files: HashMap<PathBuf, ChangedFile>,
}

struct ChangedFile {
  original: Option<DocumentSnapshot>,
  current:  DocumentSnapshot,
}

impl Repo {
  pub fn open(root: &Path) -> Self {
    // TODO: Handle `root` not existing
    let root = root.canonicalize().unwrap();
    let git = GitRepo::open(&root).ok().map(|g| Box::new(g) as Box<dyn GitBackend>);
    Repo::with_backends(root, Rc::new(LocalFs), git)
  }

  /// A repo over another machine's filesystem and git state. `root` must
  /// already be canonical on that machine.
  pub fn with_backends(root: PathBuf, fs: Rc<dyn Fs>, git: Option<Box<dyn GitBackend>>) -> Self {
    Repo { git, head: None, fs, root, files: HashMap::new() }
  }

  fn read_snapshot(&self, path: &Path) -> Option<DocumentSnapshot> {
    let bytes = self.fs.read(path).ok()?;
    Some(Document::read_lossy(&mut std::io::Cursor::new(bytes)).ok()?.snapshot())
  }

  pub fn update(&mut self) {
    if let Some(git) = &self.git {
      let head = git.head();
      if self.head != head {
        self.head = head;
        for (path, file) in &mut self.files {
          file.original = git.lookup_in_head(path).map(snapshot);
        }
      }
    }
  }

  pub fn open_file(&mut self, path: &Path) {
    let Ok(path) = self.fs.canonicalize(path) else { return };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      let Some(current) = self.read_snapshot(&path) else { return };
      let file = if let Some(git) = &self.git {
        ChangedFile { original: git.lookup_in_head(&path).map(snapshot), current }
      } else {
        ChangedFile::new(current)
      };

      self.files.insert(rel.to_path_buf(), file);
    }
  }

  pub fn update_file(&mut self, path: &Path, doc: DocumentSnapshot) {
    let Ok(path) = self.fs.canonicalize(path) else {
      error!("unknown path: {}", path.display());
      return;
    };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get_mut(rel) {
        file.current = doc;
      } else {
        error!("unknown path: {}", path.display());
      }
    } else {
      error!("unknown path: {}", path.display());
    }
  }

  pub fn changes_in(&self, path: &Path) -> Option<LineDiffSimilarity> {
    let path = self.fs.canonicalize(path).ok()?;

    if let Ok(rel) = path.strip_prefix(&self.root)
      && let Some(file) = self.files.get(rel)
    {
      return Some(file.changes());
    }

    None
  }

  pub fn is_added(&self, path: &Path) -> bool {
    let Some(path) = self.fs.canonicalize(path).ok() else { return false };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get(rel) {
        return file.is_added();
      } else if let Some(git) = &self.git {
        return git.is_added(&path);
      }
    }

    false
  }

  pub fn is_modified(&self, path: &Path) -> bool {
    let Some(path) = self.fs.canonicalize(path).ok() else { return false };

    if let Ok(rel) = path.strip_prefix(&self.root) {
      if let Some(file) = self.files.get(rel) {
        return file.is_modified();
      } else if let Some(git) = &self.git {
        return git.is_modified(&path);
      }
    }

    false
  }

  pub fn is_ignored(&self, path: &Path) -> bool {
    let Some(git) = &self.git else { return false };

    git.is_ignored(path)
  }
}

fn snapshot(text: String) -> DocumentSnapshot {
  DocumentSnapshot::from(std::borrow::Cow::Owned(text))
}

impl ChangedFile {
  fn new(doc: DocumentSnapshot) -> Self {
    ChangedFile { original: Some(doc.clone()), current: doc }
  }

  fn changes(&self) -> diff::LineDiffSimilarity {
    if let Some(original) = &self.original {
      diff::line_diff_similarity(original, &self.current)
    } else {
      // NB: This kinda sucks. However, diffing is slow, so this probably
      // doesn't incur much cost. This should be cached at a higher level
      // honestly.
      diff::line_diff_similarity(&be_doc::Document::new().snapshot(), &self.current)
    }
  }

  fn is_modified(&self) -> bool { self.changes().hunks().next().is_some() }
  fn is_added(&self) -> bool { self.original.is_none() }
}
