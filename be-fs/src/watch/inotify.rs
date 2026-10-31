use std::{
  collections::HashMap,
  ffi::OsStr,
  fs,
  os::{fd::AsRawFd, unix::net::UnixStream},
  path::{Path, PathBuf},
  sync::{
    Arc,
    mpsc::{Receiver, TryRecvError},
  },
  time::{Duration, Instant},
};

use btree_slab::{
  BTreeMap,
  generic::map::{BTreeExt, BTreeExtMut},
};
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};

use super::Watcher;
use crate::{DirectoryChanges, WorkspacePath, WorkspacePathBuf, WorkspaceRoot, ignore::Ignores};

/// How long one `poll` spends adding the watches the background walk found.
const WALK_BUDGET: Duration = Duration::from_millis(4);
/// How many directories the background walk hands over at a time.
const WALK_BATCH: usize = 128;

type SharedWaker = Arc<dyn Fn() + Send + Sync>;

/// What the background walk reports.
enum Walked {
  /// Directories that should be watched.
  Dirs(Vec<WorkspacePathBuf>),
  /// There is nothing more.
  Done,
}

/// Watches every directory under the root that git doesn't ignore.
pub struct INotifyWatcher {
  root:          WorkspaceRoot,
  inotify:       Inotify,
  watch:         BTreeMap<WorkspacePathBuf, WatchDescriptor>,
  reverse_watch: HashMap<WatchDescriptor, WorkspacePathBuf>,
  recent_saves:  HashMap<PathBuf, Instant>,
  ignores:       Ignores,
  /// Set if `spawn` should walk the tree, because only the root is watched.
  needs_walk:    bool,
  /// Directories found by the walk that are waiting to be watched.
  walk:          Option<Receiver<Walked>>,
  waker:         Option<SharedWaker>,
  /// Dropped with the watcher, which hangs up the waker thread's end and
  /// tells it to exit.
  waker_stop:    std::cell::RefCell<Option<UnixStream>>,
}

impl INotifyWatcher {
  /// A watcher of just the root and `.git`. `spawn` goes on to find the rest
  /// of the tree on a background thread, so this doesn't have to wait for it.
  pub fn new(root: &WorkspaceRoot) -> Self {
    let mut watcher = INotifyWatcher::empty(root);
    watcher.needs_walk = true;
    watcher.add_watch_for(WorkspacePath::new(""));
    watcher.add_watch_for(WorkspacePath::new(".git"));
    watcher
  }

  /// A watcher of every directory under the root that git doesn't ignore.
  /// This walks the tree before returning, which can take a long time.
  pub fn recursive(root: &WorkspaceRoot) -> Self {
    let mut watcher = INotifyWatcher::empty(root);
    watcher.add_watch_tree_for(WorkspacePath::new(""));
    watcher
  }

  fn empty(root: &WorkspaceRoot) -> Self {
    let inotify = Inotify::init().unwrap();
    INotifyWatcher {
      root: root.clone(),
      inotify,
      watch: BTreeMap::new(),
      reverse_watch: HashMap::new(),
      recent_saves: HashMap::new(),
      ignores: Ignores::new(root.as_path()),
      needs_walk: false,
      walk: None,
      waker: None,
      waker_stop: Default::default(),
    }
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
      // The directory is gone, or was never there (like a missing `.git`).
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
      Err(err) => {
        warn!("failed to register inotify watch for `{dir}`: {err}");
        return;
      }
    };

    let dir = dir.to_path_buf();
    self.reverse_watch.insert(wd.clone(), dir.clone());
    self.watch.insert(dir, wd);
  }

  /// Watches a directory and its subdirectories, except for ones that git
  /// ignores and `.git`'s.
  fn add_watch_tree_for(&mut self, root: &WorkspacePath) {
    let mut dirs = vec![];
    walk_dirs(self.root.as_path(), &mut self.ignores, root, |dir| {
      dirs.push(dir.to_path_buf());
      true
    });

    for dir in dirs {
      self.add_watch_for(&dir);
    }
  }

  /// Watches the directories the background walk has found, for as long as
  /// there is time. Returns true once the walk has finished.
  fn take_walked(&mut self) -> bool {
    let Some(rx) = self.walk.take() else { return false };

    let start = Instant::now();
    let mut done = false;
    let mut more = false;
    loop {
      if start.elapsed() > WALK_BUDGET {
        more = true;
        break;
      }

      match rx.try_recv() {
        Ok(Walked::Dirs(dirs)) => {
          for dir in dirs {
            self.add_watch_for(&dir);
          }
        }
        Ok(Walked::Done) | Err(TryRecvError::Disconnected) => {
          done = true;
          break;
        }
        Err(TryRecvError::Empty) => break,
      }
    }

    if !done {
      self.walk = Some(rx);
      if more && let Some(waker) = &self.waker {
        // Out of time. Come back for the rest.
        waker();
      }
    }

    done
  }

  /// Finds the directories to watch on another thread. They are watched by
  /// `poll`, as they arrive.
  fn start_walk(&mut self, waker: SharedWaker) {
    if !self.needs_walk {
      return;
    }
    self.needs_walk = false;

    let (tx, rx) = std::sync::mpsc::channel();
    let root = self.root.as_path().to_path_buf();
    let spawned = std::thread::Builder::new().name("inotify-walk".into()).spawn(move || {
      let mut ignores = Ignores::new(&root);
      let mut batch = vec![];
      walk_dirs(&root, &mut ignores, WorkspacePath::new(""), |dir| {
        batch.push(dir.to_path_buf());
        if batch.len() >= WALK_BATCH {
          if tx.send(Walked::Dirs(std::mem::take(&mut batch))).is_err() {
            // The watcher is gone.
            return false;
          }
          waker();
        }
        true
      });
      if !batch.is_empty() {
        let _ = tx.send(Walked::Dirs(batch));
      }
      let _ = tx.send(Walked::Done);
      waker();
    });

    match spawned {
      Ok(_) => self.walk = Some(rx),
      Err(err) => error!("failed to start the directory walk: {err}"),
    }
  }

  /// Spawns a background thread that blocks until inotify events are ready,
  /// then calls `waker`. Events are not consumed -- the main thread must still
  /// call `poll()` to drain them. The thread exits when the watcher is dropped.
  ///
  /// This also starts walking the tree for directories to watch, if that's
  /// still to do.
  pub fn spawn(&mut self, waker: super::Waker) {
    let waker: SharedWaker = Arc::from(waker);
    self.waker = Some(waker.clone());
    self.start_walk(waker.clone());

    let fd = self.inotify.as_raw_fd();
    // dup() so the thread owns a valid fd for the lifetime of the thread,
    // independent of when the original Inotify is dropped.
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
      error!("inotify spawn: dup failed: {}", std::io::Error::last_os_error());
      return;
    }
    let (stop_tx, stop_rx) = match UnixStream::pair() {
      Ok(pair) => pair,
      Err(err) => {
        error!("inotify spawn: socketpair failed: {err}");
        unsafe { libc::close(dup_fd) };
        return;
      }
    };
    *self.waker_stop.borrow_mut() = Some(stop_tx);

    std::thread::Builder::new()
      .name("inotify-waker".into())
      .spawn(move || {
        loop {
          // poll() checks readability without consuming events.
          let mut pfds = [
            libc::pollfd { fd: dup_fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stop_rx.as_raw_fd(), events: libc::POLLIN, revents: 0 },
          ];
          let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, -1) };
          if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
              continue;
            }
            error!("inotify waker poll error: {}", err);
            break;
          }
          // The watcher was dropped.
          if pfds[1].revents != 0 {
            break;
          }
          if pfds[0].revents & libc::POLLNVAL != 0 {
            warn!("inotify waker: fd closed");
            break; // fd was closed
          }
          if pfds[0].revents & libc::POLLIN != 0 {
            waker();

            // The events stay readable until the main thread drains them, so
            // wait for it (or a stop) instead of spinning on the same wake.
            let mut stop =
              libc::pollfd { fd: stop_rx.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut stop, 1, 10) } > 0 && stop.revents != 0 {
              break;
            }
          }
        }
        unsafe { libc::close(dup_fd) };
      })
      .unwrap();
  }

  /// Removes the watch and all children watches from `root`.
  fn remove_watch_tree_for(&mut self, root: &WorkspacePath) {
    let Ok(mut addr) = self.watch.address_of(root) else { return };

    // NB: Removing the last entry leaves `addr` dangling, and btree-slab panics
    // if it's looked up. That happens when the whole watched tree goes away.
    while self.watch.item(addr).is_some_and(|it| it.key().starts_with(root)) {
      let Some((it, a)) = self.watch.remove_at(addr) else { break };
      if self.watch.is_empty() {
        self.forget_watch(it.into_value());
        break;
      }
      addr = a;
      self.forget_watch(it.into_value());
    }
  }

  fn forget_watch(&mut self, wd: WatchDescriptor) {
    self.reverse_watch.remove(&wd);
    if let Err(e) = self.inotify.watches().remove(wd) {
      // Expected when the directory itself was deleted: the kernel already
      // dropped the watch.
      warn!("failed to remove inotify watch: {}", e);
    }
  }
}

impl Watcher for INotifyWatcher {
  fn poll(&mut self) -> DirectoryChanges {
    let mut changes = DirectoryChanges::default();

    // Things may have changed in the directories that weren't watched yet, so
    // report the root as changed, to look again.
    if self.take_walked() {
      changes.insert(WorkspacePathBuf::from(""));
    }

    let mut buffer = [0; 1024];
    let events = match self.inotify.read_events(&mut buffer) {
      Ok(events) => events,
      Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return changes,
      Err(e) => {
        error!("inotify error: {}", e);
        return changes;
      }
    };

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

      // Ignored paths are left alone.
      if ev.name.is_some_and(|n| n == ".gitignore") {
        self.ignores.forget(&parent.clone());
      }
      let ignored = self.ignores.is_ignored(&path, is_dir);

      if !ignored && is_dir && ev.mask.contains(EventMask::CREATE) {
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
        if !ignored {
          self.add_watch_tree_for(&path);
        }
      }

      if ev.mask.contains(EventMask::IGNORED) {
        if let Some(dir) = self.reverse_watch.remove(&ev.wd) {
          self.remove_watch_tree_for(&dir);
        }
      }

      if !ignored {
        changes.insert(path);
      }
    }

    for (_, path) in pending_move_from_dirs.into_iter() {
      self.remove_watch_tree_for(&path);
    }

    let now = Instant::now();
    self.recent_saves.retain(|_, t| now.duration_since(*t) < Duration::from_millis(100));
    changes.retain(|p| !self.recent_saves.contains_key(&self.root.resolve_path(p)));

    changes
  }

  fn notify_saved(&mut self, path: &std::path::Path) {
    self.recent_saves.insert(path.to_path_buf(), Instant::now());
  }
}

/// Calls `visit` with `start`, and every directory under it that git doesn't
/// ignore, until it returns false. Symlinks are not followed. The contents of
/// `.git` are skipped, as nothing there is worth watching.
fn walk_dirs(
  root: &Path,
  ignores: &mut Ignores,
  start: &WorkspacePath,
  mut visit: impl FnMut(&WorkspacePath) -> bool,
) {
  let mut stack = vec![start.to_path_buf()];

  while let Some(dir) = stack.pop() {
    if !visit(&dir) {
      return;
    }
    if dir.file_name() == Some(".git") {
      continue;
    }

    let Ok(entries) = fs::read_dir(root.join(dir.to_string())) else { continue };
    for entry in entries.flatten() {
      // This doesn't follow symlinks, so they are left out.
      // TODO: Follow symlinks without making cycles.
      let Ok(file_type) = entry.file_type() else { continue };
      if !file_type.is_dir() {
        continue;
      }
      let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };

      let child = if dir.is_empty() { WorkspacePathBuf::from(&name) } else { dir.join(&name) };
      if !ignores.is_ignored(&child, true) {
        stack.push(child);
      }
    }
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
    let watcher = INotifyWatcher::recursive(&root);
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
  fn deleting_the_root_does_not_panic() {
    let (temp, mut watcher) = make_watcher(temp_dir!());
    write(&temp.path().join("a/b/c.txt"), "x");
    // Pick up the new directories, so there is a tree to tear down.
    let _ = watcher.poll();
    thread::sleep(Duration::from_millis(20));
    let _ = watcher.poll();

    fs::remove_dir_all(temp.path()).unwrap();
    thread::sleep(Duration::from_millis(50));
    let _ = watcher.poll();

    assert!(watcher.watch.is_empty());
    assert!(watcher.reverse_watch.is_empty());
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

  #[test]
  fn new_only_watches_the_root_and_git_at_first() {
    let temp = temp_dir!();
    let root = WorkspaceRoot::from_path(temp.path().to_path_buf());
    write(&temp.path().join("a/b/c.txt"), "x");
    mkdir(&temp.path().join(".git"));

    let mut watcher = INotifyWatcher::new(&root);
    check_watches(&watcher, expect!(@r#"["", ".git"]"#));

    // Files in a directory that isn't watched yet are not reported.
    write(&temp.path().join("a/b/c.txt"), "y");
    check_changes(&mut watcher, expect![@"[]"]);
  }

  #[test]
  fn recursive_skips_ignored_dirs_and_git_internals() {
    let temp = temp_dir!();
    let root = WorkspaceRoot::from_path(temp.path().to_path_buf());
    write(&temp.path().join(".gitignore"), "target/\nnode_modules\n");
    mkdir(&temp.path().join(".git/objects/ab"));
    mkdir(&temp.path().join("src/deep"));
    mkdir(&temp.path().join("target/debug"));
    mkdir(&temp.path().join("src/node_modules/x"));

    let mut watcher = INotifyWatcher::recursive(&root);
    check_watches(&watcher, expect!(@r#"["", ".git", "src", "src/deep"]"#));

    // Ignored directories are not picked up when they appear, nor are their
    // events reported.
    mkdir(&temp.path().join("target/release"));
    write(&temp.path().join("target/out.txt"), "x");
    mkdir(&temp.path().join("src/new"));
    check_changes(&mut watcher, expect![@r#"["src/new"]"#]);
    check_watches(&watcher, expect!(@r#"["", ".git", "src", "src/deep", "src/new"]"#));
  }

  #[test]
  fn ignored_files_are_not_reported() {
    let temp = temp_dir!();
    let root = WorkspaceRoot::from_path(temp.path().to_path_buf());
    write(&temp.path().join(".gitignore"), "*.log\n");
    mkdir(&temp.path().join("src"));

    let mut watcher = INotifyWatcher::recursive(&root);
    write(&temp.path().join("src/a.log"), "x");
    write(&temp.path().join("src/a.txt"), "x");
    check_changes(&mut watcher, expect![@r#"["src/a.txt"]"#]);

    // Editing the .gitignore takes effect.
    write(&temp.path().join(".gitignore"), "*.txt\n");
    check_changes(&mut watcher, expect![@r#"[".gitignore"]"#]);
    write(&temp.path().join("src/a.log"), "y");
    write(&temp.path().join("src/a.txt"), "y");
    check_changes(&mut watcher, expect![@r#"["src/a.log"]"#]);
  }

  #[test]
  fn background_walk_finds_the_tree() {
    let temp = temp_dir!();
    let root = WorkspaceRoot::from_path(temp.path().to_path_buf());
    write(&temp.path().join(".gitignore"), "target/\n");
    mkdir(&temp.path().join("a/b"));
    mkdir(&temp.path().join("c"));
    mkdir(&temp.path().join("target/debug"));

    let mut watcher = INotifyWatcher::new(&root);
    watcher.spawn(Box::new(|| {}));

    // The walk reports the root as changed once it is done.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut changes = DirectoryChanges::default();
    while Instant::now() < deadline {
      changes.merge_with(&watcher.poll());
      if !changes.is_empty() {
        break;
      }
      thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(format!("{changes:?}"), r#"[""]"#);
    check_watches(&watcher, expect!(@r#"["", "a", "a/b", "c"]"#));
  }

  #[test]
  fn lazy_without_git_dir() {
    let temp = temp_dir!();
    let root = WorkspaceRoot::from_path(temp.path().to_path_buf());
    let watcher = INotifyWatcher::new(&root);
    check_watches(&watcher, expect!(@r#"[""]"#));
  }
}
