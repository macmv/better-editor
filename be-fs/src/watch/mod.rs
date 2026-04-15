use crate::{DirectoryChanges, WorkspaceRoot};

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "linux")]
pub use inotify::INotifyWatcher;

#[cfg(target_os = "macos")]
mod fsevents;
#[cfg(target_os = "macos")]
pub use fsevents::FSEventsWatcher;

pub type Waker = Box<dyn Fn() + Send + Sync>;

#[allow(unreachable_code)]
pub fn default_watcher(root: &WorkspaceRoot, waker: Waker) -> Box<dyn Watcher> {
  #[cfg(target_os = "linux")]
  return Box::new(INotifyWatcher::new(root));

  #[cfg(target_os = "macos")]
  return Box::new(FSEventsWatcher::new(root, waker));

  panic!("no watcher implemented on platform");
}

pub trait Watcher {
  fn poll(&mut self) -> DirectoryChanges;
}
