//! Command lines handled by the UI instead of being sent to zsh. This is
//! exact-match checking on a few words, not a shell parser: anything with extra
//! arguments, pipes, `;`, etc. falls through to zsh.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCommand {
  Jobs,
  /// `fg [%n]`. `None` means the most recent job.
  Fg(Option<usize>),
  Bg(Option<usize>),
  /// `kill %n` or `kill -SIG %n`. `signal` is a signal number; `None` means
  /// the default (SIGTERM).
  Kill {
    job:    usize,
    signal: Option<i32>,
  },
  Exit,
  Clear,
  History,
}

pub fn parse_ui_command(line: &str) -> Option<UiCommand> {
  let mut words = line.split_whitespace();
  let first = words.next()?;
  let rest: Vec<&str> = words.collect();

  match (first, rest.as_slice()) {
    ("jobs", []) => Some(UiCommand::Jobs),
    ("exit", []) => Some(UiCommand::Exit),
    ("clear", []) => Some(UiCommand::Clear),
    ("history", []) => Some(UiCommand::History),
    ("fg", []) => Some(UiCommand::Fg(None)),
    ("fg", [j]) => Some(UiCommand::Fg(Some(parse_job(j)?))),
    ("bg", []) => Some(UiCommand::Bg(None)),
    ("bg", [j]) => Some(UiCommand::Bg(Some(parse_job(j)?))),
    ("kill", [j]) => Some(UiCommand::Kill { job: parse_job(j)?, signal: None }),
    ("kill", [sig, j]) => {
      let job = parse_job(j)?;
      Some(UiCommand::Kill { job, signal: Some(parse_signal(sig.strip_prefix('-')?)?) })
    }
    _ => None,
  }
}

fn parse_job(s: &str) -> Option<usize> {
  let n: usize = s.strip_prefix('%')?.parse().ok()?;
  if n == 0 { None } else { Some(n) }
}

fn parse_signal(s: &str) -> Option<i32> {
  if let Ok(n) = s.parse::<i32>() {
    return if (0..=64).contains(&n) { Some(n) } else { None };
  }
  let name = s.strip_prefix("SIG").unwrap_or(s);
  Some(match name {
    "HUP" => 1,
    "INT" => 2,
    "QUIT" => 3,
    "ILL" => 4,
    "TRAP" => 5,
    "ABRT" => 6,
    "BUS" => 7,
    "FPE" => 8,
    "KILL" => 9,
    "USR1" => 10,
    "SEGV" => 11,
    "USR2" => 12,
    "PIPE" => 13,
    "ALRM" => 14,
    "TERM" => 15,
    "CHLD" => 17,
    "CONT" => 18,
    "STOP" => 19,
    "TSTP" => 20,
    "TTIN" => 21,
    "TTOU" => 22,
    "WINCH" => 28,
    _ => return None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use UiCommand::*;

  #[test]
  fn simple() {
    assert_eq!(parse_ui_command("jobs"), Some(Jobs));
    assert_eq!(parse_ui_command("  exit  "), Some(Exit));
    assert_eq!(parse_ui_command("clear"), Some(Clear));
    assert_eq!(parse_ui_command("history"), Some(History));
  }

  #[test]
  fn fg_bg() {
    assert_eq!(parse_ui_command("fg"), Some(Fg(None)));
    assert_eq!(parse_ui_command("fg %2"), Some(Fg(Some(2))));
    assert_eq!(parse_ui_command("bg %1"), Some(Bg(Some(1))));
    assert_eq!(parse_ui_command("bg"), Some(Bg(None)));
    assert_eq!(parse_ui_command("fg 2"), None);
    assert_eq!(parse_ui_command("fg %x"), None);
    assert_eq!(parse_ui_command("fg %0"), None);
  }

  #[test]
  fn kill() {
    assert_eq!(parse_ui_command("kill %3"), Some(Kill { job: 3, signal: None }));
    assert_eq!(parse_ui_command("kill -9 %3"), Some(Kill { job: 3, signal: Some(9) }));
    assert_eq!(parse_ui_command("kill -KILL %1"), Some(Kill { job: 1, signal: Some(9) }));
    assert_eq!(parse_ui_command("kill -SIGINT %1"), Some(Kill { job: 1, signal: Some(2) }));
    assert_eq!(parse_ui_command("kill 123"), None);
    assert_eq!(parse_ui_command("kill -FOO %1"), None);
    assert_eq!(parse_ui_command("kill %1 %2"), None);
  }

  #[test]
  fn not_ui() {
    for l in [
      "",
      "   ",
      "jobs -l",
      "exit 1",
      "clear; ls",
      "history 5",
      "ls",
      "jobs | cat",
      "echo jobs",
      "jobsx",
    ] {
      assert_eq!(parse_ui_command(l), None, "{l:?}");
    }
  }
}
