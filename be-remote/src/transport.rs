use std::{io, rc::Rc};

use lode_local::{Local, RpcError};

use crate::HostError;

/// The connection, as the remote filesystem and git state see it. Calls over it
/// never wait: they return a pending answer, and the connection's owner
/// delivers it as it drives the connection.
#[derive(Clone)]
pub struct Transport {
  pub(crate) local: Local,
  /// Run after anything that queues bytes, so the owner can start waiting for
  /// the socket to drain.
  on_io:            Rc<dyn Fn()>,
}

impl Transport {
  pub fn new(local: Local, on_io: Rc<dyn Fn()>) -> Transport { Transport { local, on_io } }

  pub fn local(&self) -> &Local { &self.local }

  pub(crate) fn io_done(&self) { (self.on_io)(); }
}

pub(crate) fn io_error(e: HostError) -> io::Error {
  match e {
    HostError::NotFound => io::ErrorKind::NotFound.into(),
    HostError::PermissionDenied => io::ErrorKind::PermissionDenied.into(),
    HostError::NotUtf8 => io::ErrorKind::InvalidData.into(),
    HostError::Io(s) => io::Error::other(s),
  }
}

pub(crate) fn rpc_error(e: RpcError) -> io::Error {
  match e {
    RpcError::Disconnected => io::Error::new(io::ErrorKind::BrokenPipe, "host disconnected"),
    e => io::Error::other(format!("{e:?}")),
  }
}
