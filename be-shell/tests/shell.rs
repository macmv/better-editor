//! Drives a `Shell` headlessly. Needs a real `zsh`; skips without one.

use be_shell::{
  History, JobId, JobState, Shell, ShellEvent, ShellState, SubmitResult, UiCommand, zsh,
};
use be_terminal::{BuiltinColor, Size, TerminalColor};
use std::{
  fs,
  path::PathBuf,
  sync::atomic::{AtomicU32, Ordering},
  time::{Duration, Instant},
};

macro_rules! need_zsh {
  () => {
    if !zsh::zsh_available() {
      eprintln!("zsh not found, skipping");
      return;
    }
  };
}

const SIZE: Size = Size { rows: 24, cols: 80 };

/// A shell in a throwaway directory, which is removed on drop.
struct Fixture {
  shell: Shell,
  dir:   PathBuf,
  work:  PathBuf,
}

impl Fixture {
  fn new(rc: &str) -> Fixture {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
      "be-shell-st-{}-{}",
      std::process::id(),
      N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    let zdot = dir.join("zdot");
    let work = dir.join("work");
    fs::create_dir_all(&zdot).unwrap();
    fs::create_dir_all(&work).unwrap();
    let dir = dir.canonicalize().unwrap();
    let work = work.canonicalize().unwrap();
    fs::write(dir.join("zdot/.zshrc"), rc).unwrap();

    let mut state = ShellState::from_process();
    state.cwd = work.clone();
    state.env.retain(|(k, _)| k != "ZDOTDIR");
    state.env.push(("ZDOTDIR".into(), dir.join("zdot").to_string_lossy().into_owned()));
    let history = History::open(dir.join("history")).unwrap();
    let shell = Shell::with_parts(state, &dir.join("session"), history).unwrap();
    Fixture { shell, dir, work }
  }

  fn start(&mut self, line: &str) -> JobId {
    match self.shell.submit(line, SIZE) {
      SubmitResult::Started(id) => id,
      other => panic!("not started: {other:?}"),
    }
  }

  /// Polls until the job finishes. Returns its exit status.
  fn wait(&mut self, id: JobId) -> i32 {
    let start = Instant::now();
    loop {
      for ev in self.shell.poll() {
        if let ShellEvent::Finished(j, exit) = ev
          && j == id
        {
          return exit;
        }
      }
      assert!(start.elapsed() < Duration::from_secs(20), "timed out; screen: {}", self.text(id));
      std::thread::sleep(Duration::from_millis(2));
    }
  }

  /// Polls until `f` holds for the job's screen text.
  fn wait_for(&mut self, id: JobId, what: &str, f: impl Fn(&str) -> bool) {
    let start = Instant::now();
    loop {
      self.shell.poll();
      if f(&self.text(id)) {
        return;
      }
      assert!(
        start.elapsed() < Duration::from_secs(20),
        "timed out waiting for {what}: {}",
        self.text(id)
      );
      std::thread::sleep(Duration::from_millis(2));
    }
  }

  fn text(&self, id: JobId) -> String { self.shell.job(id).unwrap().emulator.text() }

  fn run(&mut self, line: &str) -> (i32, String) {
    let id = self.start(line);
    let exit = self.wait(id);
    (exit, self.text(id))
  }
}

impl Drop for Fixture {
  fn drop(&mut self) { let _ = fs::remove_dir_all(&self.dir); }
}

#[test]
fn output_and_exit_codes() {
  need_zsh!();
  let mut f = Fixture::new("");
  assert_eq!(f.run("echo hello; echo world"), (0, "hello\nworld".into()));
  assert_eq!(f.run("false").0, 1);
  assert_eq!(f.run("exit 7").0, 7);
  assert_eq!(f.run("nonexistent-cmd-xyz 2>/dev/null").0, 127);
  let job = f.shell.jobs().last().unwrap();
  assert_eq!(job.state, JobState::Done { exit: 127 });
  assert!(job.duration.is_some());
  assert_eq!(f.shell.history().len(), 4);
  assert_eq!(f.shell.history().entries()[1].exit_code, 1);
  // The prompt remembers the previous command.
  assert_eq!(f.shell.prompt().last_exit, Some(127));
}

#[test]
fn rc_file_state_and_banner_discarded() {
  need_zsh!();
  let mut f = Fixture::new("echo BANNER\nalias rcal='echo from-rc'\nexport RC_VAR=1\n");
  let (exit, text) = f.run("rcal $RC_VAR");
  assert_eq!((exit, text.as_str()), (0, "from-rc 1"));
}

#[test]
fn cwd_persists() {
  need_zsh!();
  let mut f = Fixture::new("");
  fs::create_dir(f.work.join("sub")).unwrap();
  f.run("cd sub");
  assert_eq!(f.shell.cwd(), f.work.join("sub"));
  assert_eq!(f.run("pwd").1, f.work.join("sub").to_string_lossy());
}

#[test]
fn env_alias_function_source() {
  need_zsh!();
  let mut f = Fixture::new("");
  f.run("export FOO=1");
  assert_eq!(f.run("echo $FOO").1, "1");

  f.run("alias hi='echo aliased'");
  assert_eq!(f.run("hi").1, "aliased");

  f.run("greet() { echo \"hello $1\" }");
  assert_eq!(f.run("greet bob").1, "hello bob");

  let script = f.dir.join("script.zsh");
  fs::write(&script, "SRCVAR=fromscript\nsrcfn() { echo srcfn }\nexport SRCEXP=e\n").unwrap();
  f.run(&format!("source {}", script.display()));
  assert_eq!(f.run("echo $SRCVAR $SRCEXP; srcfn").1, "fromscript e\nsrcfn");
}

#[test]
fn colors_pass_through() {
  need_zsh!();
  let mut f = Fixture::new("");
  let id = f.start(r"printf '\e[31mred\e[0m plain'");
  f.wait(id);
  let job = f.shell.job(id).unwrap();
  assert_eq!(job.emulator.text(), "red plain");
  let line = job.emulator.screen_line(0).unwrap();
  let red = line.styles().any(|(s, _)| {
    s.foreground == Some(TerminalColor::Builtin { color: BuiltinColor::Red, bright: false })
  });
  assert!(red, "no red cells");
}

#[test]
fn interactive_input() {
  need_zsh!();
  let mut f = Fixture::new("");
  let id = f.start("read x; echo got $x");
  f.shell.send_input(id, b"abc\n");
  assert_eq!(f.wait(id), 0);
  assert!(f.text(id).contains("got abc"), "{}", f.text(id));
}

/// Ctrl-C reaches the foreground process through the pty's line discipline.
/// zsh is killed or exits with 130 before its exit hook can complete, so the
/// old state is kept (this is logged, not an error); either way the next
/// command works.
#[test]
fn ctrl_c_interrupts() {
  need_zsh!();
  let mut f = Fixture::new("");
  f.run("export KEEP=1");
  let id = f.start("sleep 30");
  std::thread::sleep(Duration::from_millis(300));
  let start = Instant::now();
  f.shell.send_input(id, &[0x03]);
  assert_eq!(f.wait(id), 130);
  assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
  assert_eq!(f.run("echo $KEEP").1, "1");
}

#[test]
fn signal_kills() {
  need_zsh!();
  let mut f = Fixture::new("");
  let id = f.start("sleep 30");
  std::thread::sleep(Duration::from_millis(200));
  f.shell.signal(id, 9);
  assert_eq!(f.wait(id), 137);
}

#[test]
fn alt_screen_detected() {
  need_zsh!();
  let mut f = Fixture::new("");
  let id = f.start(r"printf '\e[?1049h'; sleep 0.3");
  f.wait_for(id, "alt screen", |_| true);
  let start = Instant::now();
  while !f.shell.job(id).unwrap().emulator.alt_screen() {
    f.shell.poll();
    assert!(start.elapsed() < Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(2));
  }
  f.wait(id);
}

#[test]
fn resize_reaches_program() {
  need_zsh!();
  let mut f = Fixture::new("");
  let id = f.start("print start; read x; stty size");
  f.wait_for(id, "start", |t| t.contains("start"));
  f.shell.resize(id, Size { rows: 10, cols: 33 });
  f.shell.send_input(id, b"\n");
  f.wait(id);
  assert!(f.text(id).contains("10 33"), "{}", f.text(id));
  assert_eq!(f.shell.job(id).unwrap().emulator.size(), Size { rows: 10, cols: 33 });
}

#[test]
fn missing_cwd_falls_back_to_home() {
  need_zsh!();
  let mut f = Fixture::new("");
  let home = f.dir.join("home");
  fs::create_dir(&home).unwrap();
  f.run(&format!("export HOME={}", home.display()));
  fs::create_dir(f.work.join("gone")).unwrap();
  f.run("cd gone");
  fs::remove_dir(f.work.join("gone")).unwrap();
  assert_eq!(f.run("pwd").1, home.to_string_lossy());
  assert_eq!(f.shell.cwd(), home);
}

#[test]
fn ui_commands() {
  need_zsh!();
  let mut f = Fixture::new("");
  let ev = |r| match r {
    SubmitResult::Event(e) => e,
    other => panic!("{other:?}"),
  };
  assert_eq!(ev(f.shell.submit("exit", SIZE)), ShellEvent::Exit);
  assert_eq!(ev(f.shell.submit("clear", SIZE)), ShellEvent::Clear);
  assert_eq!(ev(f.shell.submit("history", SIZE)), ShellEvent::History);
  assert_eq!(ev(f.shell.submit("jobs", SIZE)), ShellEvent::Unsupported(UiCommand::Jobs));
  assert_eq!(ev(f.shell.submit("fg", SIZE)), ShellEvent::Unsupported(UiCommand::Fg(None)));
  assert!(f.shell.jobs().is_empty());
  // `exit 3` is not a UI command: zsh gets it.
  assert_eq!(f.run("exit 3").0, 3);
}

#[test]
fn child_gets_exact_env() {
  need_zsh!();
  let mut f = Fixture::new("");
  let (_, text) = f.run("print -r -- $TERM $COLORTERM");
  assert_eq!(text, "xterm-256color truecolor");
}
