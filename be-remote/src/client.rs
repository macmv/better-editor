use std::{io, process::Command, time::Instant};

use lode_local::Pending;
use lode_poll::Connection;

use crate::{DirEntry, Host, HostError, HostInfo};

/// `ssh <target> <host_cmd> --stdio`. `target` is anything `ssh` accepts, such
/// as `user@machine`.
pub fn ssh_command(target: &str, host_cmd: &str) -> Command {
  let mut cmd = Command::new("ssh");
  // No tty: the channel is binary. BatchMode: a password prompt would hang
  // the editor, so fail instead.
  cmd.args(["-T", "-o", "BatchMode=yes", "--", target, host_cmd, "--stdio"]);
  cmd
}

/// A connection to a `be-host`. The owner drives `connection()` from its poll
/// loop (see `lode_poll::Connection`); calls return pending values that
/// complete as the loop runs.
pub struct RemoteHost {
  conn: Connection,
}

impl RemoteHost {
  /// Dials over the stdio of `cmd`.
  pub fn spawn(cmd: Command, now: Instant) -> io::Result<RemoteHost> {
    Ok(RemoteHost { conn: Connection::spawn(cmd, now)? })
  }

  pub fn connect_ssh(target: &str, host_cmd: &str, now: Instant) -> io::Result<RemoteHost> {
    RemoteHost::spawn(ssh_command(target, host_cmd), now)
  }

  pub fn connection(&self) -> &Connection { &self.conn }
  pub fn connection_mut(&mut self) -> &mut Connection { &mut self.conn }

  pub fn info(&self) -> Pending<HostInfo, HostError> { self.conn.local().proxy::<Host>().info() }

  pub fn list_dir(&self, path: &str) -> Pending<Vec<DirEntry>, HostError> {
    self.conn.local().proxy::<Host>().list_dir(path.to_string())
  }

  pub fn read_file(&self, path: &str) -> Pending<String, HostError> {
    self.conn.local().proxy::<Host>().read_file(path.to_string())
  }
}
