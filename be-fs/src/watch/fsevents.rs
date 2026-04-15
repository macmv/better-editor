use std::{ffi::c_void, path::Path, sync::mpsc};

use dispatch2::DispatchQueue;
use objc2_core_foundation::{CFArray, CFString};
use objc2_core_services::*;

use super::Watcher;
use crate::{DirectoryChanges, WorkspacePathBuf, WorkspaceRoot};

pub struct FSEventsWatcher {
  root:   WorkspaceRoot,
  rx:     mpsc::Receiver<Vec<Event>>,
  stream: FSEventStreamRef,
}

struct Event {
  path:  String,
  flags: FSEventStreamEventFlags,
}

// SAFETY: FSEventStreamRef is thread-safe when used with a dispatch queue.
unsafe impl Send for FSEventsWatcher {}

impl Drop for FSEventsWatcher {
  fn drop(&mut self) {
    unsafe {
      FSEventStreamStop(self.stream);
      FSEventStreamInvalidate(self.stream);
      FSEventStreamRelease(self.stream);
    }
  }
}

impl FSEventsWatcher {
  pub fn new(root: &WorkspaceRoot) -> Self {
    let (tx, rx) = mpsc::channel();

    let root_str = root.as_path().to_str().expect("workspace root must be valid UTF-8");
    let cf_path = CFString::from_str(root_str);
    let paths = CFArray::from_objects(&[&*cf_path]);

    let context_ptr = Box::into_raw(Box::new(tx));

    let mut context = FSEventStreamContext {
      version:         0,
      info:            context_ptr as *mut c_void,
      retain:          None,
      release:         None,
      copyDescription: None,
    };

    let stream = unsafe {
      FSEventStreamCreate(
        None,
        Some(stream_callback),
        &mut context,
        paths.as_opaque(),
        kFSEventStreamEventIdSinceNow,
        0.0,
        kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer,
      )
    };

    let queue = DispatchQueue::new("be-fs.fsevents", None);
    unsafe {
      FSEventStreamSetDispatchQueue(stream, Some(&queue));
      FSEventStreamStart(stream);
    }

    FSEventsWatcher { root: root.clone(), rx, stream }
  }
}

unsafe extern "C-unwind" fn stream_callback(
  _stream_ref: ConstFSEventStreamRef,
  info: *mut c_void,
  num_events: usize,
  event_paths: std::ptr::NonNull<c_void>,
  event_flags: std::ptr::NonNull<FSEventStreamEventFlags>,
  _event_ids: std::ptr::NonNull<FSEventStreamEventId>,
) {
  unsafe {
    let tx = &*(info as *const mpsc::Sender<Vec<Event>>);
    let paths = event_paths.as_ptr() as *const *const std::ffi::c_char;

    let events = (0..num_events)
      .map(|i| {
        let path = std::ffi::CStr::from_ptr(*paths.add(i));
        let path = path.to_string_lossy().into_owned();
        let flags = *event_flags.as_ptr().add(i);
        Event { path, flags }
      })
      .collect();

    if tx.send(events).is_err() {
      error!("fsevents receiver has disconnected");
    }
  }
}

impl Watcher for FSEventsWatcher {
  fn poll(&mut self) -> DirectoryChanges {
    let mut changes = DirectoryChanges::default();

    while let Ok(events) = self.rx.try_recv() {
      let root = self.root.as_path();
      for ev in events {
        const INTERESTED_FLAGS: FSEventStreamEventFlags = kFSEventStreamEventFlagItemCreated
          | kFSEventStreamEventFlagItemRemoved
          | kFSEventStreamEventFlagItemRenamed
          | kFSEventStreamEventFlagItemModified;

        if ev.flags & INTERESTED_FLAGS == 0 {
          continue;
        }

        let path = Path::new(&ev.path);
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let Some(rel) = rel.to_str() else { continue };

        changes.insert(WorkspacePathBuf::from(&rel));
      }
    }

    changes
  }
}
