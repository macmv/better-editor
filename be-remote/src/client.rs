use std::{io, process::Command, time::Instant};

use lode_local::Pending;
use lode_poll::{
  Connection,
  sys::{self, PollFd},
};

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

/// Quotes `s` as one word for a POSIX shell.
fn shell_quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

/// `ssh <target> 'cd <dir> && exec <cmd>'`, with the same options as
/// `ssh_command`. Used to run a language server next to the files it reads;
/// its stdio is the command's stdio. `cmd` is run by the remote shell, so it
/// may carry arguments.
pub fn ssh_exec_command(target: &str, dir: &std::path::Path, cmd: &str) -> Command {
  let mut ssh = Command::new("ssh");
  ssh.args(["-T", "-o", "BatchMode=yes", "--", target]);
  ssh.arg(format!("cd {} && exec {cmd}", shell_quote(&dir.to_string_lossy())));
  ssh
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

  /// Waits up to `max_ms` for the connection to have something to do, and does
  /// it. For owners with no poll loop of their own, and for blocking calls.
  /// Returns false once the connection has closed.
  pub fn pump(&self, max_ms: i32) -> bool {
    let conn = &self.conn;
    if conn.is_finished() {
      return false;
    }

    let mut fds: Vec<_> = conn
      .read_fds()
      .into_iter()
      .map(|fd| PollFd { fd, readable: true, writable: false })
      .collect();
    fds.extend(conn.write_fds().into_iter().map(|fd| PollFd {
      fd,
      readable: false,
      writable: true,
    }));

    let timeout = match conn.poll_timeout_ms(Instant::now()) {
      t if t < 0 => max_ms,
      t => t.min(max_ms),
    };
    let ready = match sys::poll(&fds, timeout) {
      Ok(r) => r,
      Err(e) if e.kind() == io::ErrorKind::Interrupted => return true,
      Err(_) => return false,
    };

    let now = Instant::now();
    if ready.iter().zip(&fds).any(|(r, f)| r.readable && f.readable) {
      conn.handle_readable(now);
    }
    if ready.iter().zip(&fds).any(|(r, f)| r.writable && f.writable) {
      conn.handle_writable();
    }
    conn.handle_timeout(now);
    while conn.poll_event().is_some() {}

    !conn.is_finished()
  }

  pub fn connection(&self) -> &Connection { &self.conn }
  pub fn connection_mut(&mut self) -> &mut Connection { &mut self.conn }

  pub fn info(&self) -> Pending<HostInfo, HostError> { self.conn.local().proxy::<Host>().info() }

  pub fn list_dir(&self, path: &str) -> Pending<Vec<DirEntry>, HostError> {
    self.conn.local().proxy::<Host>().list_dir(path.to_string())
  }

  pub fn canonicalize(&self, path: &str) -> Pending<String, HostError> {
    self.conn.local().proxy::<Host>().canonicalize(path.to_string())
  }

  /// The directory terminal `id`'s shell is in now.
  pub fn terminal_cwd(&self, id: u64) -> Pending<String, HostError> {
    self.conn.local().proxy::<Host>().terminal_cwd(id)
  }

  /// The commit `root` is at, or `None` if it isn't a git repository.
  pub fn git_head(&self, root: &str) -> Pending<Option<String>, HostError> {
    self.conn.local().proxy::<Host>().git_head(root.to_string())
  }

  pub fn read_file(&self, path: &str) -> Pending<String, HostError> {
    self.conn.local().proxy::<Host>().read_file(path.to_string())
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::*;

  #[test]
  fn exec_command_quotes_the_directory() {
    let cmd = ssh_exec_command("me@box", Path::new("/home/a b/it's"), "rust-analyzer");
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(
      args,
      [
        "-T",
        "-o",
        "BatchMode=yes",
        "--",
        "me@box",
        r"cd '/home/a b/it'\''s' && exec rust-analyzer"
      ]
    );
  }
}
