use std::{
  fs::File,
  io::{self, Read, Write},
  os::{
    fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    unix::process::CommandExt,
  },
  process::Command,
};

use crate::Size;

pub struct Pty {
  _child: std::process::Child,
  user:   OwnedFd,
  pty:    File,
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

    Pty { _child: child, user: pty.user, pty: File::from(pty.controller) }
  }

  pub fn fd(&self) -> BorrowedFd<'_> { self.pty.as_fd() }

  pub fn resize(&mut self, size: Size) {
    rustix::termios::tcsetwinsize(
      &self.user,
      rustix::termios::Winsize {
        ws_row: size.rows as u16,
        ws_col: size.cols as u16,
        ..unsafe { std::mem::zeroed() }
      },
    )
    .unwrap();
  }

  pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> { self.pty.read(buf) }

  pub fn input(&mut self, c: char) { write!(self.pty, "{c}").unwrap(); }
}
