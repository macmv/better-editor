use std::{
  collections::VecDeque,
  sync::{Arc, Weak, atomic::AtomicUsize},
};

mod change;
mod path;
mod watch;

pub use change::DirectoryChanges;
pub use path::{WorkspacePath, WorkspacePathBuf, WorkspaceRoot};
pub use watch::*;

use parking_lot::Mutex;

#[cfg(test)]
#[macro_use]
extern crate expect_test;

#[macro_use]
extern crate log;

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

  pub fn take_changes(&mut self) -> DirectoryChanges {
    // Copy of `WatcherHandle::changes` and `WatcherHandle::clear_changes` with a
    // single lock
    let mut state = self.state.lock();
    let mut changes = DirectoryChanges::default();
    for version in
      state.versions_since(self.latest_version.load(std::sync::atomic::Ordering::Relaxed))
    {
      changes.merge_with(&version.changes);
    }
    self.latest_version.store(state.bump_version(), std::sync::atomic::Ordering::Relaxed);
    changes
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
      #[allow(clippy::iter_skip_zero)]
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
  pub fn new(root: &WorkspaceRoot) -> Self {
    WorkspaceWatcher {
      watcher: watch::default_watcher(root),
      state:   Arc::new(Mutex::new(WorkspaceState { versions: VecDeque::new() })),
      handles: vec![],
    }
  }

  pub fn add_handle(&mut self) -> WatcherHandle {
    let latest_version = self.state.lock().latest_version();
    let handle = WatcherHandle {
      state:          self.state.clone(),
      latest_version: Arc::new(latest_version.into()),
    };
    self.handles.push(Arc::downgrade(&handle.latest_version));
    handle
  }

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

#[cfg(test)]
mod tests {
  use std::sync::mpsc::{Receiver, Sender};

  use expect_test::Expect;

  use super::*;

  struct ChannelWatcher {
    rx: Receiver<WorkspacePathBuf>,
  }

  impl Watcher for ChannelWatcher {
    fn poll(&mut self) -> DirectoryChanges {
      if let Some(v) = self.rx.try_recv().ok() {
        DirectoryChanges::for_path(v)
      } else {
        DirectoryChanges::default()
      }
    }
  }

  fn dummy_watcher() -> (WorkspaceWatcher, Sender<WorkspacePathBuf>) {
    let (tx, rx) = std::sync::mpsc::channel();

    (
      WorkspaceWatcher {
        watcher: Box::new(ChannelWatcher { rx }),
        state:   Arc::new(Mutex::new(WorkspaceState { versions: VecDeque::new() })),
        handles: vec![],
      },
      tx,
    )
  }

  fn check_changes(handle: &WatcherHandle, expected: Expect) {
    let got = handle.changes().iter().map(|p| p.to_string()).collect::<Vec<_>>();
    expected.assert_eq(&format!("[{}]", got.join(", ")));
  }

  #[test]
  fn it_works() {
    let (mut watcher, tx) = dummy_watcher();

    let mut handle = watcher.add_handle();

    tx.send("foo/bar".into()).unwrap();
    watcher.update();

    assert_eq!(watcher.state.lock().versions.len(), 1);
    check_changes(&handle, expect![@"[foo/bar]"]);

    // Changes are added.
    tx.send("foo/baz".into()).unwrap();
    watcher.update();

    assert_eq!(watcher.state.lock().versions.len(), 1);
    check_changes(&handle, expect![@"[foo/bar, foo/baz]"]);

    // Changes are merged.
    tx.send("foo".into()).unwrap();
    watcher.update();

    assert_eq!(watcher.state.lock().versions.len(), 1);
    check_changes(&handle, expect![@"[foo]"]);

    // Clearing results in a new version.
    handle.clear_changes();

    assert_eq!(watcher.state.lock().versions.len(), 2);
    check_changes(&handle, expect![@"[]"]);

    // The new version is modified correctly.
    tx.send("foo/baz".into()).unwrap();
    watcher.update();

    // The old version is dropped once all handles observe it.
    assert_eq!(watcher.state.lock().versions.len(), 1);
    check_changes(&handle, expect![@"[foo/baz]"]);
  }

  #[test]
  fn merge_between_versions() {
    let (mut watcher, tx) = dummy_watcher();

    let mut h1 = watcher.add_handle();
    let h2 = watcher.add_handle();

    tx.send("foo/bar".into()).unwrap();
    watcher.update();

    h1.clear_changes();

    tx.send("foo/baz".into()).unwrap();
    watcher.update();

    assert_eq!(watcher.state.lock().versions.len(), 2);
    check_changes(&h1, expect![@"[foo/baz]"]);
    check_changes(&h2, expect![@"[foo/bar, foo/baz]"]);

    h1.clear_changes();
    watcher.update();
    assert_eq!(watcher.state.lock().versions.len(), 3);
  }

  #[test]
  fn no_bump_on_empty_versions() {
    let (mut watcher, tx) = dummy_watcher();

    let mut h1 = watcher.add_handle();
    let h2 = watcher.add_handle();

    tx.send("foo/bar".into()).unwrap();
    watcher.update();

    h1.clear_changes();
    h1.clear_changes();

    tx.send("foo/baz".into()).unwrap();
    watcher.update();

    assert_eq!(watcher.state.lock().versions.len(), 2);
    check_changes(&h1, expect![@"[foo/baz]"]);
    check_changes(&h2, expect![@"[foo/bar, foo/baz]"]);
  }
}
