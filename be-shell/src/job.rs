//! One submitted command line: its pty, its output, and how it ended.

use crate::{prompt::PromptContext, zsh::PreparedRun};
use be_terminal::{Emulator, PtyHost};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// A job is either running or done.
///
/// Background jobs (`cmd &`) are not supported: each line runs in its own
/// `zsh -c`, and when that zsh exits the kernel hangs up the pty, which kills
/// anything it left behind. So "done" always means the whole line is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
  Running,
  /// `exit` is the exit status, or `128 + signal` if the shell was killed.
  Done {
    exit: i32,
  },
}

pub struct Job {
  pub id:              JobId,
  pub line:            String,
  pub started:         Instant,
  /// The wall-clock time of `started`.
  pub started_at:      SystemTime,
  /// Set when the job finishes.
  pub duration:        Option<Duration>,
  pub state:           JobState,
  pub emulator:        Emulator,
  /// Dropped once the job is done.
  pub(crate) pty:      Option<Box<dyn PtyHost>>,
  pub(crate) prepared: Option<PreparedRun>,
  /// The prompt as it looked when this line was submitted.
  pub context:         PromptContext,

  /// The pty reported EOF.
  pub(crate) eof:         bool,
  /// When the process was first seen to have exited.
  pub(crate) exited_at:   Option<Instant>,
  /// The last poll stopped reading before the pty ran dry.
  pub(crate) more_output: bool,
}

impl Job {
  pub fn is_running(&self) -> bool { self.state == JobState::Running }

  pub fn exit(&self) -> Option<i32> {
    match self.state {
      JobState::Done { exit } => Some(exit),
      JobState::Running => None,
    }
  }

  /// How long the job has run, or ran for.
  pub fn elapsed(&self) -> Duration { self.duration.unwrap_or_else(|| self.started.elapsed()) }
}

impl Drop for Job {
  fn drop(&mut self) {
    // Don't leave a running command behind when the view goes away.
    if let Some(pty) = &mut self.pty {
      pty.signal(libc::SIGHUP);
    }
  }
}
