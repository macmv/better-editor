fn foo() {
  use inotify::{Inotify, WatchMask};

  let mut inotify = Inotify::init().unwrap();

  // Watch for modify and close events.
  inotify
    .watches()
    .add(
      "/tmp/inotify-test",
      WatchMask::ATTRIB
        | WatchMask::CREATE
        | WatchMask::DELETE
        | WatchMask::DELETE_SELF
        | WatchMask::MODIFY
        | WatchMask::MOVE_SELF
        | WatchMask::MOVE,
    )
    .expect("Failed to add file watch");

  let mut buffer = [0; 1024];
  let events = inotify.read_events_blocking(&mut buffer).expect("Error while reading events");

  for event in events {
    dbg!(&event);
    // Handle event
  }
}
