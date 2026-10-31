use std::{
  io,
  path::{Path, PathBuf},
};

use be_fs::{DirEntry, FileKind, Fs, Metadata};
use lode::Bytes;

use crate::{DirKind, Host, Transport};

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

  fn metadata(&self, path: &Path) -> io::Result<Metadata> {
    let stat = self.t.wait(self.t.local().proxy::<Host>().stat(path_str(path)))?;
    Ok(Metadata { kind: kind(stat.kind), mtime: stat.mtime })
  }

  fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
    Ok(self.t.wait(self.t.local().proxy::<Host>().read_bytes(path_str(path)))?.to_vec())
  }

  fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
    self.t.wait(self.t.local().proxy::<Host>().write_bytes(path_str(path), Bytes::copy_from_slice(data)))
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
