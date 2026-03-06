use std::path::Path;

use inotify::{Inotify, WatchMask};

use super::Watcher;
use crate::DirectoryChanges;

pub struct INotifyWatcher {
  inotify: Inotify,
}

impl INotifyWatcher {
  pub fn new() -> Self {
    let inotify = Inotify::init().unwrap();

    inotify
      .watches()
      .add(
        ".",
        WatchMask::ATTRIB
          | WatchMask::CREATE
          | WatchMask::DELETE
          | WatchMask::DELETE_SELF
          | WatchMask::MODIFY
          | WatchMask::MOVE_SELF
          | WatchMask::MOVE,
      )
      .expect("Failed to add file watch");

    INotifyWatcher { inotify }
  }
}

impl Watcher for INotifyWatcher {
  fn poll(&mut self) -> DirectoryChanges {
    let mut buffer = [0; 1024];
    let events = self.inotify.read_events(&mut buffer).expect("Error while reading events");

    let mut changes = DirectoryChanges::default();

    for ev in events {
      if let Some(name) = ev.name {
        changes.insert(Path::new(name).into());
      }
    }

    changes
  }
}
