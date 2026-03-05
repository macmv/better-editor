use std::{
  collections::VecDeque,
  sync::{Arc, Weak, atomic::AtomicUsize},
};

mod change;
mod watch;

pub use change::DirectoryChanges;
pub use watch::*;

use parking_lot::Mutex;

pub struct WorkspaceWatcher {
  watcher: Box<dyn Watcher>,

  state:   Arc<Mutex<WorkspaceState>>,
  handles: Vec<Weak<AtomicUsize>>,
}

pub struct WatcherHandle {
  state:          Arc<Mutex<WorkspaceState>>,
  latest_version: Arc<AtomicUsize>,
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
    for version in
      state.versions_since(self.latest_version.load(std::sync::atomic::Ordering::Relaxed))
    {
      changes.merge_with(&version.changes);
    }
    changes
  }

  pub fn clear_changes(&mut self) {
    let mut state = self.state.lock();
    self.latest_version.store(state.bump_version(), std::sync::atomic::Ordering::Relaxed);
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

impl WorkspaceWatcher {
  pub fn update(&mut self) {
    let changes = self.watcher.poll();
    if changes.is_empty() {
      return;
    }

    let mut state = self.state.lock();
    if state.versions.is_empty() {
      state.versions.push_back(Version { id: 0, changes });
    } else {
      state.versions.iter_mut().last().unwrap().changes.merge_with(&changes);
    }

    let mut min_version = usize::MAX;

    self.handles.retain_mut(|h| {
      if let Some(h) = h.upgrade() {
        min_version = min_version.min(h.load(std::sync::atomic::Ordering::Relaxed));
        true
      } else {
        false
      }
    });

    if state.versions[0].id < min_version {
      let idx = min_version - state.versions[0].id;
      state.versions.drain(..idx);
    }
  }
}
