//! The editor's remote protocol, spoken over Lode. `be-host` serves the `Host`
//! interface; the editor dials it, usually through `ssh`.

#[macro_use]
extern crate log;

// The interface macro names `Stream` but only inside the trait it consumes.
#[allow(unused_imports)]
use lode_local::Stream;

mod client;
mod fs;
mod git;
mod pty;
mod transport;
mod watch;

pub use client::{RemoteHost, ssh_command, ssh_exec_command};
pub use lode::Bytes;
pub use fs::RemoteFs;
pub use git::RemoteGit;
pub use pty::RemotePty;
pub use transport::Transport;
pub use watch::RemoteWatcher;

/// Bumped when the `Host` interface changes in a way old peers cannot read.
pub const PROTOCOL_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, lode::Message)]
pub struct HostInfo {
  pub version: u32,
  pub cwd:     String,
}

#[derive(Debug, Clone, Copy, PartialEq, lode::Message)]
pub enum DirKind {
  File,
  Dir,
  Other,
}

/// A terminal's size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, lode::Message)]
pub struct TermSize {
  pub rows: u16,
  pub cols: u16,
}

#[derive(Debug, Clone, PartialEq, lode::Message)]
pub struct DirEntry {
  pub name: String,
  pub kind: DirKind,
}

#[derive(Debug, Clone, Copy, PartialEq, lode::Message)]
pub struct FileStat {
  pub kind:  DirKind,
  /// Modification time, in seconds since the epoch.
  pub mtime: i64,
}

/// What git says about one path.
#[derive(Debug, Clone, Copy, Default, PartialEq, lode::Message)]
pub struct GitFileStatus {
  pub ignored:  bool,
  pub added:    bool,
  pub modified: bool,
}

#[derive(Debug, Clone, PartialEq, lode::Message)]
pub enum HostError {
  NotFound,
  PermissionDenied,
  NotUtf8,
  Io(String),
}

/// What a write reports.
#[derive(Debug, Clone, Copy, PartialEq, lode::Message)]
pub struct WrittenFile {
  /// The file's modification time just before the write.
  pub prev_mtime: i64,
  /// The file's modification time just after the write.
  pub mtime:      i64,
}

/// A file, read in one call.
#[derive(Debug, Clone, PartialEq, lode::Message)]
pub struct OpenedFile {
  /// The canonical path.
  pub path:  String,
  pub mtime: i64,
  pub data:  Bytes,
}

/// What the editor may ask of the machine it connected to.
#[lode::interface(lock = "host.lock")]
pub trait Host {
  fn info(&self) -> Result<HostInfo, HostError>;
  fn list_dir(&self, path: String) -> Result<Vec<DirEntry>, HostError>;
  fn read_file(&self, path: String) -> Result<String, HostError>;

  fn stat(&self, path: String) -> Result<FileStat, HostError>;
  fn canonicalize(&self, path: String) -> Result<String, HostError>;
  /// `canonicalize`, `stat`, and `read_bytes` in one round trip.
  fn open_file(&self, path: String) -> Result<OpenedFile, HostError>;
  fn read_bytes(&self, path: String) -> Result<Bytes, HostError>;
  /// Replaces the contents of an existing file.
  fn write_file(&self, path: String, data: Bytes) -> Result<WrittenFile, HostError>;

  /// Every file under `root` that git would not ignore, relative to `root`.
  fn list_files(&self, root: String) -> Result<Vec<String>, HostError>;

  /// Streams the workspace-relative paths that changed under `root`, in
  /// batches. `id` names the watch, and is unique among the connection's
  /// terminals and watches. The call answers never; dropping it stops the
  /// watch.
  fn watch(&self, id: u64, root: String) -> Result<Stream<Vec<String>>, HostError>;

  /// An identifier for the tree at HEAD, which changes when HEAD does. `None`
  /// if `root` is not in a git repository.
  fn git_head(&self, root: String) -> Result<Option<String>, HostError>;
  /// The contents of `path` at HEAD, if it is tracked there.
  fn git_lookup(&self, root: String, path: String) -> Result<Option<String>, HostError>;
  fn git_status(&self, root: String, path: String) -> Result<GitFileStatus, HostError>;

  /// Starts a shell in `cwd` (empty: the host's working directory) and streams
  /// what it prints. `id` names the terminal in the calls below, and is chosen
  /// by the caller. The call answers when the shell exits; dropping the call
  /// or the stream kills the shell.
  fn spawn_terminal(
    &self,
    id: u64,
    size: TermSize,
    cwd: String,
  ) -> Result<Stream<Bytes>, HostError>;
  /// Keystrokes for terminal `id`.
  fn terminal_input(&self, id: u64, data: Bytes);
  fn terminal_resize(&self, id: u64, size: TermSize);
}
