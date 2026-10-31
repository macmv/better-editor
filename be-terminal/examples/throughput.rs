//! Headless parser throughput benchmark.
//!
//! Feeds canned pty output through `Terminal::update` and reports MB/s.
//!
//!   cargo run --release -p be-terminal --example throughput
//!
//! Pass a workload name to run only that one (`ascii`, `color`, `redraw`,
//! `unicode`, `scroll`).

use std::{
  cell::Cell,
  io,
  path::PathBuf,
  rc::Rc,
  time::{Duration, Instant},
};

use be_terminal::{PtyHost, Size, Terminal};

struct Feed {
  data: Rc<Vec<u8>>,
  pos:  Rc<Cell<usize>>,
}

impl PtyHost for Feed {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    let pos = self.pos.get();
    if pos >= self.data.len() {
      return Err(io::ErrorKind::WouldBlock.into());
    }
    let n = buf.len().min(self.data.len() - pos);
    buf[..n].copy_from_slice(&self.data[pos..pos + n]);
    self.pos.set(pos + n);
    Ok(n)
  }

  fn write_bytes(&mut self, _bytes: &[u8]) {}
  fn has_pending_writes(&self) -> bool { false }
  fn flush(&mut self) {}
  fn resize(&mut self, _size: Size) {}
  fn current_dir(&self) -> PathBuf { PathBuf::from("/") }
  fn set_wake(&mut self, _wake: Box<dyn Fn() + Send>) {}
}

const WORDS: &[&str] = &[
  "fn", "let", "mut", "terminal", "state", "cursor", "grid", "scroll", "damage", "line", "style",
  "parser", "buffer", "render", "layout", "draw", "the", "quick", "brown", "fox", "jumps",
];

struct Rng(u64);
impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
  fn word(&mut self) -> &'static str { WORDS[self.next() as usize % WORDS.len()] }
}

fn ascii(target: usize) -> Vec<u8> {
  let mut rng = Rng(1);
  let mut out = String::new();
  while out.len() < target {
    let mut line = String::new();
    while line.len() < 70 {
      line.push_str(rng.word());
      line.push(' ');
    }
    out.push_str(&line);
    out.push_str("\r\n");
  }
  out.into_bytes()
}

fn color(target: usize) -> Vec<u8> {
  let mut rng = Rng(2);
  let mut out = String::new();
  while out.len() < target {
    for _ in 0..8 {
      match rng.next() % 3 {
        0 => out.push_str(&format!("\x1b[3{}m", rng.next() % 8)),
        1 => {
          out.push_str(&format!("\x1b[1;38;2;{};{};{}m", rng.next() % 256, rng.next() % 256, 200))
        }
        _ => out.push_str("\x1b[0m"),
      }
      out.push_str(rng.word());
      out.push(' ');
    }
    out.push_str("\x1b[0m\r\n");
  }
  out.into_bytes()
}

/// Full-screen redraws, like htop or a TUI: home, then every row positioned and
/// cleared.
fn redraw(target: usize) -> Vec<u8> {
  let mut rng = Rng(3);
  let mut out = String::new();
  while out.len() < target {
    out.push_str("\x1b[H");
    for row in 1..=40 {
      out.push_str(&format!("\x1b[{row};1H\x1b[3{}m", rng.next() % 8));
      for _ in 0..8 {
        out.push_str(rng.word());
        out.push(' ');
      }
      out.push_str("\x1b[0m\x1b[K");
    }
  }
  out.into_bytes()
}

fn unicode(target: usize) -> Vec<u8> {
  let mut out = String::new();
  while out.len() < target {
    out.push_str(
      "\u{2502} caf\u{e9} na\u{ef}ve \u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500} r\u{e9}sum\u{e9} \u{2514}\u{2500} done\r\n",
    );
  }
  out.into_bytes()
}

/// Scrolls inside a region with reverse index / insert line, like an editor or
/// pager.
fn scroll(target: usize) -> Vec<u8> {
  let mut rng = Rng(4);
  let mut out = String::new();
  out.push_str("\x1b[5;35r\x1b[35;1H");
  while out.len() < target {
    out.push('\n');
    out.push_str(rng.word());
    out.push(' ');
    out.push_str(rng.word());
    out.push_str("\x1b[2;1H\x1bM\x1b[L\x1b[35;1H");
  }
  out.into_bytes()
}

fn run(name: &str, data: Vec<u8>) {
  let data = Rc::new(data);
  let pos = Rc::new(Cell::new(0));
  let size = Size { rows: 40, cols: 120 };
  let mut term = Terminal::with_host(size, Box::new(Feed { data: data.clone(), pos: pos.clone() }));

  let mut best = Duration::MAX;
  let mut worst_update = Duration::ZERO;
  for _ in 0..5 {
    pos.set(0);
    let start = Instant::now();
    while pos.get() < data.len() {
      let t = Instant::now();
      term.update();
      worst_update = worst_update.max(t.elapsed());
    }
    best = best.min(start.elapsed());
  }

  let mb = data.len() as f64 / 1e6;
  println!(
    "{name:8} {:6.1} MB  {:8.1} MB/s  {:6.2} ns/byte  longest update() {:?}",
    mb,
    mb / best.as_secs_f64(),
    best.as_secs_f64() * 1e9 / data.len() as f64,
    worst_update,
  );
}

fn main() {
  let only = std::env::args().nth(1);
  let size = 16 << 20;

  let workloads: [(&str, fn(usize) -> Vec<u8>); 5] = [
    ("ascii", ascii),
    ("color", color),
    ("redraw", redraw),
    ("unicode", unicode),
    ("scroll", scroll),
  ];
  for (name, gen_fn) in workloads {
    if only.as_deref().is_some_and(|o| o != name) {
      continue;
    }
    run(name, gen_fn(size));
  }
}
