//! Runs the real `be-host` binary over a pipe and drives the remote
//! filesystem against it.

use std::{
  cell::RefCell,
  path::{Path, PathBuf},
  process::Command,
  rc::Rc,
  time::{Duration, Instant},
};

use be_fs::{Fs, Watcher};
use be_git::GitBackend;
use be_remote::{RemoteFs, RemoteGit, RemoteHost, RemoteWatcher, Transport};

struct Session {
  host: Rc<RefCell<RemoteHost>>,
  t:    Transport,
}

fn connect() -> Session {
  let mut cmd = Command::new(env!("CARGO_BIN_EXE_be-host"));
  cmd.arg("--stdio");
  let host = Rc::new(RefCell::new(RemoteHost::spawn(cmd, Instant::now()).unwrap()));
  let local = host.borrow().connection().local().clone();
  let pump = {
    let host = host.clone();
    Rc::new(move || host.borrow().pump(50)) as Rc<dyn Fn() -> bool>
  };
  Session { t: Transport::new(local, Rc::new(|| {}), pump), host }
}

fn temp_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("be-host-test-{name}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir.canonicalize().unwrap()
}

#[test]
fn fs_round_trip() {
  let dir = temp_dir("fs");
  std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
  std::fs::create_dir(dir.join("sub")).unwrap();

  let s = connect();
  let fs = RemoteFs::new(s.t.clone());

  let mut read = fs.begin_read(&dir.join("a.txt"));
  let data = loop {
    if let Some(res) = read.poll() {
      break res.unwrap();
    }
    s.host.borrow().pump(50);
  };
  assert_eq!(data.data, b"hello\n");

  let mut write = fs.begin_write(&dir.join("a.txt"), b"bye\n".to_vec());
  let deadline = Instant::now() + Duration::from_secs(10);
  let written = loop {
    if let Some(res) = write.poll() {
      break res.unwrap();
    }
    assert!(Instant::now() < deadline, "the write never finished");
    s.host.borrow().pump(50);
  };
  assert!(written.mtime >= written.prev_mtime);
  assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"bye\n");

  let mut list = fs.begin_read_dir(&dir);
  let entries = loop {
    if let Some(res) = list.poll() {
      break res.unwrap();
    }
    s.host.borrow().pump(50);
  };
  let mut names: Vec<_> = entries.into_iter().map(|e| e.name).collect();
  names.sort();
  assert_eq!(names, ["a.txt", "sub"]);

  let mut read = fs.begin_read(&dir.join("missing"));
  let err = loop {
    if let Some(res) = read.poll() {
      break res.unwrap_err();
    }
    s.host.borrow().pump(50);
  };
  assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

  drop(s.host);
  let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_reports_changes() {
  let dir = temp_dir("watch");
  let s = connect();

  let mut watcher = RemoteWatcher::new(&s.t, 1, &dir);
  let deadline = Instant::now() + Duration::from_secs(10);
  let mut next = || {
    let mut seen = vec![];
    while Instant::now() < deadline && seen.is_empty() {
      s.host.borrow().pump(50);
      seen = watcher.poll().iter().map(|p| p.to_string()).collect::<Vec<_>>();
    }
    seen
  };

  // The host says the root changed once the watch is ready, so nothing that
  // happens while it starts is missed.
  assert_eq!(next(), [""]);

  std::fs::write(dir.join("new.txt"), "x").unwrap();
  assert_eq!(next(), ["new.txt"]);

  let _ = std::fs::remove_dir_all(&dir);
}

fn git(dir: &Path, args: &[&str]) {
  let status = Command::new("git")
    .args(["-c", "user.name=t", "-c", "user.email=t@t"])
    .args(args)
    .current_dir(dir)
    .status()
    .unwrap();
  assert!(status.success(), "git {args:?} failed");
}

#[test]
fn git_state() {
  let dir = temp_dir("git");
  git(&dir, &["init", "-q"]);
  std::fs::write(dir.join("a.txt"), "one\n").unwrap();
  std::fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
  git(&dir, &["add", "."]);
  git(&dir, &["commit", "-q", "-m", "init"]);

  let s = connect();
  let git = RemoteGit::new(s.t.clone(), &dir);

  // Nothing is known until the host answers.
  assert!(git.head().is_none());
  let deadline = Instant::now() + Duration::from_secs(10);
  let wait_for = |what: &str, mut done: &mut dyn FnMut() -> bool| {
    while !done() {
      assert!(Instant::now() < deadline, "{what} never arrived");
      s.host.borrow().pump(50);
    }
  };
  wait_for("head", &mut || git.head().is_some());

  let lookup = |name: &str| {
    let task = git.begin_lookup_in_head(&dir.join(name));
    let mut out = None;
    wait_for("lookup", &mut || {
      out = task.completed();
      out.is_some()
    });
    out.unwrap()
  };
  assert_eq!(lookup("a.txt").as_deref(), Some("one\n"));
  assert_eq!(lookup("nope.txt"), None);

  std::fs::write(dir.join("a.txt"), "two\n").unwrap();
  std::fs::write(dir.join("b.txt"), "new\n").unwrap();
  std::fs::write(dir.join("ignored.txt"), "x\n").unwrap();

  let status = |name: &str| {
    let mut out = None;
    wait_for("status", &mut || {
      out = git.status(&dir.join(name));
      out.is_some()
    });
    out.unwrap()
  };
  let a = status("a.txt");
  assert!(a.modified && !a.added);
  assert!(status("b.txt").added);
  assert!(status("ignored.txt").ignored);

  let _ = std::fs::remove_dir_all(&dir);
}
