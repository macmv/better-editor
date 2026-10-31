use std::{io, path::PathBuf, rc::Rc};

use be_terminal::{PtyHost, Size};
use lode::Bytes;
use lode_local::{CallError, Local, Pending, Stream, TryRecvError};

use crate::{Host, HostError, TermSize};

/// A shell running on a `be-host`, as the other end of a terminal.
///
/// Nothing here blocks or owns a thread. Output arrives through the
/// connection, whose owner runs its poll loop; `read` just takes what that loop
/// has delivered. Input is sent as soon as it is written, so a write only
/// waits (inside the connection) when the kernel's socket buffer is full.
pub struct RemotePty {
  local: Local,
  id:    u64,
  cwd:   PathBuf,

  // Dropping the call kills the remote shell, so it lives as long as we do.
  call:   Pending<(), HostError>,
  stream: Stream<Bytes>,
  // The unread end of the last chunk.
  unread: Bytes,

  // Run after anything that queues bytes, so the owner can start waiting for
  // the socket to drain if it did not take them all.
  on_io: Rc<dyn Fn()>,
}

impl RemotePty {
  /// Asks the host for a shell in `cwd` (empty: the host's working directory).
  /// `id` must be unique among this connection's terminals.
  pub fn new(local: Local, id: u64, size: Size, cwd: String, on_io: Rc<dyn Fn()>) -> RemotePty {
    let (call, stream) = local.proxy::<Host>().spawn_terminal(id, term_size(size), cwd.clone());
    on_io();
    RemotePty { local, id, cwd: cwd.into(), call, stream, unread: Bytes::new(), on_io }
  }

  /// The name this terminal goes by in calls to the host.
  pub fn id(&self) -> u64 { self.id }

  fn closed(&mut self, err: Option<String>) -> io::Result<usize> {
    if let Some(err) = err {
      error!("remote terminal {} ended: {err}", self.id);
    }
    Ok(0)
  }
}

fn term_size(size: Size) -> TermSize {
  TermSize {
    rows: size.rows.min(u16::MAX as usize) as u16,
    cols: size.cols.min(u16::MAX as usize) as u16,
  }
}

impl PtyHost for RemotePty {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    if self.unread.is_empty() {
      match self.stream.try_recv() {
        Ok(chunk) => self.unread = chunk,
        Err(TryRecvError::Empty) => {
          // The call answers early only when the shell could not start.
          return match self.call.try_take() {
            Some(Err(CallError::App(e))) => self.closed(Some(format!("{e:?}"))),
            Some(Err(CallError::Rpc(e))) => self.closed(Some(format!("{e:?}"))),
            Some(Ok(())) => self.closed(None),
            None => Err(io::ErrorKind::WouldBlock.into()),
          };
        }
        Err(TryRecvError::Closed(e)) => return self.closed(e.map(|e| format!("{e:?}"))),
      }
    }

    let n = self.unread.len().min(buf.len());
    buf[..n].copy_from_slice(&self.unread[..n]);
    self.unread = self.unread.slice(n..);
    Ok(n)
  }

  fn write_bytes(&mut self, bytes: &[u8]) {
    let _ = self.local.proxy::<Host>().terminal_input(self.id, Bytes::copy_from_slice(bytes));
    (self.on_io)();
  }

  // Queued bytes live in the connection, which the owner already flushes.
  fn has_pending_writes(&self) -> bool { false }
  fn flush(&mut self) {}

  fn resize(&mut self, size: Size) {
    let _ = self.local.proxy::<Host>().terminal_resize(self.id, term_size(size));
    (self.on_io)();
  }

  /// Where the shell was started. The host does not report where it has since
  /// been `cd`ed to, so this is not the shell's current directory.
  fn current_dir(&self) -> PathBuf { self.cwd.clone() }

  /// Output arrives with the connection's own wakeups, which already redraw.
  fn set_wake(&mut self, _wake: Box<dyn Fn() + Send>) {}
}
