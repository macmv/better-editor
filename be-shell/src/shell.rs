use crate::job::{Job, JobId};
use be_terminal::{Emulator, Pty, PtyHost, Size, SpawnSpec};
use std::{
  io,
  path::{Path, PathBuf},
  sync::Arc,
  time::{Duration, Instant},
};

/// How long one `poll` spends reading from one job.
const READ_BUDGET: Duration = Duration::from_millis(4);
/// After the process has exited but the pty has not reported EOF (something
/// else still holds the slave end open), wait this long before giving up on
/// further output.
const EXIT_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
  /// A job produced output (or its screen changed).
  Output(JobId),
  Finished(JobId, i32),
}

type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Shell {
  cwd:     PathBuf,
  jobs:    Vec<Job>,
  next_id: u64,
  wake:    Option<Wake>,
}

impl Shell {
  pub fn new(cwd: impl Into<PathBuf>) -> Shell {
    Shell { cwd: cwd.into(), jobs: vec![], next_id: 1, wake: None }
  }

  pub fn cwd(&self) -> &Path { &self.cwd }

  pub fn jobs(&self) -> &[Job] { &self.jobs }

  pub fn job(&self, id: JobId) -> Option<&Job> { self.jobs.iter().find(|j| j.id == id) }

  pub fn running(&self) -> Option<&Job> { self.jobs.iter().find(|j| j.is_running()) }

  /// Forgets all finished jobs.
  pub fn clear_finished(&mut self) { self.jobs.retain(|j| j.is_running()); }

  /// True if the last `poll` stopped early, so it should be called again soon
  /// (without waiting for a wakeup).
  pub fn needs_poll(&self) -> bool {
    self.jobs.iter().any(|j| {
      j.is_running() && (j.more_output || j.eof || j.exited_at.is_some() || j.emulator.is_holding())
    })
  }

  /// Sets a function to call from any thread when a job may have output.
  pub fn set_wake(&mut self, wake: impl Fn() + Send + Sync + 'static) {
    let wake: Wake = Arc::new(wake);
    for job in &mut self.jobs {
      apply_wake(job.pty.as_deref_mut(), &wake);
    }
    self.wake = Some(wake);
  }

  /// Runs `line` with `sh -c` on a pty of `size`.
  pub fn submit(&mut self, line: &str, size: Size) -> io::Result<JobId> {
    let cwd = if self.cwd.is_dir() { self.cwd.clone() } else { PathBuf::from("/") };
    let mut pty = Pty::spawn(SpawnSpec {
      program: "sh".into(),
      args: vec!["-c".into(), line.into()],
      env: vec![],
      clear_env: false,
      cwd,
      size,
    })?;
    if let Some(wake) = &self.wake {
      apply_wake(Some(&mut pty), wake);
    }

    let id = JobId(self.next_id);
    self.next_id += 1;
    self.jobs.push(Job {
      id,
      line: line.to_string(),
      emulator: Emulator::new(size),
      exit: None,
      pty: Some(Box::new(pty)),
      eof: false,
      exited_at: None,
      more_output: false,
    });
    Ok(id)
  }

  /// Reads output and detects completion.
  pub fn poll(&mut self) -> Vec<ShellEvent> {
    let mut events = vec![];
    for job in &mut self.jobs {
      if !job.is_running() {
        continue;
      }
      let (output, done) = drain(job);
      if output {
        events.push(ShellEvent::Output(job.id));
      }
      if done {
        let exit = job.pty.as_mut().and_then(|p| p.exit_status()).unwrap_or(-1);
        job.emulator.finish();
        job.exit = Some(exit);
        job.pty = None;
        events.push(ShellEvent::Output(job.id));
        events.push(ShellEvent::Finished(job.id, exit));
      }
    }
    events
  }

  /// Sends bytes to a running job's stdin (Ctrl-C is `0x03`).
  pub fn send_input(&mut self, id: JobId, bytes: &[u8]) {
    if let Some(pty) = self.jobs.iter_mut().find(|j| j.id == id).and_then(|j| j.pty.as_mut()) {
      pty.write_bytes(bytes);
      pty.flush();
    }
  }

  /// Resizes the pty of every running job.
  pub fn resize_running(&mut self, size: Size) {
    for job in self.jobs.iter_mut().filter(|j| j.is_running()) {
      job.emulator.resize(size);
      if let Some(pty) = &mut job.pty {
        pty.resize(size);
      }
    }
  }
}

fn apply_wake(pty: Option<&mut (dyn PtyHost + '_)>, wake: &Wake) {
  if let Some(pty) = pty {
    let wake = wake.clone();
    pty.set_wake(Box::new(move || wake()));
  }
}

/// Reads a job's pty into its emulator. Returns `(got output, finished)`.
fn drain(job: &mut Job) -> (bool, bool) {
  let Some(pty) = job.pty.as_mut() else { return (false, true) };
  let mut buf = [0u8; 16 * 1024];
  let mut output = false;
  let mut would_block = false;
  let start = Instant::now();
  job.more_output = false;

  while !job.eof {
    match pty.read(&mut buf) {
      // Closed on macos.
      Ok(0) => job.eof = true,
      Ok(n) => {
        output = true;
        job.emulator.feed(&buf[..n]);
        let replies = job.emulator.take_replies();
        if !replies.is_empty() {
          pty.write_bytes(&replies);
        }
        if start.elapsed() >= READ_BUDGET {
          job.more_output = true;
          break;
        }
      }
      Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
        would_block = true;
        break;
      }
      // Closed on linux.
      Err(e) if e.raw_os_error() == Some(libc::EIO) => job.eof = true,
      Err(e) => {
        log::error!("failed to read from a job's pty: {e}");
        job.eof = true;
      }
    }
  }
  job.emulator.tick();
  let replies = job.emulator.take_replies();
  if !replies.is_empty() {
    pty.write_bytes(&replies);
  }
  pty.flush();

  if job.more_output {
    return (output, false);
  }
  let exited = pty.exit_status().is_some();
  if exited && job.exited_at.is_none() {
    job.exited_at = Some(Instant::now());
  }
  let done = exited
    && (job.eof || (would_block && job.exited_at.is_some_and(|t| t.elapsed() >= EXIT_GRACE)));
  (output, done)
}
