//! The elements of the shell prompt, in the style of Zim's `asciiship` theme,
//! and how a command is wrapped after it. Nothing here touches rendering: the
//! info line is a list of elements placed on terminal cell columns, which the
//! view draws as text and rounded "pills".
//!
//! The prompt is a blank row, an info line, then a symbol line which the
//! command is typed after:
//!
//! ```text
//! 
//! ~/code/proj  main  $!                                   [130 SIGINT] [12s]
//! % cargo build
//! ```
//!
//! Because the right side shows the result of the block's own command, the
//! frozen prompt of a block does not depend on the command before it.

use std::{path::Path, time::Duration};

use be_shell::{GitHead, GitStatus, PromptContext, format_duration};
use be_terminal::BuiltinColor;

use crate::{Color, view::terminal_paint::ansi_color};

/// Columns left empty between two elements.
const GAP: usize = 1;
/// Columns kept empty between the left and right sides.
const SIDE_GAP: usize = 2;
/// The shortest the working directory is cut to, ellipsis included.
const MIN_CWD: usize = 2;
/// Cut-off marker of a long working directory.
pub const ELLIPSIS: char = '\u{2026}';
/// Columns the chevron takes at the start of the info line: the glyph and a
/// blank cell. Every prompt keeps them, even without a chevron, so that
/// freezing the live prompt into a block moves nothing.
pub const CHEVRON_COLS: usize = 2;
/// Columns the spinner takes.
const SPINNER_COLS: usize = 2;

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

/// How the command of a prompt is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockResult {
  /// The live prompt: nothing has been run.
  Live,
  Running,
  Done {
    exit:     i32,
    duration: Duration,
  },
}

/// The collapse chevron at the start of the info line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chevron {
  /// The live prompt: the cells are blank.
  Hidden,
  Collapsed,
  Expanded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
  Left,
  Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
  /// Text is empty; the state is in `Element::chevron`.
  Chevron,
  User,
  Host,
  Cwd,
  Branch,
  GitFlags,
  GitAction,
  Venv,
  Exit,
  Duration,
  Spinner,
}

impl Kind {
  /// Whether the element sits on a tinted rounded background.
  pub fn is_pill(self) -> bool { !matches!(self, Kind::Cwd | Kind::Spinner | Kind::Chevron) }
}

/// One thing on the info line, `col_len` cells starting at `col_start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
  pub kind:      Kind,
  pub text:      String,
  pub col_start: usize,
  pub col_len:   usize,
  pub color:     PromptColor,
  /// From this char of `text` on, the text has another color.
  pub tail:      Option<(usize, PromptColor)>,
  pub side:      Side,
  /// For `Kind::Chevron` only.
  pub chevron:   Chevron,
}

fn element(kind: Kind, text: impl Into<String>, color: PromptColor, side: Side) -> Element {
  let text = text.into();
  let col_len = if kind == Kind::Spinner { SPINNER_COLS } else { text.chars().count() };
  Element { kind, text, col_start: 0, col_len, color, tail: None, side, chevron: Chevron::Hidden }
}

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

/// The name of the signal a shell reports as `128 + n`, for the common ones.
fn signal_name(exit: i32) -> Option<&'static str> {
  Some(match exit {
    129 => "SIGHUP",
    130 => "SIGINT",
    131 => "SIGQUIT",
    134 => "SIGABRT",
    137 => "SIGKILL",
    139 => "SIGSEGV",
    141 => "SIGPIPE",
    143 => "SIGTERM",
    _ => return None,
  })
}

/// The symbol that starts the line the command is typed on.
pub fn symbol(ctx: &PromptContext) -> &'static str { if ctx.root { "#" } else { "%" } }

/// The columns of the symbol line before the command: the symbol and a space.
pub const SYMBOL_COLS: usize = 2;

/// Green, or red for a finished command that failed.
pub fn symbol_color(result: &BlockResult) -> PromptColor {
  match result {
    BlockResult::Done { exit, .. } if *exit != 0 => PromptColor::Red,
    _ => PromptColor::Green,
  }
}

fn left_elements(ctx: &PromptContext) -> Vec<Element> {
  use Kind::*;
  use PromptColor::*;
  use Side::Left;

  let mut out = vec![];
  if let Some(user) = &ctx.user {
    if ctx.root {
      out.push(element(User, user, Red, Left));
    } else if ctx.ssh {
      out.push(element(User, user, Yellow, Left));
    }
  }
  if ctx.ssh
    && let Some(host) = &ctx.host
  {
    out.push(element(Host, host, Green, Left));
  }

  out.push(element(Cwd, tilde_path(&ctx.cwd, ctx.home.as_deref()), Cyan, Left));

  if let Some(head) = &ctx.git {
    match head {
      GitHead::Branch(name) => out.push(element(Branch, name, Magenta, Left)),
      GitHead::Detached(sha) => {
        let mut e = element(Branch, format!("HEAD {sha}"), Magenta, Left);
        e.tail = Some((5, Green));
        out.push(e);
      }
    }
    // The status shows up later; it only adds elements after the branch.
    if let Some(status) = ctx.git_status() {
      let flags = git_flags(status);
      if !flags.is_empty() {
        out.push(element(GitFlags, flags, Red, Left));
      }
      if let Some(action) = &status.action {
        out.push(element(GitAction, action, Yellow, Left));
      }
    }
  }

  if let Some(venv) = &ctx.venv {
    out.push(element(Venv, venv, Yellow, Left));
  }
  out
}

/// What the right side shows, in order from left to right. Elements are
/// dropped from the end when there is no room, so the one dropped first
/// (the duration) comes first.
fn right_elements(result: &BlockResult) -> Vec<Element> {
  use PromptColor::*;
  use Side::Right;

  let mut out = vec![];
  match result {
    BlockResult::Live => {}
    BlockResult::Running => out.push(element(Kind::Spinner, "", Default, Right)),
    BlockResult::Done { exit, duration } => {
      if let Some(text) = format_duration(*duration) {
        out.push(element(Kind::Duration, text, Yellow, Right));
      }
      if *exit != 0 {
        let text = match signal_name(*exit) {
          Some(name) => format!("{exit} {name}"),
          None => exit.to_string(),
        };
        out.push(element(Kind::Exit, text, Red, Right));
      }
    }
  }
  out
}

fn side_width(elements: &[Element]) -> usize {
  elements.iter().map(|e| e.col_len).sum::<usize>() + GAP * elements.len().saturating_sub(1)
}

/// The last `max` chars of `text`, starting with an ellipsis if it was cut.
fn truncate_start(text: &str, max: usize) -> String {
  let count = text.chars().count();
  if count <= max {
    return text.to_string();
  }
  let keep = max.saturating_sub(1);
  std::iter::once(ELLIPSIS).chain(text.chars().skip(count - keep)).collect()
}

/// The elements of the info line of a prompt in a terminal `cols` wide.
///
/// The first `CHEVRON_COLS` columns hold the chevron (nothing for
/// `Chevron::Hidden`); the rest of the left side starts after them.
///
/// The left side is the working directory and what is known about the
/// environment, in order, with a gap of one column between elements; it never
/// moves when more is learned (git status only adds elements at its end). The
/// right side is the result of the block, right-aligned. When the two don't
/// fit, the duration is dropped first, then the exit code, and then the
/// working directory is cut short from the left. Nothing ever overlaps.
pub fn prompt_elements(
  ctx: &PromptContext,
  result: &BlockResult,
  chevron: Chevron,
  cols: usize,
) -> Vec<Element> {
  let mut left = left_elements(ctx);
  let mut right = right_elements(result);

  // Drop the right side, last element (exit code or spinner) last.
  while !right.is_empty() && CHEVRON_COLS + side_width(&left) + SIDE_GAP + side_width(&right) > cols
  {
    right.remove(0);
  }

  if right.is_empty() && CHEVRON_COLS + side_width(&left) > cols {
    // Everything but the directory, including the gap next to it.
    let cwd_len = left.iter().find(|e| e.kind == Kind::Cwd).map_or(0, |e| e.col_len);
    let others = CHEVRON_COLS + side_width(&left) - cwd_len;
    let room = cols.saturating_sub(others).max(MIN_CWD);
    if let Some(cwd) = left.iter_mut().find(|e| e.kind == Kind::Cwd) {
      cwd.text = truncate_start(&cwd.text, room);
      cwd.col_len = cwd.text.chars().count();
    }
    // Still too much (a very narrow pane): drop what follows the directory.
    while CHEVRON_COLS + side_width(&left) > cols
      && left.last().is_some_and(|e| e.kind != Kind::Cwd)
    {
      left.pop();
    }
  }

  let mut col = CHEVRON_COLS;
  for e in &mut left {
    e.col_start = col;
    col += e.col_len + GAP;
  }
  let mut end = cols;
  for e in right.iter_mut().rev() {
    e.col_start = end.saturating_sub(e.col_len);
    end = e.col_start.saturating_sub(GAP);
  }
  left.extend(right);
  if chevron != Chevron::Hidden {
    let mut e = element(Kind::Chevron, "", PromptColor::Default, Side::Left);
    e.col_len = CHEVRON_COLS;
    e.chevron = chevron;
    left.insert(0, e);
  }
  left
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
  use std::path::PathBuf;

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

  fn done(exit: i32, ms: u64) -> BlockResult {
    BlockResult::Done { exit, duration: Duration::from_millis(ms) }
  }

  /// The row as it would be laid out on the cells, then one line per element:
  /// `kind side col_start+col_len color 'text'`.
  fn show(c: &PromptContext, result: BlockResult, cols: usize) -> String {
    let elements = prompt_elements(c, &result, Chevron::Hidden, cols);
    let mut row = vec![' '; cols];
    let mut lines = vec![];
    for e in &elements {
      assert!(e.col_start + e.col_len <= cols, "{e:?} overflows {cols}");
      for (i, ch) in e.text.chars().enumerate() {
        row[e.col_start + i] = ch;
      }
      let tail = e.tail.map(|(i, c)| format!(" tail {i}:{c:?}")).unwrap_or_default();
      lines.push(format!(
        "{:?} {:?} {}+{} {:?} {:?}{tail}",
        e.kind, e.side, e.col_start, e.col_len, e.color, e.text
      ));
    }
    for pair in elements.windows(2) {
      let (a, b) = (&pair[0], &pair[1]);
      let gap = if a.side == b.side { GAP } else { SIDE_GAP };
      assert!(a.col_start + a.col_len + gap <= b.col_start, "{a:?} touches {b:?}");
    }
    let row: String = row.into_iter().collect();
    format!("|{row}|\n{}", lines.join("\n")).replace(ELLIPSIS, "^")
  }

  fn check(c: &PromptContext, result: BlockResult, cols: usize, expect: Expect) {
    expect.assert_eq(&show(c, result, cols));
  }

  #[test]
  fn directories() {
    check(
      &ctx("/etc/nginx"),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /etc/nginx                  |
        Cwd Left 2+10 Cyan "/etc/nginx""#],
    );
    check(
      &ctx("/home/me"),
      BlockResult::Live,
      30,
      expect![@r#"
        |  ~                           |
        Cwd Left 2+1 Cyan "~""#],
    );
    check(
      &ctx("/home/me/x"),
      BlockResult::Live,
      30,
      expect![@r#"
        |  ~/x                         |
        Cwd Left 2+3 Cyan "~/x""#],
    );
    // A sibling that merely shares the prefix is not under home.
    assert_eq!(tilde_path(Path::new("/home/media"), Some(Path::new("/home/me"))), "/home/media");
  }

  #[test]
  fn git_states() {
    let s = GitStatus::default;
    check(
      &ctx_git("/w", branch("main"), s()),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w main                     |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main""#],
    );
    // The status has not been computed yet.
    let mut c = ctx("/w");
    c.git = Some(branch("main"));
    check(
      &c,
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w main                     |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main""#],
    );

    check(
      &ctx_git("/w", branch("dev"), GitStatus { stashed: true, ..s() }),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w dev $                    |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+3 Magenta "dev"
        GitFlags Left 9+1 Red "$""#],
    );
    check(
      &ctx_git("/w", branch("dev"), GitStatus { unindexed: true, ..s() }),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w dev !                    |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+3 Magenta "dev"
        GitFlags Left 9+1 Red "!""#],
    );
    check(
      &ctx_git("/w", branch("dev"), GitStatus { indexed: true, ..s() }),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w dev +                    |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+3 Magenta "dev"
        GitFlags Left 9+1 Red "+""#],
    );
    check(
      &ctx_git("/w", branch("dev"), GitStatus { ahead: true, ..s() }),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w dev >                    |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+3 Magenta "dev"
        GitFlags Left 9+1 Red ">""#],
    );
    check(
      &ctx_git("/w", branch("dev"), GitStatus { behind: true, ..s() }),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w dev <                    |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+3 Magenta "dev"
        GitFlags Left 9+1 Red "<""#],
    );
    check(
      &ctx_git(
        "/w",
        branch("main"),
        GitStatus {
          stashed:   true,
          unindexed: true,
          indexed:   true,
          ahead:     true,
          behind:    true,
          action:    None,
        },
      ),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w main $!+><               |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        GitFlags Left 10+5 Red "$!+><""#],
    );
    check(
      &ctx_git("/w", GitHead::Detached("abc1234".into()), s()),
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w HEAD abc1234             |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+12 Magenta "HEAD abc1234" tail 5:Green"#],
    );
    check(
      &ctx_git(
        "/w",
        branch("topic"),
        GitStatus { action: Some("REBASE-I".into()), indexed: true, ..s() },
      ),
      BlockResult::Live,
      40,
      expect![@r#"
        |  /w topic + REBASE-I                   |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+5 Magenta "topic"
        GitFlags Left 11+1 Red "+"
        GitAction Left 13+8 Yellow "REBASE-I""#],
    );
  }

  #[test]
  fn git_status_arriving_does_not_shift_anything() {
    let mut before = ctx("/w");
    before.git = Some(branch("main"));
    let after =
      ctx_git("/w", branch("main"), GitStatus { unindexed: true, ..GitStatus::default() });
    let a = prompt_elements(&before, &BlockResult::Live, Chevron::Hidden, 40);
    let b = prompt_elements(&after, &BlockResult::Live, Chevron::Hidden, 40);
    assert_eq!(a[..], b[..a.len()]);
    assert_eq!(b.len(), a.len() + 1);
  }

  #[test]
  fn freezing_the_prompt_moves_nothing() {
    let mut c = ctx_git("/w", branch("main"), GitStatus { indexed: true, ..GitStatus::default() });
    c.venv = Some(".venv".into());
    let live = prompt_elements(&c, &BlockResult::Live, Chevron::Hidden, 40);
    for chevron in [Chevron::Collapsed, Chevron::Expanded] {
      let frozen = prompt_elements(&c, &BlockResult::Live, chevron, 40);
      assert_eq!(frozen[0].kind, Kind::Chevron);
      assert_eq!((frozen[0].col_start, frozen[0].col_len), (0, CHEVRON_COLS));
      assert_eq!(frozen[0].chevron, chevron);
      assert_eq!(frozen[1..], live[..]);
    }
    // The first element after the chevron starts right after its cells.
    assert_eq!(live[0].col_start, CHEVRON_COLS);
  }

  #[test]
  fn venv_root_and_ssh() {
    let mut c = ctx("/w");
    c.venv = Some(".venv".into());
    check(
      &c,
      BlockResult::Live,
      30,
      expect![@r#"
        |  /w .venv                    |
        Cwd Left 2+2 Cyan "/w"
        Venv Left 5+5 Yellow ".venv""#],
    );

    let mut c = ctx("/root");
    c.root = true;
    c.user = Some("root".into());
    check(
      &c,
      BlockResult::Live,
      30,
      expect![@r#"
        |  root /root                  |
        User Left 2+4 Red "root"
        Cwd Left 7+5 Cyan "/root""#],
    );
    assert_eq!(symbol(&c), "#");

    let mut c = ctx("/w");
    c.ssh = true;
    check(
      &c,
      BlockResult::Live,
      30,
      expect![@r#"
        |  me box /w                   |
        User Left 2+2 Yellow "me"
        Host Left 5+3 Green "box"
        Cwd Left 9+2 Cyan "/w""#],
    );
  }

  #[test]
  fn results() {
    let c = ctx_git("/w", branch("main"), GitStatus::default());
    check(
      &c,
      done(0, 100),
      40,
      expect![@r#"
        |  /w main                               |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main""#],
    );
    check(
      &c,
      done(1, 100),
      40,
      expect![@r#"
        |  /w main                              1|
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Exit Right 39+1 Red "1""#],
    );
    check(
      &c,
      done(130, 100),
      40,
      expect![@r#"
        |  /w main                     130 SIGINT|
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Exit Right 30+10 Red "130 SIGINT""#],
    );
    check(
      &c,
      done(0, 499),
      40,
      expect![@r#"
        |  /w main                               |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main""#],
    );
    check(
      &c,
      done(0, 40_000),
      40,
      expect![@r#"
        |  /w main                            40s|
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Duration Right 37+3 Yellow "40s""#],
    );
    check(
      &c,
      done(2, 65_000),
      40,
      expect![@r#"
        |  /w main                         1m5s 2|
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Duration Right 34+4 Yellow "1m5s"
        Exit Right 39+1 Red "2""#],
    );
    check(
      &c,
      done(0, 7_385_000),
      40,
      expect![@r#"
        |  /w main                           2h3m|
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Duration Right 36+4 Yellow "2h3m""#],
    );
    check(
      &c,
      BlockResult::Running,
      40,
      expect![@r#"
        |  /w main                               |
        Cwd Left 2+2 Cyan "/w"
        Branch Left 5+4 Magenta "main"
        Spinner Right 38+2 Default """#],
    );

    assert_eq!(symbol_color(&BlockResult::Live), PromptColor::Green);
    assert_eq!(symbol_color(&BlockResult::Running), PromptColor::Green);
    assert_eq!(symbol_color(&done(0, 0)), PromptColor::Green);
    assert_eq!(symbol_color(&done(1, 0)), PromptColor::Red);
  }

  #[test]
  fn narrow_panes_drop_then_truncate() {
    let c = ctx_git("/home/me/code/project", branch("main"), GitStatus::default());
    // Everything fits.
    check(
      &c,
      done(1, 12_000),
      40,
      expect![@r#"
        |  ~/code/project main              12s 1|
        Cwd Left 2+14 Cyan "~/code/project"
        Branch Left 17+4 Magenta "main"
        Duration Right 35+3 Yellow "12s"
        Exit Right 39+1 Red "1""#],
    );
    // Not the duration.
    check(
      &c,
      done(1, 12_000),
      34,
      expect![@r#"
        |  ~/code/project main        12s 1|
        Cwd Left 2+14 Cyan "~/code/project"
        Branch Left 17+4 Magenta "main"
        Duration Right 29+3 Yellow "12s"
        Exit Right 33+1 Red "1""#],
    );
    // Not the exit code either: the directory is cut.
    check(
      &c,
      done(1, 12_000),
      24,
      expect![@r#"
        |  ~/code/project main  1|
        Cwd Left 2+14 Cyan "~/code/project"
        Branch Left 17+4 Magenta "main"
        Exit Right 23+1 Red "1""#],
    );
    check(
      &c,
      done(1, 12_000),
      15,
      expect![@r#"
        |  ^project main|
        Cwd Left 2+8 Cyan "^project"
        Branch Left 11+4 Magenta "main""#],
    );
    check(
      &c,
      BlockResult::Running,
      15,
      expect![@r#"
        |  ^project main|
        Cwd Left 2+8 Cyan "^project"
        Branch Left 11+4 Magenta "main""#],
    );
    // Very narrow.
    check(
      &c,
      done(1, 12_000),
      8,
      expect![@r#"
        |  ^t    |
        Cwd Left 2+2 Cyan "^t""#],
    );
    check(
      &c,
      done(1, 12_000),
      4,
      expect![@r#"
        |  ^t|
        Cwd Left 2+2 Cyan "^t""#],
    );
  }

  #[test]
  fn very_long_directory() {
    let long = format!("/home/me/{}", "deep/".repeat(30));
    let c = ctx_git(&long, branch("main"), GitStatus::default());
    check(
      &c,
      BlockResult::Live,
      50,
      expect![@r#"
        |  ^ep/deep/deep/deep/deep/deep/deep/deep/deep main|
        Cwd Left 2+43 Cyan "^ep/deep/deep/deep/deep/deep/deep/deep/deep"
        Branch Left 46+4 Magenta "main""#],
    );
    check(
      &c,
      done(0, 3000),
      50,
      expect![@r#"
        |  ^ep/deep/deep/deep/deep/deep/deep/deep/deep main|
        Cwd Left 2+43 Cyan "^ep/deep/deep/deep/deep/deep/deep/deep/deep"
        Branch Left 46+4 Magenta "main""#],
    );
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
}
