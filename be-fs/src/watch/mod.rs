use crate::{DirectoryChanges, WorkspaceRoot};

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
  fn poll(&mut self) -> DirectoryChanges;
}
