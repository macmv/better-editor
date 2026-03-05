use std::{
  collections::VecDeque,
  sync::{Arc, Weak},
};

mod change;
mod watch;

pub use change::DirectoryChanges;
pub use watch::*;

use parking_lot::Mutex;

pub struct WorkspaceWatcher {
  watcher: Box<dyn Watcher>,

  state:   Arc<Mutex<WorkspaceState>>,
  handles: Vec<Weak<WatcherHandle>>,
}

pub struct WatcherHandle {
  state:          Arc<Mutex<WorkspaceState>>,
  latest_version: usize,
}

struct WorkspaceState {
  versions: VecDeque<Version>,
}

struct Version {
  pub id:      usize,
  pub changes: DirectoryChanges,
}

impl WatcherHandle {
  pub fn changes(&self) -> DirectoryChanges {
    let state = self.state.lock();
    let mut changes = DirectoryChanges::default();
    for version in state.versions_since(self.latest_version) {
      changes.merge_with(&version.changes);
    }
    changes
  }

  pub fn clear_changes(&mut self) {
    let mut state = self.state.lock();
    self.latest_version = state.bump_version();
  }
}

impl WorkspaceState {
  fn bump_version(&mut self) -> usize {
    if self.versions.iter().last().is_some_and(|l| !l.changes.is_empty()) {
      self
        .versions
        .push_back(Version { id: self.latest_version() + 1, changes: Default::default() });
    }

    self.latest_version()
  }
}

impl WorkspaceState {
  fn versions_since(&self, version: usize) -> impl Iterator<Item = &Version> {
    if self.versions.is_empty() {
      return self.versions.iter().skip(0); // empty iterator
    }

    if version < self.versions[0].id {
      panic!("version is too old");
    }
    let index = version - self.versions[0].id;
    self.versions.iter().skip(index)
  }

  pub fn latest_version(&self) -> usize {
    if self.versions.is_empty() { 0 } else { self.versions[self.versions.len() - 1].id }
  }
}
