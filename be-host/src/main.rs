//! The server end of a remote editing session: `ssh machine be-host --stdio`.
//! Serves `be_remote::Host` over its own stdin and stdout, so nothing else may
//! print to stdout. Logs go to stderr.

mod terminal;
mod watch;

use std::{cell::RefCell, collections::HashMap, fs, io, path::Path, rc::Rc, time::Instant};

use std::os::unix::fs::MetadataExt;

use be_git::GitRepo;
use be_remote::{
  Bytes, DirEntry, DirKind, GitFileStatus, Host, HostError, HostInfo, HostService, OpenedFile,
  PROTOCOL_VERSION, TermSize, WrittenFile,
};
use be_terminal::{PtyHost, Size};
use lode_local::{Event, Reply, Sink, Token};
use lode_poll::{ChannelFds, Connection, sys};

use crate::{
  terminal::{SharedTerms, Term},
  watch::SharedWatches,
};

struct Service {
  terms:   SharedTerms,
  watches: SharedWatches,
  /// `None`: the root is not in a repository.
  repos:   RefCell<HashMap<String, Option<GitRepo>>>,
}

fn io_error(e: io::Error) -> HostError {
  match e.kind() {
    io::ErrorKind::NotFound => HostError::NotFound,
    io::ErrorKind::PermissionDenied => HostError::PermissionDenied,
    io::ErrorKind::InvalidData => HostError::NotUtf8,
    _ => HostError::Io(e.to_string()),
  }
}

impl Service {
  fn with_repo<R>(&self, root: &str, f: impl FnOnce(&GitRepo) -> R) -> Option<R> {
    let mut repos = self.repos.borrow_mut();
    let repo = repos.entry(root.to_string()).or_insert_with(|| GitRepo::open(Path::new(root)).ok());
    repo.as_ref().map(f)
  }
}

fn size(s: TermSize) -> Size { Size { rows: s.rows as usize, cols: s.cols as usize } }

#[lode::handler]
impl Host for Service {
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

  fn list_files(&self, root: String) -> Result<Vec<String>, HostError> {
    fs::metadata(&root).map_err(io_error)?;

    let mut files = vec![];
    for entry in ignore::Walk::new(&root).flatten() {
      if entry.file_type().is_some_and(|t| t.is_dir()) {
        continue;
      }
      if let Ok(rel) = entry.path().strip_prefix(&root)
        && let Some(rel) = rel.to_str()
      {
        files.push(rel.to_string());
      }
    }
    Ok(files)
  }

  fn canonicalize(&self, path: String) -> Result<String, HostError> {
    let path = fs::canonicalize(path).map_err(io_error)?;
    Ok(path.to_string_lossy().into_owned())
  }

  fn open_file(&self, path: String) -> Result<OpenedFile, HostError> {
    let path = fs::canonicalize(path).map_err(io_error)?;
    let mtime = fs::metadata(&path).map_err(io_error)?.mtime();
    let data = Bytes::from(fs::read(&path).map_err(io_error)?);
    Ok(OpenedFile { path: path.to_string_lossy().into_owned(), mtime, data })
  }

  fn write_file(&self, path: String, data: Bytes) -> Result<WrittenFile, HostError> {
    use std::io::Write;

    let prev_mtime = fs::metadata(&path).map_err(io_error)?.mtime();
    let mut file =
      fs::OpenOptions::new().write(true).truncate(true).open(&path).map_err(io_error)?;
    file.write_all(&data).map_err(io_error)?;
    drop(file);
    let mtime = fs::metadata(&path).map_err(io_error)?.mtime();
    self.watches.borrow_mut().notify_saved(Path::new(&path));
    Ok(WrittenFile { prev_mtime, mtime })
  }

  // Takes its `Reply` by hand, like `spawn_terminal`: the call stays open until
  // the client drops it.
  fn watch(&self, id: u64, root: String, sink: Sink<Vec<String>>, reply: Reply<(), HostError>) {
    if let Err(e) = fs::metadata(&root) {
      return reply.err(io_error(e));
    }
    reply.on_cancel(Token(id));
    self.watches.borrow_mut().insert(id, Path::new(&root), sink, reply);
  }

  fn git_head(&self, root: String) -> Result<Option<String>, HostError> {
    Ok(self.with_repo(&root, |repo| repo.head_id()).flatten())
  }

  fn git_lookup(&self, root: String, path: String) -> Result<Option<String>, HostError> {
    Ok(self.with_repo(&root, |repo| repo.text_at_head(Path::new(&path))).flatten())
  }

  fn git_status(&self, root: String, path: String) -> Result<GitFileStatus, HostError> {
    let path = Path::new(&path);
    Ok(
      self
        .with_repo(&root, |repo| {
          let s = repo.path_status(path);
          GitFileStatus { ignored: s.ignored, added: s.added, modified: s.modified }
        })
        .unwrap_or_default(),
    )
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
  let watches: SharedWatches = Rc::new(RefCell::new(Default::default()));

  let conn = Connection::builder()
    .serve::<Host, _>({
      let terms = terms.clone();
      let watches = watches.clone();
      move |_| Service { terms, watches, repos: Default::default() }
    })
    .accept(vec![ChannelFds::stdio().expect("stdio")], Instant::now())
    .expect("accept");

  while !conn.is_finished() {
    terms.borrow_mut().pump();
    watches.borrow_mut().pump();

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
    fds.push(watches.borrow().poll_fd());

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
    // Pty and watcher readiness need no handling of their own: the next `pump`
    // reads them.
    terms.borrow_mut().flush();
    conn.handle_timeout(now);
    while let Some(event) = conn.poll_event() {
      if let Event::Ready(Token(id)) = event {
        terms.borrow_mut().remove(id);
        watches.borrow_mut().remove(id);
      }
    }
  }
}
