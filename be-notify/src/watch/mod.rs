use crate::DirectoryChanges;

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "linux")]
pub use inotify::INotifyWatcher;

#[allow(unreachable_code)]
pub fn default_watcher() -> Box<dyn Watcher> {
  #[cfg(target_os = "linux")]
  return Box::new(INotifyWatcher::new());

  panic!("no watcher implemented on platform");
}

pub trait Watcher {
  fn poll(&mut self) -> DirectoryChanges;
}
