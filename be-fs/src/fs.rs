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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
  pub kind:  FileKind,
  /// Modification time, in seconds since the epoch.
  pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
  pub name: String,
  pub kind: FileKind,
}

/// The file operations the editor needs from whichever machine holds the
/// workspace. `LocalFs` goes straight to disk.
pub trait Fs {
  fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
  fn metadata(&self, path: &Path) -> io::Result<Metadata>;
  fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
  fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
  fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
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
  fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> { path.canonicalize() }

  fn metadata(&self, path: &Path) -> io::Result<Metadata> {
    let meta = path.metadata()?;
    Ok(Metadata { kind: kind(meta.file_type()), mtime: meta.mtime() })
  }

  fn read(&self, path: &Path) -> io::Result<Vec<u8>> { std::fs::read(path) }

  fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(data)
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
