use std::{
  fs::File,
  io::{self, Read, Write},
  os::{
    fd::{AsFd, AsRawFd, BorrowedFd},
    unix::process::CommandExt,
  },
  process::Command,
};

use crate::Size;

pub struct Pty {
  _child:    std::process::Child,
  pty:       File,
  write_buf: Vec<u8>,
}

impl Pty {
  pub fn new(size: Size) -> Self {
    let pty = rustix_openpty::openpty(
      None,
      Some(&rustix::termios::Winsize {
        ws_row: size.rows as u16,
        ws_col: size.cols as u16,
        ..unsafe { std::mem::zeroed() }
      }),
    )
    .unwrap();

    let mut cmd = Command::new("/bin/zsh");

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    cmd.stdin(pty.user.try_clone().unwrap());
    cmd.stdout(pty.user.try_clone().unwrap());
    cmd.stderr(pty.user.try_clone().unwrap());

    unsafe {
      let user = pty.user.as_raw_fd();
      let controller = pty.controller.as_raw_fd();
      cmd.pre_exec(move || {
        // Create a new process group.
        let err = libc::setsid();
        if err == -1 {
          return Err(io::Error::other("Failed to set session id"));
        }

        // Needed for control signals to work.
        libc::ioctl(user, libc::TIOCSCTTY as _);

        // No longer need user/controller fds.
        libc::close(user);
        libc::close(controller);

        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
        libc::signal(libc::SIGHUP, libc::SIG_DFL);
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGQUIT, libc::SIG_DFL);
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGALRM, libc::SIG_DFL);

        Ok(())
      });
    }
    let child = cmd.spawn().unwrap();

    be_async::set_nonblocking(&pty.controller).unwrap();

    Pty { _child: child, pty: File::from(pty.controller), write_buf: Vec::new() }
  }

  pub fn fd(&self) -> BorrowedFd<'_> { self.pty.as_fd() }

  pub fn resize(&mut self, size: Size) {
    rustix::termios::tcsetwinsize(
      &self.pty,
      rustix::termios::Winsize {
        ws_row: size.rows as u16,
        ws_col: size.cols as u16,
        ..unsafe { std::mem::zeroed() }
      },
    )
    .unwrap();
  }

  pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> { self.pty.read(buf) }

  pub fn write(&mut self, args: std::fmt::Arguments) { self.write_buf.write_fmt(args).unwrap(); }
  pub fn write_bytes(&mut self, bytes: &[u8]) { self.write_buf.extend_from_slice(bytes); }

  pub fn has_pending_writes(&self) -> bool { !self.write_buf.is_empty() }

  pub fn flush(&mut self) {
    while !self.write_buf.is_empty() {
      match self.pty.write(&self.write_buf) {
        Ok(n) => {
          self.write_buf.drain(..n);
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        Err(e) => {
          self.write_buf.clear();
          fatal!("failed to write to pty: {e}");
          break;
        }
      }
    }
  }
}
