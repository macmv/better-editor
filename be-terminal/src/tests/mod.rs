use std::{
  cell::RefCell,
  collections::VecDeque,
  fmt::Write as _,
  io,
  path::PathBuf,
  rc::Rc,
  time::{Duration, Instant},
};

use anstyle_parse::{Parser, Utf8Parser};
use expect_test::Expect;

use crate::{BuiltinColor, PtyHost, Size, Terminal, TerminalState};

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

  fn feed(&mut self, seq: Seq) { self.feed_bytes(seq.0.as_bytes()); }

  fn feed_bytes(&mut self, bytes: &[u8]) {
    crate::advance(&mut self.parser, &mut self.state, bytes);
  }

  /// Feeds every byte through the parser, without the printable ASCII fast
  /// path.
  fn feed_slow(&mut self, bytes: &[u8]) {
    for &b in bytes {
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
fn kitty_keyboard_flags() {
  let mut vt = Vt::new(2, 16);
  assert_eq!(vt.state.kitty_flags, 0);

  vt.feed_bytes(b"\x1b[>1u");
  assert_eq!(vt.state.kitty_flags, 1);

  vt.feed_bytes(b"\x1b[?u");
  assert_eq!(vt.state.pending_writes, b"\x1b[?1u");

  vt.feed_bytes(b"\x1b[>0u");
  assert_eq!(vt.state.kitty_flags, 0);

  vt.feed_bytes(b"\x1b[<u");
  assert_eq!(vt.state.kitty_flags, 1);

  vt.feed_bytes(b"\x1b[=8;2u");
  assert_eq!(vt.state.kitty_flags, 9);
  vt.feed_bytes(b"\x1b[=1;3u");
  assert_eq!(vt.state.kitty_flags, 8);

  // Popping more than was pushed resets to 0.
  vt.feed_bytes(b"\x1b[<5u");
  assert_eq!(vt.state.kitty_flags, 0);
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

#[test]
fn pending_wrap_cursor() {
  let mut vt = Vt::new(2, 5);
  vt.feed(s().print("hell"));
  vt.check(expect![@r#"
    |hell🮙|
    |     |
  "#]);
  vt.feed(s().print("o"));
  vt.check(expect![@r#"
    |hell🮙|
    |     |
  "#]);
  vt.feed(s().print("X"));
  vt.check(expect![@r#"
    |hello|
    |X🮙   |
  "#]);
}

/// Repro for nvim's cursorline not clearing on scroll in a vertical split.
///
/// nvim redraws the old cursorline row by setting the normal background and
/// emitting ECH (`CSI n X`) to blank the cells, relying on the terminal to
/// fill them with the current background (bce). We only replace the chars, so
/// the old cursorline background is left behind.
#[test]
fn erase_chars_uses_current_background() {
  let mut vt = Vt::new(2, 16);
  vt.feed(s().set_bg(BuiltinColor::Red).print("line 2          "));
  vt.feed(s().move_to(0, 0).reset_style().set_bg(BuiltinColor::Blue).print("\x1b[8X"));

  let line = vt.state.grid.line(0).unwrap();
  let bgs: Vec<_> = line.specific_styles(|_, style| style.background).collect();
  let color = |color| Some(crate::TerminalColor::Builtin { color, bright: false });
  // Cells 0..8 were erased with a blue background; the rest are untouched.
  assert_eq!(bgs, vec![(color(BuiltinColor::Blue), 8), (color(BuiltinColor::Red), 16)]);
}

fn key(vt: &Vt, row: usize) -> (u64, u64) {
  let line = vt.state.grid.line(row).unwrap();
  (line.id(), line.stamp())
}

#[test]
fn untouched_lines_keep_their_stamp() {
  let mut vt = Vt::new(3, 8);
  vt.state.begin_frame();
  vt.feed(s().print("a").crlf().print("b"));
  let (a, b, c) = (key(&vt, 0), key(&vt, 1), key(&vt, 2));

  vt.state.begin_frame();
  vt.feed(s().move_to(1, 1).print("x"));
  assert_eq!(key(&vt, 0), a);
  assert_eq!(key(&vt, 2), c);
  // Same line, new contents.
  assert_eq!(key(&vt, 1).0, b.0);
  assert_ne!(key(&vt, 1).1, b.1);
}

#[test]
fn scrolling_moves_ids_with_the_text() {
  let mut vt = Vt::new(3, 8);
  vt.state.begin_frame();
  vt.feed(s().print("a").crlf().print("b").crlf().print("c"));
  let (b, c) = (key(&vt, 1), key(&vt, 2));

  vt.state.begin_frame();
  vt.feed(s().crlf().print("d"));
  // "b" and "c" moved up a row, and weren't touched.
  assert_eq!(key(&vt, 0), b);
  assert_eq!(key(&vt, 1), c);
  // The new bottom line has a fresh id, and "a" is in scrollback.
  assert!(![b.0, c.0].contains(&key(&vt, 2).0));
  assert_eq!(vt.state.scrollback.len(), 1);
  assert_eq!(vt.state.scrollback[0].to_string().trim_end(), "a");
}

#[test]
fn scrollback_is_capped() {
  let mut vt = Vt::new(2, 4);
  for i in 0..crate::SCROLLBACK_LIMIT + 50 {
    vt.feed(s().print(&format!("{}", i % 10)).crlf());
  }
  assert_eq!(vt.state.scrollback.len(), crate::SCROLLBACK_LIMIT);
  // The first line only reaches scrollback on the second linefeed, so there are
  // LIMIT + 49 lines in total. The oldest 49 were dropped, which makes line
  // 49 (printed as "9") the oldest.
  assert_eq!(vt.state.scrollback[0].to_string().trim_end(), "9");
  assert_eq!(vt.state.scrollback[1].to_string().trim_end(), "0");
}

// --- Fast path ---

#[test]
fn ascii_fast_path_wraps() {
  let mut vt = Vt::new(3, 4);
  vt.feed(s().print("abcdefghij"));
  vt.check(expect![@r#"
    |abcd|
    |efgh|
    |ij🮙 |
  "#]);
}

#[test]
fn ascii_fast_path_stops_at_the_last_column() {
  let mut vt = Vt::new(2, 4);
  // Without autowrap, everything after the last column overwrites it.
  vt.feed(s().print("\x1b[?7l").print("abcdef").move_to(1, 0));
  vt.check(expect![@r#"
    |abcf|
    |🮙   |
  "#]);
}

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

/// Pieces of output that exercise the parser's states: text, controls, and
/// sequences that are complete, cut short, or cancelled.
const PIECES: &[&str] = &[
  "hello world",
  "x",
  "                    ",
  "\r\n",
  "\r",
  "\n",
  "\t",
  "\x08",
  "\x7f",
  "\x07",
  "\x0e",
  "\x0f",
  "\x1b(0",
  "\x1b(B",
  "lqqqk",
  "caf\u{e9}",
  "\u{2500}\u{2502}",
  "\x1b[1m",
  "\x1b[31m",
  "\x1b[38;2;1;2;3m",
  "\x1b[0m",
  "\x1b[7m",
  "\x1b[2;3H",
  "\x1b[H",
  "\x1b[K",
  "\x1b[2K",
  "\x1b[J",
  "\x1b[2J",
  "\x1b[3A",
  "\x1b[2C",
  "\x1b[@",
  "\x1b[3P",
  "\x1b[L",
  "\x1b[M",
  "\x1b[S",
  "\x1b[T",
  "\x1b[2;4r",
  "\x1b[r",
  "\x1b[4h",
  "\x1b[4l",
  "\x1b[?7l",
  "\x1b[?7h",
  "\x1b[?1049h",
  "\x1b[?1049l",
  "\x1bM",
  "\x1b7",
  "\x1b8",
  "\x1b]0;title\x07",
  "\x1b]0;title\x1b\\",
  "\x1b]0;cut short",
  "\x1bP1$rtext\x1b\\",
  "\x1b[3",
  "\x1b[",
  "\x1b",
  "\x18",
  "\x1a",
];

/// Bytes that aren't valid utf-8 on their own: an invalid byte, and the start
/// of a character.
const RAW_PIECES: &[&[u8]] = &[b"\xff", b"\xc3"];

#[test]
fn ascii_fast_path_matches_the_parser() {
  for seed in 1..200u64 {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));

    let mut bytes = vec![];
    for _ in 0..rng.below(80) + 1 {
      if rng.below(16) == 0 {
        bytes.extend_from_slice(RAW_PIECES[rng.below(RAW_PIECES.len())]);
      } else {
        bytes.extend_from_slice(PIECES[rng.below(PIECES.len())].as_bytes());
      }
    }

    let mut fast = Vt::new(4, 10);
    let mut slow = Vt::new(4, 10);

    // Split the input at random places, as reads from a pty would.
    let mut rest = &bytes[..];
    while !rest.is_empty() {
      let n = (rng.below(24) + 1).min(rest.len());
      fast.feed_bytes(&rest[..n]);
      rest = &rest[n..];
    }
    slow.feed_slow(&bytes);

    let rows =
      |vt: &Vt| (0..4).map(|row| vt.state.grid.line(row).unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(
      rows(&fast),
      rows(&slow),
      "seed {seed}, input {:?}",
      String::from_utf8_lossy(&bytes)
    );
    assert_eq!(fast.cursor(), slow.cursor(), "seed {seed}");
    assert_eq!(fast.state.cursor.pending_wrap, slow.state.cursor.pending_wrap, "seed {seed}");
    assert_eq!(fast.state.scrollback.len(), slow.state.scrollback.len(), "seed {seed}");

    for row in 0..4 {
      let styles = |vt: &Vt| {
        vt.state.grid.line(row).unwrap().specific_styles(|_, style| style).collect::<Vec<_>>()
      };
      assert_eq!(styles(&fast), styles(&slow), "seed {seed}, row {row}");
    }
  }
}

// --- Synchronized updates ---

#[derive(Clone, Default)]
struct Script {
  chunks:  Rc<RefCell<VecDeque<Vec<u8>>>>,
  written: Rc<RefCell<Vec<u8>>>,
}

impl Script {
  fn send(&self, s: &str) { self.chunks.borrow_mut().push_back(s.as_bytes().to_vec()); }
}

struct FakeHost(Script);

impl PtyHost for FakeHost {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    let Some(chunk) = self.0.chunks.borrow_mut().pop_front() else {
      return Err(io::ErrorKind::WouldBlock.into());
    };
    buf[..chunk.len()].copy_from_slice(&chunk);
    Ok(chunk.len())
  }

  fn write_bytes(&mut self, bytes: &[u8]) { self.0.written.borrow_mut().extend_from_slice(bytes); }
  fn has_pending_writes(&self) -> bool { false }
  fn flush(&mut self) {}
  fn resize(&mut self, _size: Size) {}
  fn current_dir(&self) -> PathBuf { PathBuf::from("/") }
  fn set_wake(&mut self, _wake: Box<dyn Fn() + Send>) {}
}

fn terminal() -> (Terminal, Script) {
  let script = Script::default();
  let term = Terminal::with_host(Size { rows: 3, cols: 8 }, Box::new(FakeHost(script.clone())));
  (term, script)
}

fn row(term: &Terminal, row: usize) -> String {
  term.state.grid.line(row).unwrap().to_string().trim_end().to_string()
}

const BSU: &str = "\x1b[?2026h";
const ESU: &str = "\x1b[?2026l";

#[test]
fn synchronized_update_is_shown_all_at_once() {
  let (mut term, script) = terminal();

  script.send(&format!("before{BSU}\x1b[1;1Hone"));
  term.update();
  // Nothing after the start of the update is visible yet.
  assert_eq!(row(&term, 0), "before");
  assert!(term.has_pending_output());

  script.send("\x1b[2;1Htwo");
  term.update();
  assert_eq!(row(&term, 0), "before");
  assert_eq!(row(&term, 1), "");

  script.send(&format!("\x1b[3;1Hthree{ESU}!!"));
  term.update();
  assert_eq!(row(&term, 0), "oneore");
  assert_eq!(row(&term, 1), "two");
  assert_eq!(row(&term, 2), "three!!");
  assert!(!term.has_pending_output());
}

#[test]
fn synchronized_update_end_split_across_reads() {
  let (mut term, script) = terminal();

  script.send(&format!("{BSU}hi\x1b[?20"));
  term.update();
  assert_eq!(row(&term, 0), "");

  script.send("26l");
  term.update();
  assert_eq!(row(&term, 0), "hi");
}

#[test]
fn synchronized_updates_in_one_read() {
  let (mut term, script) = terminal();

  // The second update is never finished, so it stays held.
  script.send(&format!("{BSU}a{ESU}{BSU}b"));
  term.update();
  assert_eq!(row(&term, 0), "a");
  assert!(term.has_pending_output());

  script.send(ESU);
  term.update();
  assert_eq!(row(&term, 0), "ab");
}

#[test]
fn unfinished_synchronized_update_times_out() {
  let (mut term, script) = terminal();

  script.send(&format!("{BSU}stuck"));
  term.update();
  assert_eq!(row(&term, 0), "");

  term.hold_deadline = Some(Instant::now() - Duration::from_millis(1));
  term.update();
  assert_eq!(row(&term, 0), "stuck");
  assert!(!term.has_pending_output());
  assert!(!term.state.synchronized);
}

#[test]
fn synchronized_update_mode_can_be_queried() {
  let (mut term, script) = terminal();

  script.send("\x1b[?2026$p");
  term.update();
  assert_eq!(script.written.borrow().as_slice(), b"\x1b[?2026;2$y");

  script.written.borrow_mut().clear();
  script.send(&format!("{BSU}\x1b[?2026$p"));
  term.update();
  // The query is held with the rest of the update, so it is answered when the
  // update ends.
  assert_eq!(script.written.borrow().as_slice(), b"");
  script.send(ESU);
  term.update();
  assert_eq!(script.written.borrow().as_slice(), b"\x1b[?2026;1$y");
}
