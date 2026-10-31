//! A connection to a `be-host`, driven from the winit loop.
//!
//! Lode's poll driver wants its owner to wait on its fds, but winit owns the
//! wait. A `Poller` thread bridges the two: it only polls the connection's raw
//! fds and posts a `RemoteWake` event, and everything else (the connection and
//! every call) stays on the GUI thread.
//!
//! The thread is oneshot, like `epoll` with `EPOLLONESHOT`: after posting a
//! wake it sleeps until the GUI thread calls `rearm` with the connection's
//! current interest. So `rearm` must follow anything that can change that
//! interest, which is every call and every `handle_*`.

use std::{
  cell::{Cell, RefCell},
  io::{self, Read, Write},
  os::{
    fd::{AsRawFd, RawFd},
    unix::net::UnixStream,
  },
  path::{Path, PathBuf},
  process::Command,
  rc::Rc,
  sync::{Arc, Condvar, Mutex},
  thread::JoinHandle,
  time::{Duration, Instant},
};

use be_remote::{
  DirEntry, HostError, HostInfo, RemoteFs, RemoteGit, RemoteHost, RemotePty, RemoteWatcher,
  Transport,
};
use be_workspace::Backend;
use lode_local::{CallError, Pending, RpcError};
use lode_poll::sys::{self, PollFd};

/// What the poller thread saw. Handed to `RemoteSession::on_wake`.
#[derive(Debug, Clone, Copy)]
pub struct RemoteWake {
  readable: bool,
  writable: bool,
}

/// `ssh://[user@]host[/path]`, or a bare `[user@]host`.
#[derive(Debug, PartialEq)]
pub struct RemoteTarget {
  pub host: String,
  /// `None`: the host's working directory.
  pub path: Option<String>,
}

impl RemoteTarget {
  pub fn parse(s: &str) -> RemoteTarget {
    let rest = s.strip_prefix("ssh://").unwrap_or(s);
    match rest.split_once('/') {
      Some((host, path)) => RemoteTarget { host: host.into(), path: Some(format!("/{path}")) },
      None => RemoteTarget { host: rest.into(), path: None },
    }
  }
}

#[derive(Default, Clone)]
struct Interest {
  read:     Vec<RawFd>,
  write:    Vec<RawFd>,
  deadline: Option<Instant>,
}

struct Shared {
  armed:    bool,
  stop:     bool,
  interest: Interest,
}

struct Poller {
  shared: Arc<(Mutex<Shared>, Condvar)>,
  wake:   UnixStream,
  thread: Option<JoinHandle<()>>,
}

impl Poller {
  fn new(on_wake: impl Fn(RemoteWake) + Send + 'static) -> io::Result<Poller> {
    let (wake, wake_rx) = UnixStream::pair()?;
    wake.set_nonblocking(true)?;
    wake_rx.set_nonblocking(true)?;

    let shared = Arc::new((
      Mutex::new(Shared { armed: false, stop: false, interest: Interest::default() }),
      Condvar::new(),
    ));
    let thread = {
      let shared = shared.clone();
      std::thread::Builder::new()
        .name("remote-poller".into())
        .spawn(move || poll_thread(&shared, wake_rx, on_wake))?
    };
    Ok(Poller { shared, wake, thread: Some(thread) })
  }

  fn rearm(&self, interest: Interest) {
    let (lock, cv) = &*self.shared;
    {
      let mut s = lock.lock().unwrap();
      s.interest = interest;
      s.armed = true;
    }
    cv.notify_one();
    self.poke();
  }

  fn poke(&self) {
    // A full pipe already has a wake pending.
    let _ = (&self.wake).write(&[1]);
  }
}

impl Drop for Poller {
  fn drop(&mut self) {
    let (lock, cv) = &*self.shared;
    lock.lock().unwrap().stop = true;
    cv.notify_one();
    self.poke();
    if let Some(t) = self.thread.take() {
      let _ = t.join();
    }
  }
}

fn poll_thread(
  shared: &(Mutex<Shared>, Condvar),
  mut wake_rx: UnixStream,
  on_wake: impl Fn(RemoteWake),
) {
  let (lock, cv) = shared;
  loop {
    let interest = {
      let mut s = lock.lock().unwrap();
      while !s.armed && !s.stop {
        s = cv.wait(s).unwrap();
      }
      if s.stop {
        return;
      }
      s.interest.clone()
    };

    let mut fds = vec![PollFd { fd: wake_rx.as_raw_fd(), readable: true, writable: false }];
    fds.extend(interest.read.iter().map(|&fd| PollFd { fd, readable: true, writable: false }));
    fds.extend(interest.write.iter().map(|&fd| PollFd { fd, readable: false, writable: true }));

    let timeout = match interest.deadline {
      None => -1,
      Some(d) => {
        let left = d.saturating_duration_since(Instant::now());
        left.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32
      }
    };
    let ready = match sys::poll(&fds, timeout) {
      Ok(r) => r,
      Err(e) => {
        error!("remote poller: poll failed: {e}");
        return;
      }
    };

    if ready[0].readable {
      // The interest changed (or we are stopping): start over with it.
      let mut buf = [0u8; 64];
      while matches!(wake_rx.read(&mut buf), Ok(n) if n > 0) {}
      continue;
    }

    let reads = interest.read.len();
    let readable = ready[1..1 + reads].iter().any(|r| r.readable);
    let writable = ready[1 + reads..].iter().any(|r| r.writable);
    let timed_out = interest.deadline.is_some_and(|d| Instant::now() >= d);
    if !(readable || writable || timed_out) {
      continue;
    }

    lock.lock().unwrap().armed = false;
    on_wake(RemoteWake { readable, writable });
  }
}

/// A dropped connection is reported once by `on_closed`, not once per call.
fn log_call_error(what: &str, e: &CallError<HostError>) {
  if !matches!(e, CallError::Rpc(RpcError::Disconnected)) {
    error!("remote: {what} failed: {e:?}");
  }
}

enum Stage {
  Info(Pending<HostInfo, HostError>),
  List(String, Pending<Vec<DirEntry>, HostError>),
  Done,
  Failed,
}

/// The connection and the thread watching its fds. Shared by the session and
/// every terminal on the connection, since a terminal's writes must re-arm the
/// poller too.
struct Link {
  // Drops first: the thread must stop polling before the fds close.
  poller:  Poller,
  host:    RefCell<RemoteHost>,
  /// What `ssh` was told to connect to, for starting other things on the same
  /// machine.
  target:  String,
  /// Names this connection's terminals and watches.
  next_id: Cell<u64>,
}

impl Link {
  /// Tells the poller thread what the connection waits on now.
  fn rearm(&self) {
    let host = self.host.borrow();
    let conn = host.connection();
    self.poller.rearm(Interest {
      read:     conn.read_fds(),
      write:    conn.write_fds(),
      deadline: conn.next_deadline(),
    });
  }
}

/// A live connection to a host, and what the editor is asking of it.
pub struct RemoteSession {
  link:     Rc<Link>,
  host_cmd: String,
  path:     Option<String>,
  stage:    Stage,
  reported: bool,
}

/// Starts things on a host: terminals, and workspaces. Cheap to clone, and
/// keeps the connection alive.
#[derive(Clone)]
pub struct RemoteHandle {
  link: Rc<Link>,
}

impl RemoteHandle {
  /// What `ssh` connects to, such as `user@box`.
  pub fn target(&self) -> &str { &self.link.target }

  fn next_id(&self) -> u64 {
    let id = self.link.next_id.get();
    self.link.next_id.set(id + 1);
    id
  }

  /// Blocking calls over this connection. These stall the editor for a round
  /// trip, so they're for startup and for rare actions.
  fn transport(&self) -> Transport {
    let local = self.link.host.borrow().connection().local().clone();
    let on_io = {
      let link = self.link.clone();
      Rc::new(move || link.rearm())
    };
    let pump = {
      let link = self.link.clone();
      Rc::new(move || link.host.borrow().pump(50)) as Rc<dyn Fn() -> bool>
    };
    Transport::new(local, on_io, pump)
  }

  pub fn fs(&self) -> RemoteFs { RemoteFs::new(self.transport()) }

  /// A shell on the host, in `cwd` (empty: the host's working directory).
  pub fn new_pty(&self, size: be_terminal::Size, cwd: String) -> RemotePty {
    let id = self.next_id();
    let local = self.link.host.borrow().connection().local().clone();
    let link = self.link.clone();
    RemotePty::new(local, id, size, cwd, Rc::new(move || link.rearm()))
  }

  /// Starts opening the workspace at `path` on the host. Nothing blocks:
  /// `OpeningWorkspace::poll` finishes it as the host answers.
  pub fn open_workspace(&self, path: &Path) -> OpeningWorkspace {
    let call = self.link.host.borrow().canonicalize(&path.to_string_lossy());
    self.link.rearm();
    OpeningWorkspace { handle: self.clone(), stage: OpenStage::Canonicalize(call) }
  }

  /// The pieces of a workspace rooted at `root` on the host. `root` must
  /// already be canonical there. Without a repository, the editor diffs
  /// against what it opened instead.
  fn backend(&self, root: &Path, has_git: bool) -> Backend {
    let t = self.transport();
    let git = has_git.then(|| Box::new(RemoteGit::new(t.clone(), root)) as _);
    let target = self.link.target.clone();
    Backend {
      fs: Rc::new(RemoteFs::new(t.clone())),
      git,
      watcher: Box::new(RemoteWatcher::new(&t, self.next_id(), root)),
      launcher: Box::new(move |cmd, dir| be_remote::ssh_exec_command(&target, dir, cmd)),
    }
  }

  /// Every file under `root` that git would not ignore, relative to `root`.
  pub fn list_files(&self, root: &Path) -> io::Result<Vec<String>> { self.fs().list_files(root) }
}

enum OpenStage {
  Canonicalize(Pending<String, HostError>),
  Head(PathBuf, Pending<Option<String>, HostError>),
}

/// A workspace being opened on the host: its path is made canonical, then its
/// repository is looked for.
pub struct OpeningWorkspace {
  handle: RemoteHandle,
  stage:  OpenStage,
}

impl OpeningWorkspace {
  pub fn handle(&self) -> &RemoteHandle { &self.handle }

  /// The canonical path and the workspace's pieces, once the host has
  /// answered. Never blocks.
  pub fn poll(&mut self) -> Option<io::Result<(PathBuf, Backend)>> {
    loop {
      match &mut self.stage {
        OpenStage::Canonicalize(call) => {
          let res = call.try_take()?;
          let path = match res {
            Ok(path) => PathBuf::from(path),
            Err(e) => return Some(Err(call_error(e))),
          };
          let head = self.handle.link.host.borrow().git_head(&path.to_string_lossy());
          self.handle.link.rearm();
          self.stage = OpenStage::Head(path, head);
        }
        OpenStage::Head(path, call) => {
          let res = call.try_take()?;
          let path = path.clone();
          return Some(match res {
            Ok(head) => {
              let backend = self.handle.backend(&path, head.is_some());
              Ok((path, backend))
            }
            Err(e) => Err(call_error(e)),
          });
        }
      }
    }
  }
}

fn call_error(e: CallError<HostError>) -> io::Error {
  match e {
    CallError::App(HostError::NotFound) => io::ErrorKind::NotFound.into(),
    CallError::App(HostError::PermissionDenied) => io::ErrorKind::PermissionDenied.into(),
    e => io::Error::other(format!("{e:?}")),
  }
}

impl RemoteSession {
  pub fn connect(
    target: RemoteTarget,
    host_cmd: &str,
    on_wake: impl Fn(RemoteWake) + Send + 'static,
  ) -> io::Result<RemoteSession> {
    let host = RemoteHost::connect_ssh(&target.host, host_cmd, Instant::now())?;
    RemoteSession::new(host, target.host, host_cmd.into(), target.path, on_wake)
  }

  /// `Command` form of `connect`, for tests and unusual transports.
  #[allow(dead_code)]
  pub fn spawn(
    cmd: Command,
    on_wake: impl Fn(RemoteWake) + Send + 'static,
  ) -> io::Result<RemoteSession> {
    RemoteSession::new(
      RemoteHost::spawn(cmd, Instant::now())?,
      String::new(),
      String::new(),
      None,
      on_wake,
    )
  }

  fn new(
    host: RemoteHost,
    target: String,
    host_cmd: String,
    path: Option<String>,
    on_wake: impl Fn(RemoteWake) + Send + 'static,
  ) -> io::Result<RemoteSession> {
    let stage = Stage::Info(host.info());
    let link = Rc::new(Link {
      poller: Poller::new(on_wake)?,
      host: RefCell::new(host),
      target,
      next_id: Cell::new(1),
    });
    link.rearm();
    Ok(RemoteSession { link, host_cmd, path, stage, reported: false })
  }

  /// The directory the session was pointed at, if it was given one.
  pub fn path(&self) -> Option<&str> { self.path.as_deref() }

  pub fn handle(&self) -> RemoteHandle { RemoteHandle { link: self.link.clone() } }

  /// A shell on the host, in `cwd` (empty: the host's working directory).
  pub fn new_pty(&mut self, size: be_terminal::Size, cwd: String) -> RemotePty {
    self.handle().new_pty(size, cwd)
  }

  pub fn on_wake(&mut self, wake: RemoteWake) {
    let now = Instant::now();
    {
      let host = self.link.host.borrow();
      let conn = host.connection();
      if wake.readable {
        conn.handle_readable(now);
      }
      if wake.writable {
        conn.handle_writable();
      }
      conn.handle_timeout(now);
      while conn.poll_event().is_some() {}
    }

    self.advance();

    if self.link.host.borrow().connection().is_finished() {
      self.on_closed();
      // Nothing left to wait for: the poller stays asleep.
      return;
    }
    self.link.rearm();
  }

  /// The connection is gone. Says why once, in terms of what the user can fix.
  fn on_closed(&mut self) {
    if std::mem::replace(&mut self.reported, true) {
      return;
    }
    if !matches!(self.stage, Stage::Done) {
      self.stage = Stage::Failed;
    }

    let mut host = self.link.host.borrow_mut();
    debug!("remote: connection closed: {:?}", host.connection().close_reason());

    // The pipe closing can beat the child's exit by a moment.
    let mut status = None;
    for _ in 0..50 {
      match host.connection_mut().child_mut().map(|c| c.try_wait()) {
        Some(Ok(Some(s))) => {
          status = Some(s);
          break;
        }
        Some(Ok(None)) => std::thread::sleep(Duration::from_millis(10)),
        _ => break,
      }
    }

    match status.and_then(|s| s.code()) {
      Some(127) => error!(
        "remote: `{cmd}` was not found on the remote machine. Non-interactive ssh has a minimal \
         PATH, so install it there or pass --remote-host-cmd with an absolute path (such as \
         ~/.cargo/bin/be-host)",
        cmd = self.host_cmd,
      ),
      Some(255) => error!("remote: ssh could not connect (exit 255); see its output above"),
      Some(code) => error!("remote: the host exited with status {code} before answering"),
      None => error!("remote: the connection to the host was lost"),
    }
  }

  fn advance(&mut self) {
    match &mut self.stage {
      Stage::Info(p) if p.is_ready() => match p.try_take().unwrap() {
        Ok(info) => {
          info!("remote: connected, protocol {}, cwd {}", info.version, info.cwd);
          let path = self.path.clone().unwrap_or(info.cwd);
          self.stage = Stage::List(path.clone(), self.link.host.borrow().list_dir(&path));
        }
        Err(e) => {
          log_call_error("info", &e);
          self.stage = Stage::Failed;
        }
      },
      Stage::List(path, p) if p.is_ready() => {
        self.stage = match p.try_take().unwrap() {
          Ok(entries) => {
            info!("remote: {path} has {} entries", entries.len());
            Stage::Done
          }
          Err(e) => {
            log_call_error("listing", &e);
            Stage::Failed
          }
        };
      }
      _ => {}
    }
  }
}

#[cfg(test)]
mod tests {
  use std::{
    os::unix::net::UnixStream,
    sync::mpsc,
    time::{Duration, Instant},
  };

  use super::*;

  const WAIT: Duration = Duration::from_secs(5);

  fn poller() -> (Poller, mpsc::Receiver<RemoteWake>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Poller::new(move |w| tx.lock().unwrap().send(w).unwrap()).unwrap(), rx)
  }

  #[test]
  fn poller_wakes_once_per_rearm() {
    let (poller, rx) = poller();
    let (a, mut b) = UnixStream::pair().unwrap();
    let interest = Interest { read: vec![a.as_raw_fd()], ..Default::default() };

    poller.rearm(interest.clone());
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

    b.write_all(&[1]).unwrap();
    let wake = rx.recv_timeout(WAIT).unwrap();
    assert!(wake.readable && !wake.writable);

    // The byte is still unread, but the poller sleeps until it is rearmed.
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    poller.rearm(interest);
    assert!(rx.recv_timeout(WAIT).unwrap().readable);
  }

  #[test]
  fn poller_wakes_on_deadline_and_on_changed_interest() {
    let (poller, rx) = poller();
    let (a, _b) = UnixStream::pair().unwrap();

    poller.rearm(Interest {
      read: vec![a.as_raw_fd()],
      deadline: Some(Instant::now() + Duration::from_millis(50)),
      ..Default::default()
    });
    let wake = rx.recv_timeout(WAIT).unwrap();
    assert!(!wake.readable && !wake.writable);

    // A socket with room in its buffer is writable at once.
    poller.rearm(Interest { write: vec![a.as_raw_fd()], ..Default::default() });
    assert!(rx.recv_timeout(WAIT).unwrap().writable);
  }

  #[test]
  fn poller_stops_on_drop() {
    let (poller, _rx) = poller();
    poller.rearm(Interest::default());
    drop(poller);
  }

  /// What ssh does when the remote command is missing: exits 127 at once.
  #[test]
  fn session_fails_cleanly_when_the_host_command_is_missing() {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "exit 127"]);
    let mut session =
      RemoteSession::spawn(cmd, move |w| tx.lock().unwrap().send(w).unwrap()).unwrap();

    let start = Instant::now();
    while !matches!(session.stage, Stage::Done | Stage::Failed) {
      assert!(start.elapsed() < WAIT, "session made no progress");
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
    }
    assert!(matches!(session.stage, Stage::Failed));
    assert!(session.link.host.borrow().connection().is_finished());
  }

  fn be_host() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let host = exe.ancestors().find_map(|d| {
      let p = d.join("be-host");
      p.is_file().then_some(p)
    });
    if host.is_none() {
      eprintln!("skipping: be-host is not built");
    }
    host
  }

  /// Typing reaches the shell, and its output comes back, with nothing driving
  /// the connection but the poller's wakes and the pty's own re-arming.
  #[test]
  fn a_session_terminal_echoes_typed_input() {
    use be_terminal::PtyHost;

    let Some(host) = be_host() else { return };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut cmd = Command::new(host);
    cmd.arg("--stdio").env("SHELL", "/bin/sh");
    let mut session =
      RemoteSession::spawn(cmd, move |w| tx.lock().unwrap().send(w).unwrap()).unwrap();

    let size = be_terminal::Size { rows: 24, cols: 80 };
    let mut pty = session.new_pty(size, "/".into());
    pty.write_bytes(b"echo marker-$((6*7))\n");

    let mut out = vec![];
    let start = Instant::now();
    while !String::from_utf8_lossy(&out).contains("marker-42") {
      assert!(start.elapsed() < WAIT, "no output: {:?}", String::from_utf8_lossy(&out));
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
      let mut buf = [0u8; 256];
      while let Ok(n) = pty.read(&mut buf) {
        if n == 0 {
          break;
        }
        out.extend_from_slice(&buf[..n]);
      }
    }

    // Typing again, long after the first burst, must wake the poller afresh.
    pty.write_bytes(b"echo second-$((1+1))\n");
    while !String::from_utf8_lossy(&out).contains("second-2") {
      assert!(start.elapsed() < WAIT, "typing stalled: {:?}", String::from_utf8_lossy(&out));
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
      let mut buf = [0u8; 256];
      while let Ok(n) = pty.read(&mut buf) {
        if n == 0 {
          break;
        }
        out.extend_from_slice(&buf[..n]);
      }
    }
  }

  /// Needs `cargo build -p be-host` first; `cargo test --workspace` does that.
  #[test]
  fn session_talks_to_a_real_host() {
    let exe = std::env::current_exe().unwrap();
    let host = exe.ancestors().find_map(|d| {
      let p = d.join("be-host");
      p.is_file().then_some(p)
    });
    let Some(host) = host else {
      eprintln!("skipping: be-host is not built");
      return;
    };

    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut cmd = Command::new(host);
    cmd.arg("--stdio");
    let mut session =
      RemoteSession::spawn(cmd, move |w| tx.lock().unwrap().send(w).unwrap()).unwrap();

    // The session reaches `Done` through nothing but wakes.
    let start = Instant::now();
    while !matches!(session.stage, Stage::Done | Stage::Failed) {
      assert!(start.elapsed() < WAIT, "session made no progress");
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
    }
    assert!(matches!(session.stage, Stage::Done));
    assert!(!session.link.host.borrow().connection().is_finished());
  }

  /// A workspace on the host opens, lists, and saves files there, with the
  /// connection driven by blocking calls alone.
  #[test]
  fn a_remote_workspace_opens_and_saves_files() {
    let Some(host) = be_host() else { return };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mut cmd = Command::new(host);
    cmd.arg("--stdio");
    let mut session =
      RemoteSession::spawn(cmd, move |w| tx.lock().unwrap().send(w).unwrap()).unwrap();
    let handle = session.handle();

    let dir = std::env::temp_dir().join(format!("be-gui-remote-ws-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.txt"), "hello\nworld\n").unwrap();
    std::fs::write(dir.join("sub/b.txt"), "b\n").unwrap();
    let dir = dir.canonicalize().unwrap();

    let mut files = handle.list_files(&dir).unwrap();
    files.sort();
    assert_eq!(files, ["a.txt", "sub/b.txt"]);

    let root = be_fs::WorkspaceRoot::remote("box".into(), dir.clone());
    let config = Rc::new(RefCell::new(be_config::Config::default()));
    let mut workspace =
      be_workspace::Workspace::with_backend(root, config, handle.backend(&dir, false));

    // The path only exists on the host, and the file arrives with a wake.
    let mut editor = workspace.open_file(&dir.join("sub/../a.txt")).unwrap();
    assert!(editor.loading());
    let start = Instant::now();
    while editor.loading() {
      assert!(start.elapsed() < WAIT, "the file never arrived");
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
      editor.layout();
    }
    assert_eq!(editor.doc().to_string(), "hello\nworld\n");
    assert_eq!(editor.file(), Some(dir.join("a.txt").as_path()));

    // Saving writes through the host, over whatever changed there meanwhile.
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    editor.save().unwrap();
    assert!(editor.saving());
    let start = Instant::now();
    while editor.saving() {
      assert!(start.elapsed() < WAIT, "the save never finished");
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
      editor.layout();
    }
    assert!(editor.status().is_some_and(|s| s.success));
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "hello\nworld\n");

    // A missing file is reported in the status line when the host answers.
    let mut missing = workspace.open_file(&dir.join("missing.txt")).unwrap();
    let start = Instant::now();
    while missing.status().is_none() {
      assert!(start.elapsed() < WAIT, "no error for a missing file");
      if let Ok(wake) = rx.recv_timeout(Duration::from_millis(50)) {
        session.on_wake(wake);
      }
      missing.layout();
    }
    assert!(!missing.loading());

    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn parse_targets() {
    let t = |s| RemoteTarget::parse(s);
    assert_eq!(t("host"), RemoteTarget { host: "host".into(), path: None });
    assert_eq!(t("me@host"), RemoteTarget { host: "me@host".into(), path: None });
    assert_eq!(t("ssh://me@host"), RemoteTarget { host: "me@host".into(), path: None });
    assert_eq!(
      t("ssh://me@host/home/me/src"),
      RemoteTarget { host: "me@host".into(), path: Some("/home/me/src".into()) }
    );
  }
}
