use std::{
  fs::File,
  io::{self, Read, Write},
  os::{
    fd::{AsRawFd, RawFd},
    unix::process::CommandExt,
  },
  path::{Path, PathBuf},
  process::Command,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread::JoinHandle,
  time::Duration,
};

use crate::{PtyHost, Size};

/// A PTY backed by a local shell process.
pub struct Pty {
  _child:    std::process::Child,
  pty:       File,
  write_buf: Vec<u8>,
  waker:     Option<Waker>,
}

/// A thread that calls a wake function whenever the pty becomes readable.
struct Waker {
  poller: Arc<polling::Poller>,
  stop:   Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
}

impl AsRawFd for Pty {
  /// The controller end, for callers that run their own poll loop.
  fn as_raw_fd(&self) -> RawFd { self.pty.as_raw_fd() }
}

impl Pty {
  pub fn new(size: Size, path: &Path) -> Self {
    Pty::try_new(size, path).expect("failed to start a shell")
  }

  /// Starts `$SHELL` (or `/bin/zsh`) in `path`.
  pub fn try_new(size: Size, path: &Path) -> io::Result<Self> {
    let pty = rustix_openpty::openpty(
      None,
      Some(&rustix::termios::Winsize {
        ws_row: size.rows as u16,
        ws_col: size.cols as u16,
        ..unsafe { std::mem::zeroed() }
      }),
    )
    .map_err(io::Error::from)?;

    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let mut cmd = Command::new(shell);

    cmd.current_dir(path);

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    cmd.stdin(pty.user.try_clone()?);
    cmd.stdout(pty.user.try_clone()?);
    cmd.stderr(pty.user.try_clone()?);

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
    let child = cmd.spawn()?;

    be_async::set_nonblocking(&pty.controller)?;

    Ok(Pty {
      _child:    child,
      pty:       File::from(pty.controller),
      write_buf: Vec::new(),
      waker:     None,
    })
  }
}

impl PtyHost for Pty {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> { self.pty.read(buf) }

  fn write_bytes(&mut self, bytes: &[u8]) { self.write_buf.extend_from_slice(bytes); }

  fn has_pending_writes(&self) -> bool { !self.write_buf.is_empty() }

  fn flush(&mut self) {
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

  fn resize(&mut self, size: Size) {
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

  fn current_dir(&self) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
      std::fs::read_link(format!("/proc/{}/cwd", self._child.id())).unwrap()
    }
    #[cfg(not(target_os = "linux"))]
    {
      warn!("current_dir not implemented on platform");
      std::env::current_dir().unwrap()
    }
  }

  fn set_wake(&mut self, wake: Box<dyn Fn() + Send>) {
    // Stop any previous thread first.
    self.waker = None;

    // The thread owns its own handle to the pty, so the fd stays valid for as
    // long as it is registered with the poller.
    let file = self.pty.try_clone().unwrap();
    let poller = Arc::new(polling::Poller::new().unwrap());
    let stop = Arc::new(AtomicBool::new(false));

    // SAFETY: `file` outlives the registration, as it is deleted before the
    // thread exits.
    unsafe {
      poller.add(&file, polling::Event::readable(0)).unwrap();
    }

    let thread = {
      let poller = poller.clone();
      let stop = stop.clone();
      std::thread::spawn(move || {
        let mut events = polling::Events::new();
        while !stop.load(Ordering::Acquire) {
          events.clear();
          poller.wait(&mut events, None).unwrap();
          if stop.load(Ordering::Acquire) {
            break;
          }
          if events.is_empty() {
            continue;
          }

          wake();

          // We only need to wake it up once per frame, so don't spam wake ups.
          std::thread::sleep(Duration::from_millis(10));
          if poller.modify(&file, polling::Event::readable(0)).is_err() {
            break;
          }
        }

        let _ = poller.delete(&file);
      })
    };

    self.waker = Some(Waker { poller, stop, thread: Some(thread) });
  }
}

impl Drop for Waker {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    let _ = self.poller.notify();
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
  }
}
