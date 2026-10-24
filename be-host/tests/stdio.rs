//! `be-host --stdio` as a child process, dialed the way `ssh` would: over its
//! stdin and stdout.

use std::{
  fs, io,
  process::Command,
  rc::Rc,
  time::{Duration, Instant},
};

use be_remote::{DirKind, HostError, PROTOCOL_VERSION, RemoteHost, RemotePty};
use be_terminal::{PtyHost, Size};
use lode_poll::sys;

fn host() -> RemoteHost {
  let mut cmd = Command::new(env!("CARGO_BIN_EXE_be-host"));
  // A shell that behaves the same everywhere.
  cmd.arg("--stdio").env("SHELL", "/bin/sh");
  RemoteHost::spawn(cmd, Instant::now()).unwrap()
}

/// The editor's poll loop, run until `done`.
fn run_until(host: &RemoteHost, mut done: impl FnMut() -> bool) {
  let conn = host.connection();
  let start = Instant::now();
  while !done() {
    assert!(start.elapsed() < Duration::from_secs(20), "the loop made no progress");
    let mut fds: Vec<_> = conn
      .read_fds()
      .into_iter()
      .map(|fd| sys::PollFd { fd, readable: true, writable: false })
      .collect();
    fds.extend(conn.write_fds().into_iter().map(|fd| sys::PollFd {
      fd,
      readable: false,
      writable: true,
    }));
    assert!(!fds.is_empty(), "the connection closed");
    let ready = sys::poll(&fds, 20).unwrap();
    let now = Instant::now();
    if ready.iter().zip(&fds).any(|(r, f)| r.readable && f.readable) {
      conn.handle_readable(now);
    }
    if ready.iter().zip(&fds).any(|(r, f)| r.writable && f.writable) {
      conn.handle_writable();
    }
    conn.handle_timeout(now);
    while conn.poll_event().is_some() {}
  }
}

#[test]
fn info_list_and_read_over_stdio() {
  let dir = std::env::temp_dir().join(format!("be-host-test-{}", std::process::id()));
  fs::create_dir_all(dir.join("sub")).unwrap();
  fs::write(dir.join("a.txt"), "hello\n").unwrap();
  fs::write(dir.join("bad.bin"), [0xff, 0xfe]).unwrap();
  let path = |p: &str| dir.join(p).to_string_lossy().into_owned();

  let host = host();
  let mut info = host.info();
  let mut list = host.list_dir(dir.to_str().unwrap());
  let mut read = host.read_file(&path("a.txt"));
  let mut missing = host.read_file(&path("nope"));
  let mut binary = host.read_file(&path("bad.bin"));
  run_until(&host, || {
    info.is_ready() && list.is_ready() && read.is_ready() && missing.is_ready() && binary.is_ready()
  });

  assert_eq!(info.try_take().unwrap().unwrap().version, PROTOCOL_VERSION);
  let names: Vec<_> =
    list.try_take().unwrap().unwrap().into_iter().map(|e| (e.name, e.kind)).collect();
  assert_eq!(
    names,
    [
      ("a.txt".to_string(), DirKind::File),
      ("bad.bin".to_string(), DirKind::File),
      ("sub".to_string(), DirKind::Dir),
    ]
  );
  assert_eq!(read.try_take().unwrap().unwrap(), "hello\n");
  assert_eq!(
    missing.try_take().unwrap().unwrap_err(),
    lode_local::CallError::App(HostError::NotFound)
  );
  assert_eq!(
    binary.try_take().unwrap().unwrap_err(),
    lode_local::CallError::App(HostError::NotUtf8)
  );

  fs::remove_dir_all(dir).unwrap();
}

const SIZE: Size = Size { rows: 24, cols: 80 };

fn terminal(host: &RemoteHost, id: u64, cwd: &str) -> RemotePty {
  RemotePty::new(host.connection().local().clone(), id, SIZE, cwd.into(), Rc::new(|| {}))
}

/// Reads everything the terminal has sent so far.
fn drain(pty: &mut RemotePty, out: &mut Vec<u8>) -> bool {
  let mut buf = [0u8; 256];
  loop {
    match pty.read(&mut buf) {
      Ok(0) => return true,
      Ok(n) => out.extend_from_slice(&buf[..n]),
      Err(e) if e.kind() == io::ErrorKind::WouldBlock => return false,
      Err(e) => panic!("{e}"),
    }
  }
}

#[test]
fn a_remote_terminal_runs_a_shell_until_it_exits() {
  let host = host();
  let mut pty = terminal(&host, 1, "/");
  let mut out = vec![];

  // The shell echoes what it computes, which the typed command does not
  // contain.
  pty.write_bytes(b"echo marker-$((6*7)); pwd\n");
  run_until(&host, || {
    drain(&mut pty, &mut out);
    let text = String::from_utf8_lossy(&out);
    text.contains("marker-42") && text.contains("\r\n/\r\n")
  });

  pty.resize(Size { rows: 30, cols: 100 });
  pty.write_bytes(b"stty size\n");
  run_until(&host, || {
    drain(&mut pty, &mut out);
    String::from_utf8_lossy(&out).contains("30 100")
  });

  pty.write_bytes(b"exit\n");
  run_until(&host, || drain(&mut pty, &mut out));
}

#[test]
fn terminals_on_one_connection_are_independent() {
  let host = host();
  let mut a = terminal(&host, 1, "/");
  let mut b = terminal(&host, 2, "/tmp");
  let (mut out_a, mut out_b) = (vec![], vec![]);

  a.write_bytes(b"echo from-$((1+1))\n");
  b.write_bytes(b"echo from-$((2+2))\n");
  run_until(&host, || {
    drain(&mut a, &mut out_a);
    drain(&mut b, &mut out_b);
    String::from_utf8_lossy(&out_a).contains("from-2")
      && String::from_utf8_lossy(&out_b).contains("from-4")
  });
  assert!(!String::from_utf8_lossy(&out_a).contains("from-4"));
  assert!(!String::from_utf8_lossy(&out_b).contains("from-2"));
}

#[test]
fn a_terminal_that_cannot_start_reads_as_closed() {
  let host = host();
  let mut pty = terminal(&host, 1, "/no/such/dir");
  run_until(&host, || drain(&mut pty, &mut vec![]));
}

#[test]
fn a_big_burst_of_output_arrives_whole() {
  let host = host();
  let mut pty = terminal(&host, 1, "/");
  let mut out = vec![];

  // Far more than one stream window, so this needs credit to flow back.
  pty.write_bytes(b"head -c 3000000 /dev/zero | tr '\\0' 'x'; echo; echo done-$((1+1))\n");
  run_until(&host, || {
    drain(&mut pty, &mut out);
    String::from_utf8_lossy(&out[out.len().saturating_sub(64)..]).contains("done-2")
  });
  assert!(out.iter().filter(|&&b| b == b'x').count() >= 3_000_000);
}
