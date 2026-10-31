use std::{io, rc::Rc};

use lode_local::{CallError, Local, Pending, RpcError};

use crate::HostError;

/// How long a blocking call waits for the host before giving up.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The connection, as the blocking halves of the remote filesystem see it. The
/// owner of the connection supplies the two closures, since only it knows how
/// the connection is driven.
#[derive(Clone)]
pub struct Transport {
  pub(crate) local: Local,
  /// Run after anything that queues bytes, so the owner can start waiting for
  /// the socket to drain.
  on_io:            Rc<dyn Fn()>,
  /// Drives the connection once, waiting for some progress. Returns false if
  /// the connection has closed.
  pump:             Rc<dyn Fn() -> bool>,
}

impl Transport {
  pub fn new(local: Local, on_io: Rc<dyn Fn()>, pump: Rc<dyn Fn() -> bool>) -> Transport {
    Transport { local, on_io, pump }
  }

  pub fn local(&self) -> &Local { &self.local }

  pub(crate) fn io_done(&self) { (self.on_io)(); }

  /// Runs the connection until `call` is answered. This blocks the calling
  /// thread, so the editor stalls for a round trip.
  pub(crate) fn wait<T: lode::Message, E: lode::Message + Into<HostError>>(
    &self,
    mut call: Pending<T, E>,
  ) -> io::Result<T> {
    self.io_done();
    let start = std::time::Instant::now();
    loop {
      match call.try_take() {
        Some(Ok(v)) => return Ok(v),
        Some(Err(CallError::App(e))) => return Err(io_error(e.into())),
        Some(Err(CallError::Rpc(e))) => return Err(rpc_error(e)),
        None => {}
      }
      if !(self.pump)() {
        // The close may have answered the call on the way out.
        return match call.try_take() {
          Some(Ok(v)) => Ok(v),
          _ => Err(io::Error::new(io::ErrorKind::BrokenPipe, "the connection to the host closed")),
        };
      }
      if start.elapsed() > TIMEOUT {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "the host did not answer"));
      }
    }
  }
}

pub(crate) fn io_error(e: HostError) -> io::Error {
  match e {
    HostError::NotFound => io::ErrorKind::NotFound.into(),
    HostError::PermissionDenied => io::ErrorKind::PermissionDenied.into(),
    HostError::NotUtf8 => io::ErrorKind::InvalidData.into(),
    HostError::Io(s) => io::Error::other(s),
  }
}

fn rpc_error(e: RpcError) -> io::Error {
  match e {
    RpcError::Disconnected => io::Error::new(io::ErrorKind::BrokenPipe, "host disconnected"),
    e => io::Error::other(format!("{e:?}")),
  }
}
