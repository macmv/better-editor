//! Random edits against a `String`, through the public API only. Unlike the
//! unit tests, this builds the crate with its real chunk size.

use be_rope::Rope;
use unicode_segmentation::UnicodeSegmentation;

struct Rng(u64);

impl Rng {
  fn next(&mut self, n: usize) -> usize {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    (self.0 % n as u64) as usize
  }
}

const ALPHABET: &[&str] = &[
  "a",
  "bc",
  " ",
  "\n",
  "\r\n",
  "\u{e9}",
  "e\u{301}",
  "\u{200d}",
  "\u{1f1e6}",
  "\u{1f1e7}",
  "\u{1f468}\u{200d}\u{1f469}",
  "\u{1f600}",
  "\u{1100}",
  "\u{1161}",
  "\u{915}\u{94d}\u{937}",
  "\u{3042}",
];

fn random_text(rng: &mut Rng, max: usize) -> String {
  (0..rng.next(max + 1)).map(|_| ALPHABET[rng.next(ALPHABET.len())]).collect()
}

fn boundary(rng: &mut Rng, s: &str) -> usize {
  let mut at = rng.next(s.len() + 1);
  while !s.is_char_boundary(at) {
    at -= 1;
  }
  at
}

fn check(rope: &Rope, m: &str, rng: &mut Rng) {
  assert_eq!(rope.to_string(), m);
  assert_eq!(rope.byte_len(), m.len());
  assert_eq!(rope.char_len(), m.chars().count());
  assert_eq!(rope.utf16_len(), m.encode_utf16().count());
  assert_eq!(rope.grapheme_len(), m.graphemes(true).count());

  let raw: Vec<&str> = m.split_inclusive('\n').collect();
  assert_eq!(rope.line_len(), raw.len());
  let mut at = 0;
  for (i, line) in raw.iter().enumerate() {
    assert_eq!(rope.byte_of_line(i), at);
    let expected =
      line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(line);
    assert_eq!(rope.line(i).to_string(), expected);
    at += line.len();
  }
  assert_eq!(rope.raw_lines().map(|l| l.to_string()).collect::<Vec<_>>(), raw);

  let starts: Vec<usize> = m.grapheme_indices(true).map(|(i, _)| i).collect();
  for _ in 0..30 {
    if !starts.is_empty() {
      let k = rng.next(starts.len());
      assert_eq!(rope.byte_of_grapheme(k), starts[k]);
      assert_eq!(rope.grapheme_of_byte(starts[k]), k);
    }
    let b = boundary(rng, m);
    assert_eq!(rope.line_of_byte(b), m[..b].matches('\n').count());
    assert_eq!(rope.utf16_of_byte(b), m[..b].encode_utf16().count());
    assert_eq!(rope.byte_of_utf16(rope.utf16_of_byte(b)), b);

    // A random slice, segmented on its own.
    let c = boundary(rng, m);
    let (a, c) = (b.min(c), b.max(c));
    let slice = rope.byte_slice(a..c);
    assert_eq!(slice.grapheme_len(), m[a..c].graphemes(true).count());
    let sub_starts: Vec<usize> = m[a..c].grapheme_indices(true).map(|(i, _)| i).collect();
    if !sub_starts.is_empty() {
      let k = rng.next(sub_starts.len());
      assert_eq!(slice.byte_of_grapheme(k), sub_starts[k]);
    }
    assert_eq!(slice.chars().rev().collect::<String>(), m[a..c].chars().rev().collect::<String>());
  }
}

#[test]
fn random_edits() {
  for seed in 1..=12u64 {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut rope = Rope::new();
    let mut model = String::new();

    for step in 0..40 {
      let a = boundary(&mut rng, &model);
      let b = boundary(&mut rng, &model);
      let (a, b) = (a.min(b), a.max(b));
      let text = random_text(&mut rng, if step % 5 == 0 { 3000 } else { 40 });
      rope.replace(a..b, &text);
      model.replace_range(a..b, &text);
      check(&rope, &model, &mut rng);
    }
  }
}

#[test]
fn big_lines_and_edits() {
  let mut rng = Rng(99);
  let mut model: String = (0..4000).map(|_| ALPHABET[rng.next(ALPHABET.len())]).collect();
  let mut rope = Rope::from(model.as_str());
  for _ in 0..30 {
    let at = boundary(&mut rng, &model);
    let text = random_text(&mut rng, 5);
    rope.insert(at, &text);
    model.insert_str(at, &text);
  }
  check(&rope, &model, &mut rng);
}
