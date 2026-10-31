//! The text of the shell prompt, in the style of Zim's `asciiship` theme, and
//! how a command is wrapped after it. Nothing here touches rendering: the
//! prompt is a list of styled runs that the view draws in the terminal font.
//!
//! The prompt is a blank row, an info line, then a symbol line which the
//! command is typed after:
//!
//! ```text
//! 
//! ~/code/proj on main [!] via .venv took 12s
//! 127 % cargo build
//! ```

use std::path::Path;

use be_shell::{GitHead, GitStatus, PromptContext, format_duration};
use be_terminal::BuiltinColor;

use crate::Color;

use super::terminal_paint::ansi_color;

/// `zstyle ':zim:duration-info' threshold`: durations shorter than this are
/// not shown.
const DURATION_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptColor {
  /// The terminal's default foreground.
  Default,
  Red,
  Green,
  Yellow,
  Magenta,
  Cyan,
}

impl PromptColor {
  /// The color drawn, or `None` for the default foreground.
  pub fn color(self) -> Option<Color> {
    Some(ansi_color(match self {
      PromptColor::Default => return None,
      PromptColor::Red => BuiltinColor::Red,
      PromptColor::Green => BuiltinColor::Green,
      PromptColor::Yellow => BuiltinColor::Yellow,
      PromptColor::Magenta => BuiltinColor::Magenta,
      PromptColor::Cyan => BuiltinColor::Cyan,
    }))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
  pub text:  String,
  pub color: PromptColor,
  pub bold:  bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRows {
  /// The line above the symbol line.
  pub info:   Vec<Run>,
  /// The symbol line, up to where the command starts.
  pub symbol: Vec<Run>,
}

fn run(text: impl Into<String>, color: PromptColor, bold: bool) -> Run {
  Run { text: text.into(), color, bold }
}

pub fn plain(runs: &[Run]) -> String { runs.iter().map(|r| r.text.as_str()).collect() }

/// `%~`: the working directory, with `$HOME` as `~`.
fn tilde_path(cwd: &Path, home: Option<&Path>) -> String {
  if let Some(home) = home.filter(|h| h.as_os_str().len() > 1)
    && let Ok(rest) = cwd.strip_prefix(home)
  {
    return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
  }
  cwd.display().to_string()
}

/// The characters of zim's `status` git format: stashed, unindexed, indexed,
/// ahead, behind.
fn git_flags(status: &GitStatus) -> String {
  [
    (status.stashed, '$'),
    (status.unindexed, '!'),
    (status.indexed, '+'),
    (status.ahead, '>'),
    (status.behind, '<'),
  ]
  .into_iter()
  .filter_map(|(set, c)| set.then_some(c))
  .collect()
}

/// Builds the prompt as `PS1` in `.zshrc` does with asciiship.
pub fn prompt_rows(ctx: &PromptContext) -> PromptRows {
  use PromptColor::*;

  let mut info = vec![];
  if let Some(user) = &ctx.user {
    if ctx.root {
      info.push(run(user, Red, true));
      info.push(run(" in ", Default, false));
    } else if ctx.ssh {
      info.push(run(user, Yellow, true));
      info.push(run(" in ", Default, false));
    }
  }
  if ctx.ssh
    && let Some(host) = &ctx.host
  {
    info.push(run(host, Green, true));
    info.push(run(" in ", Default, false));
  }

  info.push(run(tilde_path(&ctx.cwd, ctx.home.as_deref()), Cyan, true));

  if let Some(head) = &ctx.git {
    info.push(run(" on ", Default, false));
    match head {
      GitHead::Branch(name) => info.push(run(name, Magenta, true)),
      GitHead::Detached(sha) => {
        info.push(run("HEAD ", Magenta, true));
        info.push(run(format!("({sha})"), Green, true));
      }
    }
    if let Some(status) = ctx.git_status() {
      if let Some(action) = &status.action {
        info.push(run(format!(" ({action})"), Yellow, true));
      }
      let flags = git_flags(status);
      if !flags.is_empty() {
        info.push(run(format!(" [{flags}]"), Red, true));
      }
    }
  }

  if let Some(venv) = &ctx.venv {
    info.push(run(" via ", Default, false));
    info.push(run(venv, Yellow, true));
  }

  if let Some(duration) = ctx.duration.filter(|d| *d >= DURATION_THRESHOLD)
    && let Some(text) = format_duration(duration)
  {
    info.push(run(" took ", Default, false));
    info.push(run(text, Yellow, true));
  }

  let status = ctx.last_exit.unwrap_or(0);
  let mut symbol = vec![];
  if status != 0 {
    symbol.push(run(format!("{status} "), Red, true));
  }
  let color = if status == 0 { Green } else { Red };
  symbol.push(run(if ctx.root { "#" } else { "%" }, color, true));
  symbol.push(run(" ", Default, false));

  PromptRows { info, symbol }
}

/// Where each character of a command goes when it is typed after a prompt
/// that takes `prefix` columns of the first row, in a terminal `cols` wide.
/// Rows after the first start at column 0, as a terminal draws them.
pub struct Wrapped {
  /// For every char of the command: its byte offset, row, and column.
  cells:    Vec<(usize, usize, usize)>,
  /// Where the next char would go.
  end:      (usize, usize),
  /// The text of each row, not including the prefix.
  pub rows: Vec<String>,
}

impl Wrapped {
  pub fn new(prefix: usize, text: &str, cols: usize) -> Wrapped {
    let cols = cols.max(1);
    let (mut row, mut col) = (0, prefix);
    let mut cells = vec![];
    let mut rows = vec![String::new()];
    for (i, c) in text.char_indices() {
      if c == '\n' {
        cells.push((i, row, col));
        row += 1;
        col = 0;
        rows.push(String::new());
        continue;
      }
      if col >= cols {
        row += 1;
        col = 0;
        rows.push(String::new());
      }
      cells.push((i, row, col));
      rows[row].push(c);
      col += 1;
    }
    if col >= cols {
      row += 1;
      col = 0;
      rows.push(String::new());
    }
    Wrapped { cells, end: (row, col), rows }
  }

  /// The number of rows the command takes, at least one.
  pub fn height(&self) -> usize { self.rows.len() }

  /// The row and column of the cursor, which is at byte `cursor`.
  pub fn cursor(&self, cursor: usize) -> (usize, usize) {
    self.cells.iter().find(|c| c.0 >= cursor).map_or(self.end, |c| (c.1, c.2))
  }
}

#[cfg(test)]
mod tests {
  use std::{path::PathBuf, process::Command, time::Duration};

  use expect_test::{Expect, expect};

  use super::*;

  fn ctx(cwd: &str) -> PromptContext {
    let mut c = PromptContext::bare(Path::new(cwd));
    c.home = Some(PathBuf::from("/home/me"));
    c.user = Some("me".into());
    c.host = Some("box".into());
    c
  }

  fn ctx_git(cwd: &str, head: GitHead, status: GitStatus) -> PromptContext {
    let mut c = ctx(cwd);
    c.git = Some(head);
    c.set_git_status(status);
    c
  }

  fn branch(name: &str) -> GitHead { GitHead::Branch(name.into()) }

  /// Plain text on two lines, then the runs with their colors: `{cyan,b|text}`
  /// is bold cyan, and unstyled text is bare.
  fn show(c: &PromptContext) -> String {
    let rows = prompt_rows(c);
    let fmt = |runs: &[Run]| {
      runs
        .iter()
        .map(|r| match (r.color, r.bold) {
          (PromptColor::Default, false) => r.text.clone(),
          (color, bold) => {
            let color = format!("{color:?}").to_lowercase();
            format!("{{{color}{}|{}}}", if bold { ",b" } else { "" }, r.text)
          }
        })
        .collect::<String>()
    };
    // Quotes and `$` show where lines end, as trailing spaces matter.
    format!(
      "'{}'\n'{}'\n{}$\n{}$",
      plain(&rows.info),
      plain(&rows.symbol),
      fmt(&rows.info),
      fmt(&rows.symbol)
    )
  }

  fn check(c: &PromptContext, expect: Expect) { expect.assert_eq(&show(c)); }

  #[test]
  fn directories() {
    check(
      &ctx("/etc/nginx"),
      expect![@r#"
        '/etc/nginx'
        '% '
        {cyan,b|/etc/nginx}$
        {green,b|%} $"#],
    );
    check(
      &ctx("/home/me"),
      expect![@r#"
        '~'
        '% '
        {cyan,b|~}$
        {green,b|%} $"#],
    );
    check(
      &ctx("/home/me/x"),
      expect![@r#"
        '~/x'
        '% '
        {cyan,b|~/x}$
        {green,b|%} $"#],
    );
    // A sibling that merely shares the prefix is not under home.
    assert_eq!(plain(&prompt_rows(&ctx("/home/media")).info), "/home/media");
  }

  #[test]
  fn git_states() {
    let s = GitStatus::default;
    check(
      &ctx_git("/w", branch("main"), s()),
      expect![@r#"
        '/w on main'
        '% '
        {cyan,b|/w} on {magenta,b|main}$
        {green,b|%} $"#],
    );
    check(
      &ctx_git("/w", branch("main"), GitStatus { unindexed: true, ..s() }),
      expect![@r#"
        '/w on main [!]'
        '% '
        {cyan,b|/w} on {magenta,b|main}{red,b| [!]}$
        {green,b|%} $"#],
    );
    check(
      &ctx_git(
        "/w",
        branch("main"),
        GitStatus { stashed: true, unindexed: true, indexed: true, ahead: true, ..s() },
      ),
      expect![@r#"
        '/w on main [$!+>]'
        '% '
        {cyan,b|/w} on {magenta,b|main}{red,b| [$!+>]}$
        {green,b|%} $"#],
    );
    check(
      &ctx_git("/w", branch("dev"), GitStatus { behind: true, ..s() }),
      expect![@r#"
        '/w on dev [<]'
        '% '
        {cyan,b|/w} on {magenta,b|dev}{red,b| [<]}$
        {green,b|%} $"#],
    );
    check(
      &ctx_git("/w", GitHead::Detached("abc1234".into()), s()),
      expect![@r#"
        '/w on HEAD (abc1234)'
        '% '
        {cyan,b|/w} on {magenta,b|HEAD }{green,b|(abc1234)}$
        {green,b|%} $"#],
    );
    check(
      &ctx_git(
        "/w",
        branch("topic"),
        GitStatus { action: Some("REBASE-I".into()), indexed: true, ..s() },
      ),
      expect![@r#"
        '/w on topic (REBASE-I) [+]'
        '% '
        {cyan,b|/w} on {magenta,b|topic}{yellow,b| (REBASE-I)}{red,b| [+]}$
        {green,b|%} $"#],
    );
  }

  #[test]
  fn git_status_not_computed_yet() {
    let mut c = ctx("/w");
    c.git = Some(branch("main"));
    assert_eq!(plain(&prompt_rows(&c).info), "/w on main");
  }

  #[test]
  fn venv_root_and_ssh() {
    let mut c = ctx("/w");
    c.venv = Some(".venv".into());
    check(
      &c,
      expect![@r#"
        '/w via .venv'
        '% '
        {cyan,b|/w} via {yellow,b|.venv}$
        {green,b|%} $"#],
    );

    let mut c = ctx("/root");
    c.root = true;
    c.user = Some("root".into());
    check(
      &c,
      expect![@r#"
        'root in /root'
        '# '
        {red,b|root} in {cyan,b|/root}$
        {green,b|#} $"#],
    );

    let mut c = ctx("/w");
    c.ssh = true;
    check(
      &c,
      expect![@r#"
        'me in box in /w'
        '% '
        {yellow,b|me} in {green,b|box} in {cyan,b|/w}$
        {green,b|%} $"#],
    );
  }

  #[test]
  fn exit_status_and_duration() {
    let mut c = ctx("/w");
    c.last_exit = Some(127);
    check(
      &c,
      expect![@r#"
        '/w'
        '127 % '
        {cyan,b|/w}$
        {red,b|127 }{red,b|%} $"#],
    );

    c.last_exit = Some(0);
    c.duration = Some(Duration::from_millis(1900));
    assert_eq!(plain(&prompt_rows(&c).info), "/w");
    c.duration = Some(Duration::from_millis(2000));
    check(
      &c,
      expect![@r#"
        '/w took 2s'
        '% '
        {cyan,b|/w} took {yellow,b|2s}$
        {green,b|%} $"#],
    );
    c.duration = Some(Duration::from_millis(65_400));
    assert_eq!(plain(&prompt_rows(&c).info), "/w took 1m5s");
  }

  #[test]
  fn wrapping() {
    let w = Wrapped::new(2, "abcdef", 5);
    assert_eq!(w.rows, ["abc", "def"]);
    assert_eq!(w.cursor(0), (0, 2));
    assert_eq!(w.cursor(3), (1, 0));
    assert_eq!(w.cursor(6), (1, 3));

    // The cursor after a full row is on the next one.
    let w = Wrapped::new(2, "abc", 5);
    assert_eq!(w.height(), 2);
    assert_eq!(w.cursor(3), (1, 0));

    let w = Wrapped::new(2, "a\nb", 80);
    assert_eq!(w.rows, ["a", "b"]);
    assert_eq!(w.cursor(1), (0, 3));
    assert_eq!(w.cursor(2), (1, 0));
    assert_eq!(w.cursor(3), (1, 1));

    let w = Wrapped::new(2, "", 80);
    assert_eq!(w.height(), 1);
    assert_eq!(w.cursor(0), (0, 2));
  }

  // Compares the plain text with what zsh itself prints for the asciiship PS1
  // (without the nested-shell and background-job pieces, which we don't
  // show). Skipped when zsh isn't installed.
  fn zsh_expand(
    pwd: &str,
    home: &str,
    status: i32,
    venv: &str,
    ssh: bool,
    git_prompt: &str,
    duration_info: &str,
  ) -> Option<String> {
    let script = r#"
      setopt promptsubst
      cd -- $CASE_PWD
      typeset -gA git_info
      git_info[prompt]=$GIT_PROMPT
      duration_info=$DURATION_INFO
      PS1='
%(!.%B%F{red}%n%f%b in .${SSH_TTY:+"%B%F{yellow}%n%f%b in "})${SSH_TTY:+"%B%F{green}%m%f%b in "}%B%F{cyan}%~%f%b${(e)git_info[prompt]}${VIRTUAL_ENV:+" via %B%F{yellow}${VIRTUAL_ENV:t}%f%b"}${duration_info}
%B%(?.%F{green}.%F{red}%? )%#%f%b '
      (exit $STATUS)
      print -rP -- "$PS1"
    "#;
    let mut cmd = Command::new("zsh");
    cmd
      .args(["-f", "-c", script])
      .env("HOME", home)
      .env("CASE_PWD", pwd)
      .env("STATUS", status.to_string());
    cmd.env("GIT_PROMPT", git_prompt).env("DURATION_INFO", duration_info);
    cmd.env_remove("VIRTUAL_ENV").env_remove("SSH_TTY");
    if !venv.is_empty() {
      cmd.env("VIRTUAL_ENV", venv);
    }
    if ssh {
      cmd.env("SSH_TTY", "/dev/pts/0");
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
      return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;

    // Strip color escapes.
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
      if c == '\u{1b}' {
        for c in chars.by_ref() {
          if c.is_ascii_alphabetic() {
            break;
          }
        }
      } else {
        plain.push(c);
      }
    }
    Some(plain.strip_suffix('\n').unwrap_or(&plain).to_string())
  }

  #[test]
  fn matches_zsh() {
    let Some(me) = zsh_user_and_host() else {
      eprintln!("skipping: zsh is not available");
      return;
    };

    struct Case {
      cwd:      &'static str,
      status:   i32,
      venv:     &'static str,
      ssh:      bool,
      git:      Option<(GitHead, GitStatus)>,
      duration: Option<u64>,
    }
    // Real directories, as zsh needs to be in them.
    let base = std::env::temp_dir().join(format!("be-prompt-zsh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home/x/y")).unwrap();
    std::fs::create_dir_all(base.join("w")).unwrap();
    let home = base.join("home");
    let cases = [
      Case {
        cwd:      "/etc",
        status:   0,
        venv:     "",
        ssh:      false,
        git:      None,
        duration: None,
      },
      Case {
        cwd:      "home",
        status:   0,
        venv:     "",
        ssh:      false,
        git:      None,
        duration: None,
      },
      Case {
        cwd:      "home/x/y",
        status:   127,
        venv:     "",
        ssh:      false,
        git:      None,
        duration: None,
      },
      Case {
        cwd:      "w",
        status:   0,
        venv:     "/p/.venv",
        ssh:      true,
        git:      None,
        duration: Some(3),
      },
      Case {
        cwd:      "w",
        status:   2,
        venv:     "",
        ssh:      false,
        git:      None,
        duration: Some(65),
      },
      Case {
        cwd:      "w",
        status:   0,
        venv:     "",
        ssh:      false,
        git:      Some((branch("main"), GitStatus::default())),
        duration: None,
      },
      Case {
        cwd:      "w",
        status:   1,
        venv:     "",
        ssh:      false,
        git:      Some((
          branch("topic"),
          GitStatus {
            action:    Some("REBASE".into()),
            stashed:   true,
            unindexed: true,
            indexed:   true,
            ahead:     true,
            behind:    true,
          },
        )),
        duration: Some(7384),
      },
      Case {
        cwd:      "w",
        status:   0,
        venv:     "",
        ssh:      false,
        git:      Some((GitHead::Detached("abc1234".into()), GitStatus::default())),
        duration: None,
      },
    ];

    for case in cases {
      // The git and duration segments exactly as asciiship's zstyles build
      // them, with the prompt escapes left for print -P.
      let git_prompt = match &case.git {
        None => String::new(),
        Some((head, status)) => {
          let mut s = String::from(" on %B%F{magenta}");
          match head {
            GitHead::Branch(b) => s.push_str(b),
            GitHead::Detached(c) => s.push_str(&format!("HEAD %F{{green}}({c})")),
          }
          if let Some(a) = &status.action {
            s.push_str(&format!(" %F{{yellow}}({a})"));
          }
          let flags = git_flags(status);
          if !flags.is_empty() {
            s.push_str(&format!(" %F{{red}}[{}]", flags.replace('$', "\\$")));
          }
          s.push_str("%f%b");
          s
        }
      };
      let dur = case.duration.map(Duration::from_secs);
      let dur_info = match dur.filter(|d| *d >= DURATION_THRESHOLD).and_then(format_duration) {
        Some(d) => format!(" took %B%F{{yellow}}{d}%f%b"),
        None => String::new(),
      };

      let cwd = base.join(case.cwd);
      let cwd = cwd.to_str().unwrap();
      let want = zsh_expand(
        cwd,
        home.to_str().unwrap(),
        case.status,
        case.venv,
        case.ssh,
        &git_prompt,
        &dur_info,
      );
      let Some(want) = want else {
        eprintln!("skipping: zsh failed");
        return;
      };

      let mut c = ctx(cwd);
      c.home = Some(home.clone());
      c.user = Some(me.0.clone());
      c.host = Some(me.1.clone());
      c.ssh = case.ssh;
      c.venv = Path::new(case.venv).file_name().map(|n| n.to_string_lossy().into_owned());
      c.last_exit = Some(case.status);
      c.duration = dur;
      if let Some((head, status)) = case.git {
        c.git = Some(head);
        c.set_git_status(status);
      }
      let rows = prompt_rows(&c);
      let got = format!("\n{}\n{}", plain(&rows.info), plain(&rows.symbol));
      assert_eq!(got, want, "case {}", case.cwd);
    }
    let _ = std::fs::remove_dir_all(&base);
    {}
  }

  fn zsh_user_and_host() -> Option<(String, String)> {
    let out = Command::new("zsh").args(["-f", "-c", "print -P '%n %m'"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut parts = text.split_whitespace();
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
  }
}
