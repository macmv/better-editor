use std::sync::{Arc, Weak};

mod change;
mod watch;

pub use change::DirectoryChanges;
pub use watch::*;

use parking_lot::Mutex;

use crate::change::WorkspaceState;

pub struct WorkspaceWatcher {
  watcher: Box<dyn Watcher>,

  state:   Arc<Mutex<WorkspaceState>>,
  handles: Vec<Weak<WatcherHandle>>,
}

pub struct WatcherHandle {
  watcher:        Arc<Mutex<WorkspaceState>>,
  latest_version: usize,
}

impl WatcherHandle {
  pub fn changes(&self) -> DirectoryChanges {
    let state = self.watcher.lock();
    let mut changes = DirectoryChanges::default();
    for version in state.versions_since(self.latest_version) {
      changes.merge_with(&version.changes);
    }
    changes
  }

  pub fn clear_changes(&mut self) { self.latest_version = self.watcher.lock().latest_version(); }
}
