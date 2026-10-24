//! The editor's remote protocol, spoken over Lode. `be-host` serves the `Host`
//! interface; the editor dials it, usually through `ssh`.

#[macro_use]
extern crate log;

// The interface macro names `Stream` but only inside the trait it consumes.
#[allow(unused_imports)]
use lode_local::Stream;

mod client;
mod pty;

pub use client::{RemoteHost, ssh_command};
pub use lode::Bytes;
pub use pty::RemotePty;

/// Bumped when the `Host` interface changes in a way old peers cannot read.
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, lode::Message)]
pub struct HostInfo {
  pub version: u32,
  pub cwd:     String,
}

#[derive(Debug, Clone, PartialEq, lode::Message)]
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

#[derive(Debug, Clone, PartialEq, lode::Message)]
pub enum HostError {
  NotFound,
  PermissionDenied,
  NotUtf8,
  Io(String),
}

/// What the editor may ask of the machine it connected to.
#[lode::interface(lock = "host.lock")]
pub trait Host {
  fn info(&self) -> Result<HostInfo, HostError>;
  fn list_dir(&self, path: String) -> Result<Vec<DirEntry>, HostError>;
  fn read_file(&self, path: String) -> Result<String, HostError>;

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
