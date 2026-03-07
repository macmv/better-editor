use std::{collections::HashMap, ffi::OsStr, fs};

use btree_slab::{
  BTreeMap,
  generic::map::{BTreeExt, BTreeExtMut},
};
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};

use super::Watcher;
use crate::{DirectoryChanges, WorkspacePath, WorkspacePathBuf, WorkspaceRoot};

pub struct INotifyWatcher {
  root:          WorkspaceRoot,
  inotify:       Inotify,
  watch:         BTreeMap<WorkspacePathBuf, WatchDescriptor>,
  reverse_watch: HashMap<WatchDescriptor, WorkspacePathBuf>,
}

impl INotifyWatcher {
  pub fn new(root: &WorkspaceRoot) -> Self {
    let inotify = Inotify::init().unwrap();
    let mut watcher = INotifyWatcher {
      root: root.clone(),
      inotify,
      watch: BTreeMap::new(),
      reverse_watch: HashMap::new(),
    };
    watcher.add_watch_tree_for(WorkspacePath::new(""));
    watcher
  }

  fn path_for_event(
    &self,
    parent: &WorkspacePath,
    name: Option<&OsStr>,
  ) -> Option<WorkspacePathBuf> {
    let Some(name) = name else { return Some(parent.into()) };
    let name = name.to_str()?;

    if parent.is_empty() { Some(WorkspacePathBuf::from(&name)) } else { Some(parent.join(name)) }
  }

  /// Watches a single directory, non-recursively.
  fn add_watch_for(&mut self, dir: &WorkspacePath) {
    if self.watch.contains_key(dir) {
      return;
    }

    let mask = if dir.is_empty() { root_mask() } else { inner_mask() };
    let wd = match self.inotify.watches().add(self.root.resolve_path(dir), mask) {
      Ok(wd) => wd,
      Err(err) => {
        warn!("failed to register inotify watch for `{dir}`: {err}");
        return;
      }
    };

    let dir = dir.to_path_buf();
    self.reverse_watch.insert(wd.clone(), dir.clone());
    self.watch.insert(dir, wd);
  }

  /// Watches a directory and all of its subdirectories, recursively.
  fn add_watch_tree_for(&mut self, root: &WorkspacePath) {
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
      if self.watch.contains_key(&dir) {
        continue;
      }

      // Watch before recursing to avoid races.
      self.add_watch_for(&dir);
      let Ok(entries) = fs::read_dir(self.root.resolve_path(&dir)) else { continue };

      for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else { continue };

        // TODO: Follow symlinks without making cycles.
        if !file_type.is_dir() || file_type.is_symlink() {
          continue;
        }

        if let Ok(rel) = entry.path().strip_prefix(self.root.as_path())
          && let Some(rel) = rel.to_str()
        {
          // TODO: Paths that are watched should be triggered by handles. We should not
          // greedily watch paths.
          if !rel.starts_with("target") && !rel.starts_with(".git") {
            stack.push(WorkspacePathBuf::from(&rel));
          }
        }
      }
    }
  }

  /// Removes the watch and all children watches from `root`.
  fn remove_watch_tree_for(&mut self, root: &WorkspacePath) {
    let Ok(mut addr) = self.watch.address_of(root) else { return };

    while self.watch.item(addr).is_some_and(|it| it.key().starts_with(root)) {
      let Some((it, a)) = self.watch.remove_at(addr) else { break };
      addr = a;
      let wd = it.into_value();

      self.reverse_watch.remove(&wd);
      if let Err(e) = self.inotify.watches().remove(wd) {
        warn!("failed to remove inotify watch: {}", e);
      }
    }
  }
}

impl Watcher for INotifyWatcher {
  fn poll(&mut self) -> DirectoryChanges {
    let mut buffer = [0; 1024];
    let events = match self.inotify.read_events(&mut buffer) {
      Ok(events) => events,
      Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return DirectoryChanges::default(),
      Err(e) => {
        error!("inotify error: {}", e);
        return DirectoryChanges::default();
      }
    };

    let mut changes = DirectoryChanges::default();
    let mut pending_move_from_dirs: HashMap<u32, WorkspacePathBuf> = HashMap::new();

    for ev in events {
      let Some(parent) = self.reverse_watch.get(&ev.wd) else {
        warn!("unknown watch descriptor from inotify: {:?}", ev.wd);
        continue;
      };

      if ev.mask.intersects(EventMask::DELETE_SELF | EventMask::MOVE_SELF) {
        changes.insert(parent.clone());
        self.remove_watch_tree_for(&parent.clone());
        continue;
      }

      let is_dir = ev.mask.contains(EventMask::ISDIR);
      let path = match self.path_for_event(parent, ev.name) {
        Some(path) => path,
        // TODO: Need to handle non-utf8 paths.
        None => continue,
      };

      if is_dir && ev.mask.contains(EventMask::CREATE) {
        self.add_watch_tree_for(&path);
      }

      if is_dir && ev.mask.contains(EventMask::DELETE) {
        self.remove_watch_tree_for(&path);
      }

      if is_dir && ev.mask.contains(EventMask::MOVED_FROM) {
        if ev.cookie != 0 {
          // NB: MOVED_FROM is garunteed to come before MOVED_TO.
          pending_move_from_dirs.insert(ev.cookie, path.to_path_buf());
        } else {
          self.remove_watch_tree_for(&path);
        }
      }

      if is_dir && ev.mask.contains(EventMask::MOVED_TO) {
        if ev.cookie != 0
          && let Some(old_path) = pending_move_from_dirs.remove(&ev.cookie)
        {
          self.remove_watch_tree_for(&old_path);
        }
        self.add_watch_tree_for(&path);
      }

      if ev.mask.contains(EventMask::IGNORED) {
        if let Some(dir) = self.reverse_watch.remove(&ev.wd) {
          self.remove_watch_tree_for(&dir);
        }
      }

      changes.insert(path);
    }

    for (_, path) in pending_move_from_dirs.into_iter() {
      self.remove_watch_tree_for(&path);
    }

    changes
  }
}

fn root_mask() -> WatchMask {
  WatchMask::ATTRIB
    | WatchMask::CREATE
    | WatchMask::DELETE
    | WatchMask::DELETE_SELF
    | WatchMask::MODIFY
    | WatchMask::MOVE_SELF
    | WatchMask::MOVE
}

fn inner_mask() -> WatchMask {
  WatchMask::ATTRIB | WatchMask::CREATE | WatchMask::DELETE | WatchMask::MODIFY | WatchMask::MOVE
}

#[cfg(test)]
mod tests {
  use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
  };

  use expect_test::{Expect, TempDir, temp_dir};

  use crate::DirectoryChanges;

  use super::{INotifyWatcher, Watcher, WorkspaceRoot};

  fn make_watcher(temp: TempDir) -> (expect_test::TempDir, INotifyWatcher) {
    make_watcher_in(temp, "")
  }

  fn make_watcher_in(temp: TempDir, path: &str) -> (expect_test::TempDir, INotifyWatcher) {
    let root = WorkspaceRoot::from_path(temp.path().join(path));
    mkdir(&temp.path().join(path));
    let watcher = INotifyWatcher::new(&root);
    (temp, watcher)
  }

  fn mkdir(path: &Path) { fs::create_dir_all(path).unwrap(); }

  fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).unwrap();
    }

    fs::write(path, contents).unwrap();
  }

  fn check_changes(watcher: &mut INotifyWatcher, expected: Expect) {
    let deadline = Instant::now() + Duration::from_millis(100);
    let mut changes = DirectoryChanges::default();
    loop {
      changes.merge_with(&watcher.poll());
      if expected.data() == &format!("{:?}", changes) {
        return;
      }
      if Instant::now() >= deadline {
        expected.assert_eq(&format!("{:?}", changes));
        break;
      }
      thread::sleep(Duration::from_millis(10));
    }
  }

  fn check_watches(watcher: &INotifyWatcher, expected_paths: Expect) {
    let watched_paths = watcher.watch.keys().map(|p| p.to_string()).collect::<Vec<_>>();
    expected_paths.assert_eq(&format!("{:?}", watched_paths));

    let watch_descriptors =
      watcher.watch.values().map(|wd| format!("{wd:?}")).collect::<BTreeSet<_>>();
    let reverse_descriptors =
      watcher.reverse_watch.keys().map(|wd| format!("{wd:?}")).collect::<BTreeSet<_>>();
    assert_eq!(watch_descriptors, reverse_descriptors, "watch/reverse descriptor sets diverged");
  }

  #[test]
  fn new_subtree() {
    let (temp, mut watcher) = make_watcher(temp_dir!());
    check_watches(&watcher, expect!(@r#"[""]"#));

    mkdir(&temp.path().join("new/sub"));
    check_changes(&mut watcher, expect![@r#"["new"]"#]);
    check_watches(&watcher, expect!(@r#"["", "new", "new/sub"]"#));

    write(&temp.path().join("new/sub/file.txt"), "hello");
    check_changes(&mut watcher, expect![@r#"["new/sub/file.txt"]"#]);
    check_watches(&watcher, expect!(@r#"["", "new", "new/sub"]"#));
  }

  #[test]
  fn rename_directory() {
    let (temp, mut watcher) = make_watcher(temp_dir!());
    check_watches(&watcher, expect!(@r#"[""]"#));

    mkdir(&temp.path().join("old/sub"));
    check_changes(&mut watcher, expect![@r#"["old"]"#]);
    check_watches(&watcher, expect!(@r#"["", "old", "old/sub"]"#));

    fs::rename(temp.path().join("old"), temp.path().join("moved")).unwrap();
    check_changes(&mut watcher, expect![@r#"["moved", "old"]"#]);
    check_watches(&watcher, expect!(@r#"["", "moved", "moved/sub"]"#));

    write(&temp.path().join("moved/sub/after.txt"), "1");
    check_changes(&mut watcher, expect![@r#"["moved/sub/after.txt"]"#]);
    check_watches(&watcher, expect!(@r#"["", "moved", "moved/sub"]"#));
  }

  #[test]
  fn move_from_outside() {
    let (temp, mut watcher) = make_watcher_in(temp_dir!(), "foo");
    mkdir(&temp.path().join("bar/baz"));
    fs::rename(temp.path().join("bar"), temp.path().join("foo/bar")).unwrap();

    check_changes(&mut watcher, expect![@r#"["bar"]"#]);
    check_watches(&watcher, expect!(@r#"["", "bar", "bar/baz"]"#));
  }
}
