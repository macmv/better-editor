//! The shell backend: submits lines, runs each one as `zsh -f -c` in its own
//! pty, and carries shell state between them.
//!
//! zsh is only the language implementation; nothing here parses shell syntax
//! (apart from the exact-match UI commands in [`crate::ui_cmd`]).
//!
//! # Limitations
//!
//! Background jobs (`cmd &`) are not supported. Process exit is the sync
//! point (the exit hook dumps the state), so a line is over when its zsh
//! exits; the kernel then hangs up the pty and kills anything left behind.
//!
//! # Session snapshot
//!
//! The initial state (aliases, functions, env from the user's rc files) comes
//! from an interactive `zsh -i` that takes ~50ms or much more with a heavy rc.
//! [`Shell::new`] starts it on a background thread, with all of its output
//! discarded. [`Shell::submit`] waits for it if it is still running (so the
//! very first command may be delayed by the remaining time), and
//! [`Shell::poll`] adopts it as soon as it is done.

use crate::{
  History, HistoryEntry, ShellState, UiCommand,
  job::{Job, JobId, JobState},
  parse_ui_command,
  prompt::PromptContext,
  zsh::{CommandSpec, PreparedRun, Session},
};
use be_terminal::{Emulator, Pty, PtyHost, Size, SpawnSpec};
use std::{
  ffi::OsString,
  io,
  path::{Path, PathBuf},
  process::Stdio,
  sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
  },
  thread::JoinHandle,
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
  /// The persistent state (cwd, env, ...) changed.
  StateChanged,
  /// The user typed `exit`.
  Exit,
  /// The user typed `clear`.
  Clear,
  /// The user typed `history`.
  History,
  /// A job-control command that this shell does not implement.
  Unsupported(UiCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitResult {
  /// A job was started.
  Started(JobId),
  /// The line was handled by the UI.
  Event(ShellEvent),
  /// The job could not be started.
  Failed(String),
}

type Wake = Arc<dyn Fn() + Send + Sync>;

struct PendingSnapshot {
  thread: JoinHandle<io::Result<()>>,
  run:    PreparedRun,
}

pub struct Shell {
  session:       Session,
  history:       History,
  jobs:          Vec<Job>,
  next_id:       u64,
  wake:          Option<Wake>,
  snapshot:      Option<PendingSnapshot>,
  /// Scratch dir to remove on drop, if we created it.
  owned_root:    Option<PathBuf>,
  last_exit:     Option<i32>,
  last_duration: Option<Duration>,
}

impl Shell {
  /// A shell in `cwd`, with the environment of this process, the default
  /// history file, and a private scratch dir under the system temp dir. The
  /// initial snapshot is started in the background.
  pub fn new(cwd: impl Into<PathBuf>) -> io::Result<Shell> {
    static N: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
      "be-shell-{}-{}",
      std::process::id(),
      N.fetch_add(1, Ordering::Relaxed)
    ));
    let history = History::open(History::default_path().unwrap_or_else(|| root.join("history")))?;
    let mut state = ShellState::from_process();
    state.cwd = cwd.into();
    let mut shell = Shell::with_parts(state, &root, history)?;
    shell.owned_root = Some(root);
    Ok(shell)
  }

  /// Like [`Shell::new`], with everything supplied. `state.env` is the
  /// environment the snapshot starts from.
  pub fn with_parts(state: ShellState, root: &Path, history: History) -> io::Result<Shell> {
    let session = Session::new(root, state)?;
    let mut shell = Shell {
      session,
      history,
      jobs: vec![],
      next_id: 1,
      wake: None,
      snapshot: None,
      owned_root: None,
      last_exit: None,
      last_duration: None,
    };
    shell.start_snapshot()?;
    Ok(shell)
  }

  fn start_snapshot(&mut self) -> io::Result<()> {
    let run = self.session.prepare_snapshot()?;
    let mut cmd = run.spec.to_command();
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let thread = std::thread::Builder::new()
      .name("be-shell-snapshot".into())
      .spawn(move || cmd.status().map(|_| ()))?;
    self.snapshot = Some(PendingSnapshot { thread, run });
    Ok(())
  }

  /// Adopts the snapshot if it is done (or, with `wait`, once it is). Returns
  /// whether the state changed.
  fn finish_snapshot(&mut self, wait: bool) -> bool {
    let Some(p) = self.snapshot.take_if(|p| wait || p.thread.is_finished()) else { return false };
    match p.thread.join() {
      Ok(Ok(())) => {}
      Ok(Err(e)) => log::warn!("failed to run the shell snapshot: {e}"),
      Err(_) => log::warn!("the shell snapshot thread panicked"),
    }
    match self.session.finish(p.run) {
      Ok(true) => true,
      Ok(false) => {
        log::warn!("the shell snapshot did not complete; using the process environment");
        false
      }
      Err(e) => {
        log::warn!("failed to read the shell snapshot: {e}");
        false
      }
    }
  }

  pub fn state(&self) -> &ShellState { &self.session.state }

  pub fn cwd(&self) -> &Path { &self.session.state.cwd }

  pub fn history(&self) -> &History { &self.history }

  pub fn jobs(&self) -> &[Job] { &self.jobs }

  pub fn job(&self, id: JobId) -> Option<&Job> { self.jobs.iter().find(|j| j.id == id) }

  fn job_mut(&mut self, id: JobId) -> Option<&mut Job> { self.jobs.iter_mut().find(|j| j.id == id) }

  /// The prompt for the next line.
  pub fn prompt(&self) -> PromptContext {
    PromptContext::capture(
      self.cwd(),
      &self.session.state.env,
      self.last_exit,
      self.last_duration,
      self.wake.clone(),
    )
  }

  /// Whether any job still has output to read or is running.
  pub fn has_running(&self) -> bool { self.jobs.iter().any(|j| j.is_running()) }

  /// True if the last `poll` stopped early, so it should be called again soon
  /// (without waiting for a wakeup).
  pub fn needs_poll(&self) -> bool {
    self.jobs.iter().any(|j| j.is_running() && (j.more_output || j.eof || j.emulator.is_holding()))
  }

  /// Sets a function to call from any thread when a job may have output.
  pub fn set_wake(&mut self, wake: impl Fn() + Send + Sync + 'static) {
    let wake: Wake = Arc::new(wake);
    for job in &mut self.jobs {
      apply_wake(job.pty.as_deref_mut(), &wake);
    }
    self.wake = Some(wake);
  }

  /// Handles a line: UI commands become events, anything else starts a job.
  pub fn submit(&mut self, line: &str, size: Size) -> SubmitResult {
    self.submit_with(line, size, None)
  }

  /// Like [`Shell::submit`], recording `prompt` as the prompt the line was
  /// typed at (instead of capturing a new one), so that the block shows
  /// exactly what the user saw.
  pub fn submit_with(
    &mut self,
    line: &str,
    size: Size,
    prompt: Option<PromptContext>,
  ) -> SubmitResult {
    if let Some(cmd) = parse_ui_command(line) {
      return SubmitResult::Event(match cmd {
        UiCommand::Exit => ShellEvent::Exit,
        UiCommand::Clear => ShellEvent::Clear,
        UiCommand::History => ShellEvent::History,
        other => ShellEvent::Unsupported(other),
      });
    }

    self.finish_snapshot(true);

    let context = prompt.unwrap_or_else(|| self.prompt());
    let prepared = match self.session.prepare(line) {
      Ok(p) => p,
      Err(e) => return SubmitResult::Failed(format!("failed to prepare command: {e}")),
    };
    let spec = spawn_spec(&prepared.spec, size);
    let mut pty = match Pty::spawn(spec) {
      Ok(p) => p,
      Err(e) => {
        self.session.abandon(prepared);
        return SubmitResult::Failed(format!("failed to start zsh: {e}"));
      }
    };
    if let Some(wake) = &self.wake {
      apply_wake(Some(&mut pty), wake);
    }

    let id = JobId(self.next_id);
    self.next_id += 1;
    self.jobs.push(Job {
      id,
      line: line.to_string(),
      started: Instant::now(),
      duration: None,
      state: JobState::Running,
      emulator: Emulator::new(size),
      pty: Some(Box::new(pty)),
      prepared: Some(prepared),
      context,
      eof: false,
      exited_at: None,
      more_output: false,
    });
    SubmitResult::Started(id)
  }

  /// Reads output, detects completion, and adopts new state.
  pub fn poll(&mut self) -> Vec<ShellEvent> {
    let mut events = vec![];
    if self.finish_snapshot(false) {
      events.push(ShellEvent::StateChanged);
    }

    let mut finished = vec![];
    for job in &mut self.jobs {
      if !job.is_running() {
        continue;
      }
      let (output, done) = drain(job);
      if output {
        events.push(ShellEvent::Output(job.id));
      }
      if done {
        finished.push(job.id);
      }
    }

    for id in finished {
      self.finish_job(id, &mut events);
    }
    events
  }

  fn finish_job(&mut self, id: JobId, events: &mut Vec<ShellEvent>) {
    let Some(i) = self.jobs.iter().position(|j| j.id == id) else { return };
    let job = &mut self.jobs[i];
    let exit = job.pty.as_mut().and_then(|p| p.exit_status()).unwrap_or(-1);
    job.emulator.finish();
    job.duration = Some(job.started.elapsed());
    job.state = JobState::Done { exit };
    job.pty = None;
    let prepared = job.prepared.take();
    let entry = HistoryEntry {
      timestamp:   SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
      cwd:         job.context.cwd.to_string_lossy().into_owned(),
      exit_code:   exit,
      duration_ms: job.duration.map(|d| d.as_millis() as u64).unwrap_or(0),
      command:     job.line.clone(),
    };
    self.last_exit = Some(exit);
    self.last_duration = job.duration;

    if let Some(prepared) = prepared {
      match self.session.finish(prepared) {
        Ok(true) => events.push(ShellEvent::StateChanged),
        // The exit hook did not finish (killed by a signal, `exec`, ...), so the
        // state from before the command stays.
        Ok(false) => log::info!("command {:?} did not update the shell state", entry.command),
        Err(e) => log::warn!("failed to read the shell state after {:?}: {e}", entry.command),
      }
    }
    if let Err(e) = self.history.append(entry) {
      log::warn!("failed to write shell history: {e}");
    }
    events.push(ShellEvent::Output(id));
    events.push(ShellEvent::Finished(id, exit));
  }

  /// Sends bytes to a running job's stdin (Ctrl-C is `0x03`).
  pub fn send_input(&mut self, id: JobId, bytes: &[u8]) {
    if let Some(pty) = self.job_mut(id).and_then(|j| j.pty.as_mut()) {
      pty.write_bytes(bytes);
      pty.flush();
    }
  }

  pub fn resize(&mut self, id: JobId, size: Size) {
    if let Some(job) = self.job_mut(id) {
      job.emulator.resize(size);
      if let Some(pty) = &mut job.pty {
        pty.resize(size);
      }
    }
  }

  /// Signals a running job's process group.
  pub fn signal(&mut self, id: JobId, sig: i32) {
    if let Some(pty) = self.job_mut(id).and_then(|j| j.pty.as_mut()) {
      pty.signal(sig);
    }
  }
}

impl Drop for Shell {
  fn drop(&mut self) {
    // Kill running jobs (see `Job::drop`) before removing their scratch dirs.
    self.jobs.clear();
    if let Some(snap) = self.snapshot.take() {
      let _ = snap.thread.join();
    }
    if let Some(root) = &self.owned_root {
      let _ = std::fs::remove_dir_all(root);
    }
  }
}

fn apply_wake(pty: Option<&mut (dyn PtyHost + '_)>, wake: &Wake) {
  if let Some(pty) = pty {
    let wake = wake.clone();
    pty.set_wake(Box::new(move || wake()));
  }
}

/// Builds the pty spawn for `spec`. The directory falls back to `$HOME` (then
/// `/`) when it no longer exists, and `PWD` follows it.
fn spawn_spec(spec: &CommandSpec, size: Size) -> SpawnSpec {
  let mut cwd = spec.cwd.clone();
  let mut env = spec.env.clone();
  if !cwd.is_dir() {
    cwd = env
      .iter()
      .find(|(k, _)| k == "HOME")
      .map(|(_, v)| PathBuf::from(v))
      .filter(|p| p.is_dir())
      .unwrap_or_else(|| PathBuf::from("/"));
    env.retain(|(k, _)| k != "PWD");
    env.push(("PWD".into(), cwd.to_string_lossy().into_owned()));
  }
  SpawnSpec {
    program: spec.program.clone().into(),
    args: spec.args.iter().map(OsString::from).collect(),
    env: env.into_iter().map(|(k, v)| (k.into(), v.into())).collect(),
    clear_env: true,
    cwd,
    size,
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
