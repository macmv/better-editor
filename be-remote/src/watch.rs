use be_fs::{DirectoryChanges, Watcher, WorkspacePathBuf};
use lode_local::{Pending, Stream, TryRecvError};

use crate::{Host, HostError, Transport};

/// Changes under a directory on the remote machine, as reported by its
/// `be-host`. Nothing here blocks: batches arrive with the connection's own
/// wakeups, and `poll` takes what has been delivered.
pub struct RemoteWatcher {
  // Dropping the call stops the watch.
  _call:  Pending<(), HostError>,
  stream: Stream<Vec<String>>,
}

impl RemoteWatcher {
  /// Watches `root` on the host. `id` must be unique among this connection's
  /// terminals and watches.
  pub fn new(t: &Transport, id: u64, root: &std::path::Path) -> RemoteWatcher {
    let (call, stream) = t.local().proxy::<Host>().watch(id, root.to_string_lossy().into_owned());
    t.io_done();
    RemoteWatcher { _call: call, stream }
  }
}

impl Watcher for RemoteWatcher {
  fn poll(&mut self) -> DirectoryChanges {
    let mut changes = DirectoryChanges::default();
    loop {
      match self.stream.try_recv() {
        Ok(batch) => {
          for path in batch {
            changes.insert(WorkspacePathBuf::from(&path));
          }
        }
        Err(TryRecvError::Empty) => break,
        Err(TryRecvError::Closed(_)) => break,
      }
    }
    changes
  }

  // The host marks saves itself, as it is the one writing the file.
}
