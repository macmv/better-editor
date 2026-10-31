//! The directory watches this host runs for its client.
//!
//! A watcher wakes the main loop through a pipe when the filesystem changes,
//! and the loop then forwards what changed to the client. Nothing polls on a
//! timer.
//!
//! Setting a watcher up walks the whole tree, which can take a long time in a
//! big directory, so that happens on a thread. The main loop serves everything
//! else meanwhile, and the client is sent the root as changed once the watcher
//! is ready, to cover whatever happened while it wasn't.

use std::{
  cell::RefCell,
  collections::HashMap,
  io::{Read, Write},
  os::{fd::AsRawFd, unix::net::UnixStream},
  path::Path,
  rc::Rc,
  sync::{Arc, mpsc},
};

use be_fs::{DirectoryChanges, Watcher, WorkspacePathBuf, WorkspaceRoot};
use be_remote::HostError;
use lode_local::{Reply, Sink, TrySendError};
use lode_poll::sys::PollFd;

enum State {
  /// The watcher is still being set up.
  Starting(mpsc::Receiver<Box<dyn Watcher + Send>>),
  Running(Box<dyn Watcher>),
}

struct Watch {
  state:   State,
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
    let wake_fn = {
      let wake = wake.clone();
      move || {
        // A full pipe already has a wake pending.
        let _ = (&*wake).write(&[1]);
      }
    };

    let state = begin(root, wake, wake_fn);

    self.watches.insert(id, Watch { state, sink, _reply: reply, pending: Default::default() });
  }

  pub fn remove(&mut self, id: u64) { self.watches.remove(&id); }

  /// What the poll loop must wait on for the watches.
  pub fn poll_fd(&self) -> PollFd {
    PollFd { fd: self.wake_rx.as_raw_fd(), readable: true, writable: false }
  }

  /// Tells every watch that this host is the one who wrote `path`.
  pub fn notify_saved(&mut self, path: &Path) {
    for w in self.watches.values_mut() {
      if let State::Running(watcher) = &mut w.state {
        watcher.notify_saved(path);
      }
    }
  }

  pub fn pump(&mut self) {
    // Drain first: an event that lands after this is a new wake.
    let mut buf = [0u8; 64];
    while matches!((&self.wake_rx).read(&mut buf), Ok(n) if n > 0) {}

    let mut gone = vec![];
    for (&id, w) in &mut self.watches {
      match &mut w.state {
        State::Starting(rx) => match rx.try_recv() {
          Ok(watcher) => {
            w.state = State::Running(watcher);
            w.pending.insert(WorkspacePathBuf::from(""));
          }
          Err(mpsc::TryRecvError::Empty) => continue,
          Err(mpsc::TryRecvError::Disconnected) => {
            gone.push(id);
            continue;
          }
        },
        State::Running(watcher) => w.pending.merge_with(&watcher.poll()),
      }
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

/// Starts watching `root` on a thread, and wakes the main loop when it's ready.
#[cfg(target_os = "linux")]
fn begin(
  root: WorkspaceRoot,
  wake: Arc<UnixStream>,
  wake_fn: impl Fn() + Send + Sync + 'static,
) -> State {
  let (tx, rx) = mpsc::channel();
  std::thread::Builder::new()
    .name("watch-setup".into())
    .spawn(move || {
      let watcher = be_fs::INotifyWatcher::new(&root);
      watcher.spawn(Box::new(wake_fn));
      // If the watch was dropped meanwhile, nobody wants this.
      if tx.send(Box::new(watcher) as Box<dyn Watcher + Send>).is_ok() {
        let _ = (&*wake).write(&[1]);
      }
    })
    .expect("spawn watch setup");
  State::Starting(rx)
}

// Other platforms' watchers aren't known to be `Send`, so they can't be set up
// on a thread. A big directory stalls the main loop there.
#[cfg(not(target_os = "linux"))]
fn begin(
  root: WorkspaceRoot,
  _wake: Arc<UnixStream>,
  wake_fn: impl Fn() + Send + Sync + 'static,
) -> State {
  State::Running(be_fs::default_watcher(&root, Box::new(wake_fn)))
}
