//! The shells this host runs for its client, and the pumping of their output.
//!
//! A terminal's output is read from its pty only while the client has granted
//! credit for it, so a client that stops reading stops the shell through the
//! kernel's own pty buffer, and nothing is buffered here.

use std::{cell::RefCell, collections::HashMap, io, os::fd::AsRawFd, rc::Rc};

use be_remote::{Bytes, HostError};
use be_terminal::{Pty, PtyHost, Size};
use lode_local::{Reply, Sink, TrySendError};
use lode_poll::sys::PollFd;

/// The most bytes read from a pty at once.
const CHUNK: usize = 4096;
/// What the connection charges on top of an item's bytes (`ITEM_OVERHEAD`).
const ITEM_OVERHEAD: usize = 16;

pub struct Term {
  pub pty:   Pty,
  pub sink:  Sink<Bytes>,
  /// Answered when the shell exits.
  pub reply: Reply<(), HostError>,
  /// Read from the pty but not yet accepted by the sink.
  unsent:    Option<Bytes>,
}

impl Term {
  pub fn new(pty: Pty, sink: Sink<Bytes>, reply: Reply<(), HostError>) -> Term {
    Term { pty, sink, reply, unsent: None }
  }

  /// Bytes the sink will take right now.
  fn room(&self) -> usize { self.sink.credit().saturating_sub(ITEM_OVERHEAD).min(CHUNK) }

  /// Moves output from the pty to the client until one of them has no more.
  fn pump(&mut self) -> Pump {
    loop {
      if let Some(chunk) = self.unsent.take() {
        match self.sink.try_send(chunk) {
          Ok(()) => {}
          Err(TrySendError::Full(chunk)) => {
            self.unsent = Some(chunk);
            return Pump::Idle;
          }
          Err(_) => return Pump::Gone,
        }
      }

      let room = self.room();
      if room == 0 {
        return Pump::Idle;
      }
      let mut buf = [0u8; CHUNK];
      match self.pty.read(&mut buf[..room]) {
        // The shell exited: EOF, or EIO on Linux.
        Ok(0) => return Pump::Exited,
        Err(e) if e.raw_os_error() == Some(libc::EIO) => return Pump::Exited,
        Ok(n) => self.unsent = Some(Bytes::copy_from_slice(&buf[..n])),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Pump::Idle,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
        Err(e) => {
          eprintln!("be-host: reading a pty failed: {e}");
          return Pump::Exited;
        }
      }
    }
  }
}

enum Pump {
  /// Waiting on the pty or on credit.
  Idle,
  Exited,
  /// The client went away.
  Gone,
}

#[derive(Default)]
pub struct Terms {
  terms: HashMap<u64, Term>,
}

pub type SharedTerms = Rc<RefCell<Terms>>;

impl Terms {
  pub fn insert(&mut self, id: u64, term: Term) -> Option<Term> { self.terms.insert(id, term) }

  pub fn get_mut(&mut self, id: u64) -> Option<&mut Term> { self.terms.get_mut(&id) }

  /// The client dropped the call: the shell is killed by closing its pty.
  pub fn remove(&mut self, id: u64) { self.terms.remove(&id); }

  /// Pumps every terminal, and finishes the ones whose shell exited.
  pub fn pump(&mut self) {
    let mut done = vec![];
    for (&id, term) in &mut self.terms {
      match term.pump() {
        Pump::Idle => {}
        Pump::Exited => done.push((id, true)),
        Pump::Gone => done.push((id, false)),
      }
    }
    for (id, exited) in done {
      let term = self.terms.remove(&id).unwrap();
      if exited {
        term.sink.finish();
        term.reply.finish();
      }
    }
  }

  /// What the poll loop must wait on for the terminals.
  pub fn poll_fds(&self) -> Vec<PollFd> {
    self
      .terms
      .values()
      .map(|t| PollFd {
        fd:       t.pty.as_raw_fd(),
        // Only listen for output we have room to forward.
        readable: t.unsent.is_none() && t.room() > 0,
        writable: t.pty.has_pending_writes(),
      })
      .collect()
  }

  /// Sends queued input to the shells whose pty has room.
  pub fn flush(&mut self) {
    for t in self.terms.values_mut() {
      t.pty.flush();
    }
  }
}

pub fn spawn(size: Size, cwd: &str) -> io::Result<Pty> {
  let cwd = if cwd.is_empty() { std::env::current_dir()? } else { cwd.into() };
  Pty::try_new(size, &cwd)
}
