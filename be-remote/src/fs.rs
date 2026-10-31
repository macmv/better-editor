use std::{
  io,
  path::{Path, PathBuf},
};

use be_fs::{DirEntry, FileData, FileKind, Fs, PendingRead, PendingWrite, WriteResult};
use lode::Bytes;

use crate::{DirKind, Host, Transport, transport};

/// The remote machine's filesystem. Every call blocks for a round trip.
pub struct RemoteFs {
  t: Transport,
}

impl RemoteFs {
  pub fn new(t: Transport) -> RemoteFs { RemoteFs { t } }
}

fn kind(k: DirKind) -> FileKind {
  match k {
    DirKind::File => FileKind::File,
    DirKind::Dir => FileKind::Dir,
    DirKind::Other => FileKind::Other,
  }
}

fn path_str(path: &Path) -> String { path.to_string_lossy().into_owned() }

impl Fs for RemoteFs {
  fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
    self.t.wait(self.t.local().proxy::<Host>().canonicalize(path_str(path))).map(PathBuf::from)
  }

  fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
    Ok(self.t.wait(self.t.local().proxy::<Host>().read_bytes(path_str(path)))?.to_vec())
  }

  fn is_local(&self) -> bool { false }

  /// One round trip, and no waiting: the result is picked up by polling, as
  /// the connection's owner delivers it.
  fn begin_read(&self, path: &Path) -> PendingRead {
    let mut call = self.t.local().proxy::<Host>().open_file(path_str(path));
    self.t.io_done();
    PendingRead::new(move || {
      Some(match call.try_take()? {
        Ok(f) => Ok(FileData { path: PathBuf::from(f.path), mtime: f.mtime, data: f.data.to_vec() }),
        Err(lode_local::CallError::App(e)) => Err(transport::io_error(e)),
        Err(lode_local::CallError::Rpc(e)) => Err(transport::rpc_error(e)),
      })
    })
  }

  fn begin_write(&self, path: &Path, data: Vec<u8>) -> PendingWrite {
    let mut call = self.t.local().proxy::<Host>().write_file(path_str(path), Bytes::from(data));
    self.t.io_done();
    PendingWrite::new(move || {
      Some(match call.try_take()? {
        Ok(w) => Ok(WriteResult { prev_mtime: w.prev_mtime, mtime: w.mtime }),
        Err(lode_local::CallError::App(e)) => Err(transport::io_error(e)),
        Err(lode_local::CallError::Rpc(e)) => Err(transport::rpc_error(e)),
      })
    })
  }

  fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
    let entries = self.t.wait(self.t.local().proxy::<Host>().list_dir(path_str(path)))?;
    Ok(entries.into_iter().map(|e| DirEntry { name: e.name, kind: kind(e.kind) }).collect())
  }
}

impl RemoteFs {
  /// Every file under `root` that git would not ignore, relative to `root`.
  pub fn list_files(&self, root: &Path) -> io::Result<Vec<String>> {
    self.t.wait(self.t.local().proxy::<Host>().list_files(path_str(root)))
  }
}
