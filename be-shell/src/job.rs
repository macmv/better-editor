use be_terminal::{Emulator, PtyHost};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// One submitted command line: its pty, its output, and how it ended.
pub struct Job {
  pub id:       JobId,
  pub line:     String,
  pub emulator: Emulator,
  /// The exit status (`128 + signal` if killed), once the job is done.
  pub exit:     Option<i32>,

  /// Dropped once the job is done.
  pub(crate) pty:         Option<Box<dyn PtyHost>>,
  /// The pty reported EOF.
  pub(crate) eof:         bool,
  /// When the process was first seen to have exited.
  pub(crate) exited_at:   Option<Instant>,
  /// The last poll stopped reading before the pty ran dry.
  pub(crate) more_output: bool,
}

impl Job {
  pub fn is_running(&self) -> bool { self.exit.is_none() }
}

impl Drop for Job {
  fn drop(&mut self) {
    // Don't leave a running command behind when the view goes away.
    if let Some(pty) = &mut self.pty {
      pty.signal(libc::SIGHUP);
    }
  }
}
