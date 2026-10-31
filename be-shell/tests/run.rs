use be_shell::{Shell, ShellEvent};
use be_terminal::Size;
use std::time::{Duration, Instant};

const SIZE: Size = Size { rows: 24, cols: 80 };

fn run(line: &str) -> (String, i32) {
  let mut shell = Shell::new(std::env::temp_dir());
  let id = shell.submit(line, SIZE).unwrap();

  let deadline = Instant::now() + Duration::from_secs(10);
  let exit = loop {
    let finished = shell.poll().into_iter().find_map(|e| match e {
      ShellEvent::Finished(_, exit) => Some(exit),
      _ => None,
    });
    if let Some(exit) = finished {
      break exit;
    }
    assert!(Instant::now() < deadline, "{line:?} did not finish");
    std::thread::sleep(Duration::from_millis(2));
  };

  let state = shell.job(id).unwrap().emulator.state();
  let mut text = String::new();
  for row in 0..shell.job(id).unwrap().emulator.used_rows() {
    if let Some(line) = state.absolute_line(row) {
      text.push_str(line.to_string().trim_end());
      text.push('\n');
    }
  }
  (text, exit)
}

#[test]
fn prints_output() {
  let (text, exit) = run("echo hello; echo world");
  assert_eq!(text.trim_end(), "hello\nworld");
  assert_eq!(exit, 0);
}

#[test]
fn reports_exit_status() {
  let (_, exit) = run("exit 3");
  assert_eq!(exit, 3);
}
