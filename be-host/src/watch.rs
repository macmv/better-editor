//! The directory watches this host runs for its client.
//!
//! Each watch polls its watcher whenever the main loop wakes, and the loop
//! wakes at least every `POLL_MS` while any watch exists.

use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};

use be_fs::{DirectoryChanges, WorkspaceRoot, Watcher};
use be_remote::HostError;
use lode_local::{Reply, Sink, TrySendError};

/// How often an idle loop looks at the watchers.
pub const POLL_MS: i32 = 50;

struct Watch {
  watcher: Box<dyn Watcher>,
  sink:    Sink<Vec<String>>,
  /// Kept open until the client drops the call.
  _reply:  Reply<(), HostError>,
  /// Changed, but not yet accepted by the sink.
  pending: DirectoryChanges,
}

#[derive(Default)]
pub struct Watches {
  watches: HashMap<u64, Watch>,
}

pub type SharedWatches = Rc<RefCell<Watches>>;

impl Watches {
  pub fn insert(
    &mut self,
    id: u64,
    root: &Path,
    sink: Sink<Vec<String>>,
    reply: Reply<(), HostError>,
  ) {
    let root = WorkspaceRoot::from_path(root.to_path_buf());
    // Nothing needs waking: the main loop polls on a timer instead.
    let watcher = be_fs::default_watcher(&root, Box::new(|| {}));
    self.watches.insert(id, Watch { watcher, sink, _reply: reply, pending: Default::default() });
  }

  pub fn remove(&mut self, id: u64) { self.watches.remove(&id); }

  pub fn is_empty(&self) -> bool { self.watches.is_empty() }

  /// Tells every watch that this host is the one who wrote `path`.
  pub fn notify_saved(&mut self, path: &Path) {
    for w in self.watches.values_mut() {
      w.watcher.notify_saved(path);
    }
  }

  pub fn pump(&mut self) {
    let mut gone = vec![];
    for (&id, w) in &mut self.watches {
      w.pending.merge_with(&w.watcher.poll());
      if w.pending.is_empty() {
        continue;
      }

      let batch: Vec<String> = w.pending.iter().map(|p| p.to_string()).collect();
      match w.sink.try_send(batch) {
        Ok(()) => w.pending = DirectoryChanges::default(),
        // Keep merging until the client catches up.
        Err(TrySendError::Full(_)) => {}
        Err(_) => gone.push(id),
      }
    }
    for id in gone {
      self.watches.remove(&id);
    }
  }
}
