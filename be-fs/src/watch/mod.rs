use crate::{DirectoryChanges, WorkspacePath, WorkspaceRoot};

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "linux")]
pub use inotify::INotifyWatcher;

#[allow(unreachable_code)]
pub fn default_watcher(root: &WorkspaceRoot) -> Box<dyn Watcher> {
  #[cfg(target_os = "linux")]
  return Box::new(INotifyWatcher::new(root));

  panic!("no watcher implemented on platform");
}

pub trait Watcher {
  /// Watches a particular directory. This simply ensures the path will be
  /// watched, and that events for the path will be returned.
  ///
  /// If the directory is deleted and re-created, it will be watched again.
  fn watch_dir(&mut self, dir: &WorkspacePath);
  /// Stops watching a particular directory.
  fn unwatch_dir(&mut self, dir: &WorkspacePath);

  fn poll(&mut self) -> DirectoryChanges;
}
