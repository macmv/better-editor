use std::{
  collections::HashMap,
  path::{Path, PathBuf},
};

use be_doc::DocumentSnapshot;
use be_task::Task;

#[macro_use]
extern crate log;

mod diff;
mod git;

pub use diff::{Change, LineDiff, LineDiffSimilarity};
pub use git::{GitBackend, GitRepo, PathStatus};

/// This acts like a store for modified files in the editor.
///
/// If the editor is in a git repo, this will show changes since HEAD.
/// Otherwise, this will show changes since the editor was opened.
pub struct Repo {
  root: PathBuf,
  git:  Option<Box<dyn GitBackend>>,
  head: Option<String>,

  files: HashMap<PathBuf, ChangedFile>,
}

struct ChangedFile {
  original: Option<DocumentSnapshot>,
  current:  DocumentSnapshot,

  /// False until HEAD's version of the file has arrived for the first time.
  /// Until then, there is nothing to compare against.
  loaded: bool,
  /// HEAD's version of the file, on its way.
  lookup: Option<Task<Option<String>>>,
}

impl Repo {
  pub fn open(root: &Path) -> Self {
    // TODO: Handle `root` not existing
    let root = root.canonicalize().unwrap();
    let git = GitRepo::open(&root).ok().map(|g| Box::new(g) as Box<dyn GitBackend>);
    Repo::with_backends(root, git)
  }

  /// A repo over another machine's filesystem and git state. `root` must
  /// already be canonical on that machine.
  pub fn with_backends(root: PathBuf, git: Option<Box<dyn GitBackend>>) -> Self {
    Repo { git, head: None, root, files: HashMap::new() }
  }

  pub fn update(&mut self) {
    if let Some(git) = &self.git {
      let head = git.head();
      if self.head != head {
        self.head = head;
        for (rel, file) in &mut self.files {
          // Replacing an older lookup drops it, which cancels its result.
          file.lookup = Some(git.begin_lookup_in_head(&self.root.join(rel)));
        }
      }
    }

    for file in self.files.values_mut() {
      file.poll();
    }
  }

  /// Starts tracking a file that has already been read. `path` must be
  /// canonical.
  pub fn open_file_with(&mut self, path: &Path, current: DocumentSnapshot) {
    let path = path.to_path_buf();
    if let Ok(rel) = path.strip_prefix(&self.root) {
      let mut file = if let Some(git) = &self.git {
        ChangedFile {
          original: None,
          current,
          loaded: false,
          lookup: Some(git.begin_lookup_in_head(&path)),
        }
      } else {
        ChangedFile::new(current)
      };
      // A local lookup is done already.
      file.poll();

      self.files.insert(rel.to_path_buf(), file);
    }
  }

  /// `path` must be canonical.
  pub fn update_file(&mut self, path: &Path, doc: DocumentSnapshot) {
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

  /// The changes to `path` since HEAD, once HEAD's version has arrived.
  pub fn changes_in(&self, path: &Path) -> Option<LineDiffSimilarity> {
    if let Ok(rel) = path.strip_prefix(&self.root)
      && let Some(file) = self.files.get(rel)
      && file.loaded
    {
      return Some(file.changes());
    }

    None
  }

  /// What git says about `path`, or `None` if the answer hasn't arrived. See
  /// `GitBackend::status`.
  pub fn status(&self, path: &Path) -> Option<PathStatus> {
    let git = match &self.git {
      Some(git) => Some(git.status(path)?),
      None => None,
    };
    let ignored = git.is_some_and(|g| g.ignored);

    let Ok(rel) = path.strip_prefix(&self.root) else {
      return Some(PathStatus { ignored, added: false, modified: false });
    };

    // An open file is compared with the text in the editor, not the disk.
    if let Some(file) = self.files.get(rel) {
      if !file.loaded {
        return None;
      }
      return Some(PathStatus { ignored, added: file.is_added(), modified: file.is_modified() });
    }

    Some(PathStatus {
      ignored,
      added: git.is_some_and(|g| g.added),
      modified: git.is_some_and(|g| g.modified),
    })
  }
}

fn snapshot(text: String) -> DocumentSnapshot {
  DocumentSnapshot::from(std::borrow::Cow::Owned(text))
}

impl ChangedFile {
  fn new(doc: DocumentSnapshot) -> Self {
    ChangedFile { original: Some(doc.clone()), current: doc, loaded: true, lookup: None }
  }

  /// Takes HEAD's version of the file, if it has arrived.
  fn poll(&mut self) {
    let Some(lookup) = &self.lookup else { return };
    let Some(original) = lookup.completed() else { return };
    self.lookup = None;

    self.original = original.map(snapshot);
    self.loaded = true;
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
