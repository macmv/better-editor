//! Times building a rope from text of different kinds.
use std::time::Instant;

fn main() {
  let mb: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(256);
  for (name, piece) in [
    ("ascii", "hello world, this is some ascii text.\n"),
    ("mixed", "hello w\u{f6}rld \u{1f600} e\u{301} \u{1f468}\u{200d}\u{1f469} "),
    ("cjk", "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{4e16}\u{754c}"),
  ] {
    let reps = mb * 1024 * 1024 / piece.len();
    let t = Instant::now();
    let mut b = be_rope::RopeBuilder::new();
    for _ in 0..reps {
      b.append(piece);
    }
    let rope = b.build();
    let e = t.elapsed();
    println!(
      "{name:6} {} MB in {e:?} = {:.0} MB/s",
      rope.byte_len() >> 20,
      rope.byte_len() as f64 / 1048576.0 / e.as_secs_f64()
    );
  }
}
