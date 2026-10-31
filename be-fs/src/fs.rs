use std::{
  io,
  os::unix::fs::MetadataExt,
  path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
  File,
  Dir,
  Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
  pub name: String,
  pub kind: FileKind,
}

/// A file as it was read.
#[derive(Debug)]
pub struct FileData {
  /// The canonical path.
  pub path:  PathBuf,
  pub mtime: i64,
  pub data:  Vec<u8>,
}

/// A read that may not have finished. Polling it never blocks.
pub struct PendingRead {
  poll: Box<dyn FnMut() -> Option<io::Result<FileData>>>,
}

impl PendingRead {
  /// A read that finishes when `poll` returns `Some`.
  pub fn new(poll: impl FnMut() -> Option<io::Result<FileData>> + 'static) -> PendingRead {
    PendingRead { poll: Box::new(poll) }
  }

  /// A read that has already finished.
  pub fn ready(result: io::Result<FileData>) -> PendingRead {
    let mut result = Some(result);
    PendingRead::new(move || result.take())
  }

  /// The result, once there is one. After that, this returns `None` forever.
  pub fn poll(&mut self) -> Option<io::Result<FileData>> { (self.poll)() }
}

/// What a finished write reports.
#[derive(Debug, Clone, Copy)]
pub struct WriteResult {
  /// The file's modification time just before the write, so the caller can
  /// tell if it changed underneath them.
  pub prev_mtime: i64,
  /// The file's modification time just after the write.
  pub mtime:      i64,
}

/// A write that may not have finished. Polling it never blocks.
pub struct PendingWrite {
  poll: Box<dyn FnMut() -> Option<io::Result<WriteResult>>>,
}

impl PendingWrite {
  /// A write that finishes when `poll` returns `Some`.
  pub fn new(poll: impl FnMut() -> Option<io::Result<WriteResult>> + 'static) -> PendingWrite {
    PendingWrite { poll: Box::new(poll) }
  }

  /// A write that has already finished.
  pub fn ready(result: io::Result<WriteResult>) -> PendingWrite {
    let mut result = Some(result);
    PendingWrite::new(move || result.take())
  }

  /// The result, once there is one. After that, this returns `None` forever.
  pub fn poll(&mut self) -> Option<io::Result<WriteResult>> { (self.poll)() }
}

/// The file operations the editor needs from whichever machine holds the
/// workspace. `LocalFs` goes straight to disk.
pub trait Fs {
  fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;

  /// False if every call here waits on another machine, which makes the cheap
  /// ones (like resolving symlinks) worth avoiding on the UI thread.
  fn is_local(&self) -> bool { true }

  /// Starts reading `path` for opening in an editor. An `Fs` that has to wait
  /// on something should return without waiting.
  fn begin_read(&self, path: &Path) -> PendingRead;

  /// Starts replacing the contents of the existing file at `path`. `data` is a
  /// snapshot, so the caller is free to keep editing.
  fn begin_write(&self, path: &Path, data: Vec<u8>) -> PendingWrite;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFs;

fn kind(t: std::fs::FileType) -> FileKind {
  if t.is_dir() {
    FileKind::Dir
  } else if t.is_file() {
    FileKind::File
  } else {
    FileKind::Other
  }
}

impl Fs for LocalFs {
  fn begin_read(&self, path: &Path) -> PendingRead {
    PendingRead::ready((|| {
      let path = path.canonicalize()?;
      let mtime = path.metadata()?.mtime();
      let data = std::fs::read(&path)?;
      Ok(FileData { path, mtime, data })
    })())
  }

  fn begin_write(&self, path: &Path, data: Vec<u8>) -> PendingWrite {
    use std::io::Write;

    PendingWrite::ready((|| {
      let prev_mtime = path.metadata()?.mtime();
      let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
      file.write_all(&data)?;
      drop(file);
      Ok(WriteResult { prev_mtime, mtime: path.metadata()?.mtime() })
    })())
  }

  fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries = vec![];
    for entry in std::fs::read_dir(path)? {
      let entry = entry?;
      // `metadata` follows symlinks, like the file tree always has.
      let meta = entry.metadata()?;
      entries.push(DirEntry {
        name: entry.file_name().to_string_lossy().into_owned(),
        kind: kind(meta.file_type()),
      });
    }
    Ok(entries)
  }
}
