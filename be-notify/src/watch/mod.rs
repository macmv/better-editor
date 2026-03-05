#[cfg(target_os = "linux")]
mod inotify;

pub trait Watcher {}
