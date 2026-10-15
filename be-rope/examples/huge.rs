//! Times the operations that matter on a huge single line.
use std::time::Instant;

use be_rope::RopeBuilder;

fn main() {
  let mb: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100);
  let piece = "hello w\u{f6}rld \u{1f600} e\u{301} \u{1f468}\u{200d}\u{1f469} ";
  let reps = mb * 1024 * 1024 / piece.len();

  let t = Instant::now();
  let mut builder = RopeBuilder::new();
  for _ in 0..reps {
    builder.append(piece);
  }
  builder.append("\nsecond line\n");
  let mut rope = builder.build();
  println!("build {} MB: {:?}", rope.byte_len() / 1024 / 1024, t.elapsed());

  let t = Instant::now();
  let n = rope.line(0).grapheme_len();
  println!("grapheme_len of the 1-line file: {n} in {:?}", t.elapsed());

  let t = Instant::now();
  let mut sum = 0;
  for k in (0..n).step_by(n / 1000) {
    let b = rope.line(0).byte_of_grapheme(k);
    sum += rope.line(0).grapheme_of_byte(b) - k;
  }
  println!("1000 x (byte_of_grapheme + grapheme_of_byte): {:?} ({sum})", t.elapsed());

  let t = Instant::now();
  for i in 0..1000 {
    let at = rope.line(0).byte_of_grapheme(n / 2 + i);
    rope.insert(at, "x");
  }
  println!("1000 inserts in the middle: {:?}", t.elapsed());

  let t = Instant::now();
  let snapshot = rope.clone();
  rope.insert(0, "y");
  println!("clone + insert: {:?} (snapshot len {})", t.elapsed(), snapshot.byte_len());

  let t = Instant::now();
  let total: usize = rope.line(1).chars().count() + rope.lines().count();
  println!("line(1) + lines(): {total} in {:?}", t.elapsed());

  let t = Instant::now();
  let last = rope.graphemes().next_back().unwrap().to_string();
  println!("last grapheme {last:?}: {:?}", t.elapsed());
}
