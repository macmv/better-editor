//! The server end of a remote editing session: `ssh machine be-host --stdio`.
//! Serves `be_remote::Host` over its own stdin and stdout, so nothing else may
//! print to stdout. Logs go to stderr.

mod terminal;

use std::{cell::RefCell, fs, io, rc::Rc, time::Instant};

use be_remote::{
  Bytes, DirEntry, DirKind, Host, HostError, HostInfo, HostService, PROTOCOL_VERSION, TermSize,
};
use be_terminal::{PtyHost, Size};
use lode_local::{Event, Reply, Sink, Token};
use lode_poll::{ChannelFds, Connection, sys};

use crate::terminal::{SharedTerms, Term};

struct Fs {
  terms: SharedTerms,
}

fn io_error(e: io::Error) -> HostError {
  match e.kind() {
    io::ErrorKind::NotFound => HostError::NotFound,
    io::ErrorKind::PermissionDenied => HostError::PermissionDenied,
    io::ErrorKind::InvalidData => HostError::NotUtf8,
    _ => HostError::Io(e.to_string()),
  }
}

fn size(s: TermSize) -> Size { Size { rows: s.rows as usize, cols: s.cols as usize } }

#[lode::handler]
impl Host for Fs {
  fn info(&self) -> Result<HostInfo, HostError> {
    let cwd = std::env::current_dir().map_err(io_error)?;
    Ok(HostInfo { version: PROTOCOL_VERSION, cwd: cwd.to_string_lossy().into_owned() })
  }

  fn list_dir(&self, path: String) -> Result<Vec<DirEntry>, HostError> {
    let mut entries = vec![];
    for entry in fs::read_dir(path).map_err(io_error)? {
      let entry = entry.map_err(io_error)?;
      let kind = match entry.file_type().map_err(io_error)? {
        t if t.is_dir() => DirKind::Dir,
        t if t.is_file() => DirKind::File,
        _ => DirKind::Other,
      };
      entries.push(DirEntry { name: entry.file_name().to_string_lossy().into_owned(), kind });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
  }

  fn read_file(&self, path: String) -> Result<String, HostError> {
    fs::read_to_string(path).map_err(io_error)
  }

  // Takes its `Reply` by hand: the call stays open until the shell exits.
  fn spawn_terminal(
    &self,
    id: u64,
    term_size: TermSize,
    cwd: String,
    sink: Sink<Bytes>,
    reply: Reply<(), HostError>,
  ) {
    match terminal::spawn(size(term_size), &cwd) {
      Ok(pty) => {
        // A cancelled call means the client closed the terminal.
        reply.on_cancel(Token(id));
        if self.terms.borrow_mut().insert(id, Term::new(pty, sink, reply)).is_some() {
          eprintln!("be-host: terminal {id} was replaced");
        }
      }
      Err(e) => reply.err(io_error(e)),
    }
  }

  fn terminal_input(&self, id: u64, data: Bytes) {
    if let Some(term) = self.terms.borrow_mut().get_mut(id) {
      term.pty.write_bytes(&data);
      term.pty.flush();
    }
  }

  fn terminal_resize(&self, id: u64, term_size: TermSize) {
    if let Some(term) = self.terms.borrow_mut().get_mut(id) {
      term.pty.resize(size(term_size));
    }
  }
}

fn main() {
  if !std::env::args().any(|a| a == "--stdio") {
    eprintln!("usage: be-host --stdio");
    std::process::exit(2);
  }

  let terms: SharedTerms = Rc::new(RefCell::new(Default::default()));

  let conn = Connection::builder()
    .serve::<Host, _>({
      let terms = terms.clone();
      move |_| Fs { terms }
    })
    .accept(vec![ChannelFds::stdio().expect("stdio")], Instant::now())
    .expect("accept");

  while !conn.is_finished() {
    terms.borrow_mut().pump();

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
    if fds.is_empty() {
      break;
    }
    let conn_fds = fds.len();
    fds.extend(terms.borrow().poll_fds());

    let ready = match sys::poll(&fds, conn.poll_timeout_ms(Instant::now())) {
      Ok(r) => r,
      Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
      Err(e) => {
        eprintln!("be-host: poll failed: {e}");
        std::process::exit(1);
      }
    };
    let now = Instant::now();
    let on_conn = || ready[..conn_fds].iter().zip(&fds[..conn_fds]);
    if on_conn().any(|(r, f)| r.readable && f.readable) {
      conn.handle_readable(now);
    }
    if on_conn().any(|(r, f)| r.writable && f.writable) {
      conn.handle_writable();
    }
    // Pty readiness needs no handling of its own: the next `pump` reads it.
    terms.borrow_mut().flush();
    conn.handle_timeout(now);
    while let Some(event) = conn.poll_event() {
      if let Event::Ready(Token(id)) = event {
        terms.borrow_mut().remove(id);
      }
    }
  }
}
