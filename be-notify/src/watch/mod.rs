use crate::DirectoryChanges;

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "linux")]
pub use inotify::INotifyWatcher;

pub trait Watcher {
  fn poll(&mut self) -> DirectoryChanges;
}
