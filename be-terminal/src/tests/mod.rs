use std::fmt::Write as _;

use anstyle_parse::{Parser, Utf8Parser};
use expect_test::Expect;

use crate::{BuiltinColor, Size, TerminalState};

// --- Sequence builder ---

struct Seq(String);

fn s() -> Seq { Seq(String::new()) }

macro_rules! helper {
  ($name:ident($($arg:ident:$ty:ty),*) = ($($expr:expr),*)) => {
    fn $name(mut self, $($arg: $ty),*) -> Self {
      write!(self.0, $($expr),*).unwrap();
      self
    }
  };
  ($name:ident($($arg:ident:$ty:ty),*) = $expr:expr) => {
    fn $name(mut self, $($arg: $ty),*) -> Self {
      write!(self.0, $expr).unwrap();
      self
    }
  };
}

#[allow(dead_code)]
impl Seq {
  fn print(mut self, s: &str) -> Self {
    self.0.push_str(s);
    self
  }

  helper!(crlf() = "\r\n");
  helper!(move_to(row: usize, col: usize) = ("\x1b[{};{}H", row + 1, col + 1));
  helper!(move_up(n: usize) = "\x1b[{n}A");
  helper!(move_down(n: usize) = "\x1b[{n}B");
  helper!(move_right(n: usize) = "\x1b[{n}C");
  helper!(move_left(n: usize) = "\x1b[{n}D");
  helper!(move_home() = "\x1b[H");
  helper!(clear_to_eol() = "\x1b[K");
  helper!(clear_screen() = "\x1b[2J");
  helper!(reset_style() = "\x1b[0m");
  helper!(bold() = "\x1b[1m");
  helper!(set_fg(color: BuiltinColor) = ("\x1b[{}m", fg_code(color)));
  helper!(set_bg(color: BuiltinColor) = ("\x1b[{}m", bg_code(color)));
}

fn fg_code(color: BuiltinColor) -> u8 {
  match color {
    BuiltinColor::Black => 30,
    BuiltinColor::Red => 31,
    BuiltinColor::Green => 32,
    BuiltinColor::Yellow => 33,
    BuiltinColor::Blue => 34,
    BuiltinColor::Magenta => 35,
    BuiltinColor::Cyan => 36,
    BuiltinColor::White => 37,
  }
}

fn bg_code(color: BuiltinColor) -> u8 {
  match color {
    BuiltinColor::Black => 40,
    BuiltinColor::Red => 41,
    BuiltinColor::Green => 42,
    BuiltinColor::Yellow => 43,
    BuiltinColor::Blue => 44,
    BuiltinColor::Magenta => 45,
    BuiltinColor::Cyan => 46,
    BuiltinColor::White => 47,
  }
}

struct Vt {
  state:  TerminalState,
  parser: Parser,
}

impl Vt {
  fn new(rows: usize, cols: usize) -> Self {
    Vt { state: TerminalState::new(Size { rows, cols }), parser: Parser::<Utf8Parser>::new() }
  }

  fn feed(&mut self, seq: Seq) {
    for b in seq.0.bytes() {
      self.parser.advance(&mut self.state, b);
    }
  }

  fn render(&self) -> String {
    let size = self.state.size;
    let cursor = self.state.cursor.pos;

    let mut out = String::new();
    for row in 0..size.rows {
      let line = self.state.grid.line(row).unwrap();
      out.push('|');
      let mut s = line.to_string();
      if row == cursor.row {
        s.replace_range(cursor.col..cursor.col + 1, "🮙");
      }
      out.push_str(&s);
      out.push_str("|\n");
    }

    out
  }

  fn check(&self, expect: Expect) { expect.assert_eq(&self.render()); }

  fn cursor(&self) -> (usize, usize) { (self.state.cursor.row, self.state.cursor.col) }
}

fn check(seq: Seq, expect: Expect) {
  let mut vt = Vt::new(2, 16);
  vt.feed(seq);
  vt.check(expect);
}

#[test]
fn plain_text() {
  check(
    s().print("hello world"),
    expect![@r#"
      |hello world🮙    |
      |                |
    "#],
  );
}

#[test]
fn newline() {
  check(
    s().print("hello").crlf().print("world"),
    expect![@r#"
      |hello           |
      |world🮙          |
    "#],
  );
}

#[test]
fn cursor_home() {
  let mut vt = Vt::new(2, 16);
  vt.feed(s().move_to(1, 9));
  assert_eq!(vt.cursor(), (1, 9));
  vt.feed(s().move_left(1));
  assert_eq!(vt.cursor(), (1, 8));
  vt.feed(s().move_up(1));
  assert_eq!(vt.cursor(), (0, 8));
  vt.feed(s().move_down(1));
  assert_eq!(vt.cursor(), (1, 8));
  vt.feed(s().move_home());
  assert_eq!(vt.cursor(), (0, 0));
}

#[test]
fn text_at_position() {
  check(
    s().move_to(2, 9).print("hello"),
    expect![@r#"
      |                |
      |         hello🮙 |
    "#],
  );
}

#[test]
fn clear_to_eol() {
  check(
    s().print("hello").move_left(3).clear_to_eol(),
    expect![@r#"
      |he🮙             |
      |                |
    "#],
  );
}

#[test]
fn sgr_no_effect_on_content() {
  check(
    s().set_fg(BuiltinColor::Red).print("hello").reset_style().print(" world"),
    expect![@r#"
      |hello world🮙    |
      |                |
    "#],
  );
}

#[test]
fn line_wrap() {
  let mut vt = Vt::new(3, 5);
  vt.feed(s().print("helloX"));
  vt.check(expect![@r#"
    |hello|
    |X🮙   |
    |     |
  "#]);
}
