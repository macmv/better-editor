//! The directory watches this host runs for its client.
//!
//! A watcher wakes the main loop through a pipe when the filesystem changes,
//! and the loop then forwards what changed to the client. Nothing polls on a
//! timer.

use std::{
  cell::RefCell,
  collections::HashMap,
  io::{Read, Write},
  os::{fd::AsRawFd, unix::net::UnixStream},
  path::Path,
  rc::Rc,
  sync::Arc,
};

use be_fs::{DirectoryChanges, WorkspaceRoot, Watcher};
use be_remote::HostError;
use lode_local::{Reply, Sink, TrySendError};
use lode_poll::sys::PollFd;

struct Watch {
  watcher: Box<dyn Watcher>,
  sink:    Sink<Vec<String>>,
  /// Kept open until the client drops the call.
  _reply:  Reply<(), HostError>,
  /// Changed, but not yet accepted by the sink.
  pending: DirectoryChanges,
}

pub struct Watches {
  watches: HashMap<u64, Watch>,
  /// Written to (from the watchers' threads) when a watcher has events, so the
  /// main loop's `poll` returns.
  wake_tx: Arc<UnixStream>,
  wake_rx: UnixStream,
}

impl Default for Watches {
  fn default() -> Self {
    let (tx, rx) = UnixStream::pair().expect("wake pipe");
    tx.set_nonblocking(true).expect("wake pipe");
    rx.set_nonblocking(true).expect("wake pipe");
    Watches { watches: HashMap::new(), wake_tx: Arc::new(tx), wake_rx: rx }
  }
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
    let wake = self.wake_tx.clone();
    let watcher = be_fs::default_watcher(
      &root,
      Box::new(move || {
        // A full pipe already has a wake pending.
        let _ = (&*wake).write(&[1]);
      }),
    );
    self.watches.insert(id, Watch { watcher, sink, _reply: reply, pending: Default::default() });
  }

  pub fn remove(&mut self, id: u64) { self.watches.remove(&id); }

  /// What the poll loop must wait on for the watches.
  pub fn poll_fd(&self) -> PollFd {
    PollFd { fd: self.wake_rx.as_raw_fd(), readable: true, writable: false }
  }

  /// Tells every watch that this host is the one who wrote `path`.
  pub fn notify_saved(&mut self, path: &Path) {
    for w in self.watches.values_mut() {
      w.watcher.notify_saved(path);
    }
  }

  pub fn pump(&mut self) {
    // Drain first: an event that lands after this is a new wake.
    let mut buf = [0u8; 64];
    while matches!((&self.wake_rx).read(&mut buf), Ok(n) if n > 0) {}

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
