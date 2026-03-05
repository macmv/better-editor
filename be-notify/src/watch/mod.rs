use crate::DirectoryChanges;

#[cfg(target_os = "linux")]
mod inotify;

pub trait Watcher {
  fn poll(&self) -> DirectoryChanges;
}
