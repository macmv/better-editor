//! Everything needed to run one command line through `zsh -c` and carry the
//! shell state across, without touching a pty. The pty layer plugs
//! [`CommandSpec`] into its own spawn call.

use crate::state::{DumpDir, ShellState};
use std::{
  fs, io,
  path::{Path, PathBuf},
  process::{Command, Stdio},
};

/// Defines `__be_dump` and registers it as a `zshexit` hook.
pub const DUMP_SCRIPT: &str = include_str!("dump.zsh");
const PREAMBLE: &str = include_str!("preamble.zsh");

/// A process to spawn, as plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
  pub program: String,
  pub args:    Vec<String>,
  /// The complete environment. The child must not inherit anything else.
  pub env:     Vec<(String, String)>,
  pub cwd:     PathBuf,
}

impl CommandSpec {
  pub fn to_command(&self) -> Command {
    let mut cmd = Command::new(&self.program);
    cmd
      .args(&self.args)
      .env_clear()
      .envs(self.env.iter().map(|(k, v)| (k, v)))
      .current_dir(&self.cwd);
    cmd
  }
}

/// The script passed to `zsh -f -c` for every command.
pub fn command_script() -> String { format!("{DUMP_SCRIPT}\n{PREAMBLE}") }

/// Builds the spawn spec for running `line` in `state`.
///
/// * `script_path` is the file holding `state.script`.
/// * `out` is the directory the exit hook writes the new state into.
/// * `baseline` is a cache file for the pristine-zsh parameter listing.
///
/// The line travels in `$BE_LINE` and is `eval`ed, so it needs no quoting. zsh
/// is started with `-f`, so no rc files run; everything comes from the state.
pub fn command_spec(
  state: &ShellState,
  line: &str,
  script_path: &Path,
  out: &Path,
  baseline: &Path,
) -> CommandSpec {
  let mut env = base_env(state);
  env.push(("BE_LINE".into(), line.into()));
  env.push(("BE_OUT".into(), out.to_string_lossy().into_owned()));
  env.push(("BE_SCRIPT".into(), script_path.to_string_lossy().into_owned()));
  env.push(("BE_BASELINE".into(), baseline.to_string_lossy().into_owned()));
  CommandSpec {
    program: "zsh".into(),
    args: vec!["-f".into(), "-c".into(), command_script(), "zsh".into()],
    env,
    cwd: state.cwd.clone(),
  }
}

/// Spec for the session-start snapshot: an interactive `zsh -i` that loads the
/// user's real rc files and then dumps its state. Its stdout/stderr (rc
/// chatter) should be discarded.
pub fn snapshot_spec(base: &ShellState, out: &Path, baseline: &Path) -> CommandSpec {
  let mut env = base_env(base);
  env.push(("BE_OUT".into(), out.to_string_lossy().into_owned()));
  env.push(("BE_BASELINE".into(), baseline.to_string_lossy().into_owned()));
  CommandSpec {
    program: "zsh".into(),
    args: vec!["-i".into(), "-c".into(), format!("{DUMP_SCRIPT}\nexit")],
    env,
    cwd: base.cwd.clone(),
  }
}

fn base_env(state: &ShellState) -> Vec<(String, String)> {
  let mut env: Vec<(String, String)> =
    state.env.iter().filter(|(k, _)| k != "PWD").cloned().collect();
  env.push(("PWD".into(), state.cwd.to_string_lossy().into_owned()));
  env
}

/// A prepared command: spawn `spec`, wait for it to exit, then pass this to
/// [`Session::finish`].
#[derive(Debug)]
pub struct PreparedRun {
  pub spec: CommandSpec,
  dump:     DumpDir,
}

#[derive(Debug)]
pub struct RunResult {
  pub status:        Option<i32>,
  pub stdout:        Vec<u8>,
  pub stderr:        Vec<u8>,
  /// False if the exit hook did not finish, so the state is unchanged.
  pub state_updated: bool,
}

/// Owns the persistent state and the scratch directory.
pub struct Session {
  root:        PathBuf,
  pub state:   ShellState,
  next_run:    u64,
  /// The script content currently in `state.zsh`.
  on_disk:     Option<String>,
  zsh_program: String,
}

impl Session {
  pub fn new(root: impl Into<PathBuf>, state: ShellState) -> io::Result<Session> {
    let root = root.into();
    fs::create_dir_all(&root)?;
    Ok(Session { root, state, next_run: 0, on_disk: None, zsh_program: "zsh".into() })
  }

  /// Overrides the zsh binary (default: `zsh` from PATH).
  pub fn set_zsh_program(&mut self, program: impl Into<String>) {
    self.zsh_program = program.into();
  }

  pub fn script_path(&self) -> PathBuf { self.root.join("state.zsh") }

  /// Cache of a pristine zsh's `typeset -p`, written by the first dump.
  pub fn baseline_path(&self) -> PathBuf { self.root.join("baseline.txt") }

  fn new_dump_dir(&mut self) -> io::Result<DumpDir> {
    self.next_run += 1;
    DumpDir::create(self.root.join(format!("run-{}", self.next_run)))
  }

  /// Discards a prepared run that was never spawned.
  pub fn abandon(&mut self, run: PreparedRun) { run.dump.remove(); }

  /// Prepares the session-start snapshot (`zsh -i`).
  pub fn prepare_snapshot(&mut self) -> io::Result<PreparedRun> {
    let dump = self.new_dump_dir()?;
    let mut spec = snapshot_spec(&self.state, dump.path(), &self.baseline_path());
    spec.program = self.zsh_program.clone();
    Ok(PreparedRun { spec, dump })
  }

  /// Prepares running `line` with the current state.
  pub fn prepare(&mut self, line: &str) -> io::Result<PreparedRun> {
    if self.on_disk.as_deref() != Some(self.state.script.as_str()) {
      let tmp = self.root.join("state.zsh.tmp");
      fs::write(&tmp, &self.state.script)?;
      fs::rename(&tmp, self.script_path())?;
      self.on_disk = Some(self.state.script.clone());
    }
    let dump = self.new_dump_dir()?;
    let mut spec =
      command_spec(&self.state, line, &self.script_path(), dump.path(), &self.baseline_path());
    spec.program = self.zsh_program.clone();
    Ok(PreparedRun { spec, dump })
  }

  /// Folds the run's dump into the state. Returns whether the state changed;
  /// when the exit hook never completed the old state is kept.
  pub fn finish(&mut self, run: PreparedRun) -> io::Result<bool> {
    let new = run.dump.read()?;
    run.dump.remove();
    match new {
      Some(new) => {
        self.state = new;
        Ok(true)
      }
      None => Ok(false),
    }
  }

  /// Runs the snapshot synchronously (no pty, stdin/out/err captured).
  pub fn snapshot_blocking(&mut self) -> io::Result<RunResult> {
    let run = self.prepare_snapshot()?;
    self.run_blocking_inner(run)
  }

  /// Runs `line` synchronously with piped stdio (no pty).
  pub fn run_blocking(&mut self, line: &str) -> io::Result<RunResult> {
    let run = self.prepare(line)?;
    self.run_blocking_inner(run)
  }

  fn run_blocking_inner(&mut self, run: PreparedRun) -> io::Result<RunResult> {
    let out = run
      .spec
      .to_command()
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .output()?;
    let state_updated = self.finish(run)?;
    Ok(RunResult {
      status: out.status.code(),
      stdout: out.stdout,
      stderr: out.stderr,
      state_updated,
    })
  }
}

/// Returns true if `zsh` can be executed.
pub fn zsh_available() -> bool {
  Command::new("zsh")
    .arg("-f")
    .arg("-c")
    .arg("true")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .map(|s| s.success())
    .unwrap_or(false)
}
