use unicode_segmentation::UnicodeSegmentation;

use crate::{
  Rope, RopeBuilder, RopeSlice,
  rope::{MAX_CHUNK, MIN_CHUNK},
};

/// A deterministic pseudo-random generator.
struct Rng(u64);

impl Rng {
  fn next(&mut self, n: usize) -> usize {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    (self.0 % n as u64) as usize
  }
}

/// Characters that exercise lines, graphemes, and UTF-16.
const ALPHABET: &[&str] = &[
  "a",
  "b",
  "c",
  " ",
  "\n",
  "\n",
  "\r\n",
  "\r",
  "\t",
  "\u{e9}",
  "e\u{301}",  // e + combining acute
  "\u{200d}",  // ZWJ
  "\u{1f1e6}", // regional indicators
  "\u{1f1e7}",
  "\u{1f468}\u{200d}\u{1f469}", // man ZWJ woman
  "\u{1f600}",                  // outside the basic plane
  "\u{1f3fb}",                  // emoji modifier
  "\u{1100}",                   // hangul L
  "\u{1161}",                   // hangul V
  "\u{11a8}",                   // hangul T
  "\u{600}",                    // prepend
  "\u{915}\u{94d}",             // devanagari consonant + virama
  "\u{937}",
  "\u{3042}",
];

fn random_text(rng: &mut Rng, max: usize) -> String {
  let n = rng.next(max + 1);
  (0..n).map(|_| ALPHABET[rng.next(ALPHABET.len())]).collect()
}

/// A random character boundary of `s`.
fn random_boundary(rng: &mut Rng, s: &str) -> usize {
  let mut at = rng.next(s.len() + 1);
  while !s.is_char_boundary(at) {
    at -= 1;
  }
  at
}

fn strip_line_break(line: &str) -> &str {
  let line = line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(line);
  line
}

/// Asserts every query on `slice` agrees with the same query on `m`, which is
/// the same text.
#[track_caller]
fn check_slice(slice: RopeSlice<'_>, m: &str) {
  assert_eq!(slice.to_string(), m);
  assert!(slice == m);
  assert_eq!(slice.byte_len(), m.len());
  assert_eq!(slice.is_empty(), m.is_empty());
  assert_eq!(slice.char_len(), m.chars().count());
  assert_eq!(slice.utf16_len(), m.encode_utf16().count());

  // Iterators, forward and backward.
  assert_eq!(slice.chars().collect::<String>(), m);
  assert_eq!(slice.chars().rev().collect::<String>(), m.chars().rev().collect::<String>());
  assert_eq!(slice.bytes().collect::<Vec<_>>(), m.as_bytes());
  assert_eq!(slice.bytes().rev().collect::<Vec<_>>(), m.bytes().rev().collect::<Vec<_>>());
  assert_eq!(slice.chunks().collect::<String>(), m);
  assert_eq!(slice.chunks().rev().collect::<Vec<_>>().concat(), {
    let mut chunks: Vec<&str> = slice.chunks().collect();
    chunks.reverse();
    chunks.concat()
  });
  assert!(slice.chunks().all(|c| !c.is_empty() && c.len() <= MAX_CHUNK));

  // Char boundaries.
  for i in 0..=m.len() + 1 {
    assert_eq!(slice.is_char_boundary(i), m.is_char_boundary(i), "boundary {i} of {m:?}");
  }

  // Lines.
  let raw: Vec<&str> = m.split_inclusive('\n').collect();
  assert_eq!(slice.line_len(), raw.len(), "{m:?}");
  assert_eq!(slice.raw_lines().map(|l| l.to_string()).collect::<Vec<_>>(), raw);
  assert_eq!(slice.raw_lines().rev().map(|l| l.to_string()).collect::<Vec<_>>(), {
    let mut r = raw.clone();
    r.reverse();
    r
  });
  let stripped: Vec<&str> = raw.iter().map(|l| strip_line_break(l)).collect();
  assert_eq!(slice.lines().map(|l| l.to_string()).collect::<Vec<_>>(), stripped);
  assert_eq!(slice.lines().rev().map(|l| l.to_string()).collect::<Vec<_>>(), {
    let mut r = stripped.clone();
    r.reverse();
    r
  });

  let mut at = 0;
  for (i, line) in raw.iter().enumerate() {
    assert_eq!(slice.byte_of_line(i), at, "line {i} of {m:?}");
    assert_eq!(slice.line(i), stripped[i]);
    at += line.len();
  }
  assert_eq!(slice.byte_of_line(raw.len()), m.len());
  for start in 0..=raw.len() {
    for end in start..=raw.len() {
      assert_eq!(slice.line_slice(start..end), raw[start..end].concat().as_str());
    }
  }

  // Offsets.
  let starts: Vec<usize> = m.grapheme_indices(true).map(|(i, _)| i).collect();
  assert_eq!(slice.grapheme_len(), starts.len(), "{m:?}");
  for (k, &start) in starts.iter().enumerate() {
    assert_eq!(slice.byte_of_grapheme(k), start, "grapheme {k} of {m:?}");
  }
  assert_eq!(slice.byte_of_grapheme(starts.len()), m.len());

  let mut by_utf16 = vec![];
  for (i, c) in m.char_indices() {
    for _ in 0..c.len_utf16() {
      by_utf16.push(i);
    }
  }
  by_utf16.push(m.len());
  for (u, &byte) in by_utf16.iter().enumerate() {
    assert_eq!(slice.byte_of_utf16(u), byte, "utf16 {u} of {m:?}");
  }

  for b in (0..=m.len()).filter(|&b| m.is_char_boundary(b)) {
    let before = &m[..b];
    assert_eq!(slice.line_of_byte(b), before.matches('\n').count(), "byte {b} of {m:?}");
    assert_eq!(slice.utf16_of_byte(b), before.encode_utf16().count());
    assert_eq!(slice.grapheme_of_byte(b), before.graphemes(true).count(), "byte {b} of {m:?}");
  }

  // Graphemes, forward and backward, and mixed.
  let clusters: Vec<&str> = m.graphemes(true).collect();
  assert_eq!(slice.graphemes().map(|g| g.to_string()).collect::<Vec<_>>(), clusters);
  assert_eq!(slice.graphemes().rev().map(|g| g.to_string()).collect::<Vec<_>>(), {
    let mut r = clusters.clone();
    r.reverse();
    r
  });
  let mut iter = slice.graphemes();
  let (mut lo, mut hi) = (0, clusters.len());
  for step in 0..clusters.len() + 1 {
    if step % 3 == 1 {
      assert_eq!(
        iter.next_back().map(|g| g.to_string()),
        hi.checked_sub(1).filter(|_| hi > lo).map(|h| clusters[h].to_string())
      );
      if hi > lo {
        hi -= 1;
      }
    } else {
      assert_eq!(iter.next().map(|g| g.to_string()), (lo < hi).then(|| clusters[lo].to_string()));
      if lo < hi {
        lo += 1;
      }
    }
  }
}

#[track_caller]
fn check_rope(rope: &Rope, m: &str) {
  rope.tree.check();
  assert_eq!(rope.byte_len(), m.len());
  assert_eq!(rope.is_empty(), m.is_empty());
  assert!(*rope == *m);
  check_slice(rope.as_slice(), m);

  // Every chunk is within bounds, and chunks do not pile up small.
  let sizes: Vec<usize> = rope.chunks().map(|c| c.len()).collect();
  if sizes.len() > 1 {
    // Small chunks can only come from rounding to character boundaries, or
    // from the last chunk of a build.
    let small = sizes.iter().filter(|&&s| s < MIN_CHUNK - 3).count();
    assert!(small <= 1, "{small} small chunks in {sizes:?}");
  }
}

#[test]
fn empty() {
  let rope = Rope::new();
  check_rope(&rope, "");
  assert_eq!(rope.line_len(), 0);
  assert_eq!(rope.byte_of_line(0), 0);
  assert_eq!(rope.line_of_byte(0), 0);
  assert_eq!(rope.grapheme_len(), 0);
}

#[test]
fn doc_examples_match_crop() {
  let r = Rope::from("\u{192}oo\nb\u{e4}r\r\nbaz");
  assert_eq!(r.byte_of_line(0), 0);
  assert_eq!(r.byte_of_line(1), "\u{192}oo\n".len());
  assert_eq!(r.byte_of_line(2), "\u{192}oo\nb\u{e4}r\r\n".len());
  assert_eq!(r.byte_of_line(r.line_len()), r.byte_len());

  let r = Rope::from("foo\nbar\r\nbaz");
  assert_eq!(r.line(0), "foo");
  assert_eq!(r.line(1), "bar");
  assert_eq!(r.line(2), "baz");
  assert_eq!(r.line_of_byte(0), 0);
  assert_eq!(r.line_of_byte(3), 0);
  assert_eq!(r.line_of_byte(4), 1);
  assert_eq!(r.line_of_byte(8), 1); // between the '\r' and the '\n'
  assert_eq!(r.line_of_byte(r.byte_len()), 2);

  let mut r = Rope::new();
  assert_eq!(r.line_len(), 0);
  r.insert(0, "a");
  assert_eq!(r.line_len(), 1);
  r.insert(1, "\n");
  assert_eq!(r.line_len(), 1);
  r.insert(2, "b");
  assert_eq!(r.line_len(), 2);
  r.insert(3, "\r\n");
  assert_eq!(r.line_len(), 2);

  let r = Rope::from("foo\nbar\r\nbaz\nfoobar\n");
  assert_eq!(r.line_slice(..1), "foo\n");
  assert_eq!(r.line_slice(1..3), "bar\r\nbaz\n");
  assert_eq!(r.line_slice(3..4), "foobar\n");
  assert_eq!(r.line_slice(4..), "");
}

#[test]
#[should_panic]
fn line_of_byte_off_a_boundary_panics() { Rope::from("\u{1f496}a").line_of_byte(1); }

#[test]
#[should_panic]
fn replace_out_of_bounds_panics() { Rope::from("abc").replace(2..4, ""); }

#[test]
fn builder_makes_the_same_rope() {
  let mut rng = Rng(0x1234_5678_9abc_def1);
  for _ in 0..300 {
    let text = random_text(&mut rng, 200);
    let mut builder = RopeBuilder::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
      let cut = random_boundary(&mut rng, rest).max(1);
      let cut = (cut..=rest.len()).find(|&i| rest.is_char_boundary(i)).unwrap();
      builder.append(&rest[..cut]);
      rest = &rest[cut..];
    }
    check_rope(&builder.build(), &text);
    check_rope(&Rope::from(text.as_str()), &text);
  }
}

#[test]
fn random_edits_match_a_string() {
  for seed in 1..=40u64 {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut rope = Rope::new();
    let mut model = String::new();
    let mut snapshots: Vec<(Rope, String)> = vec![];

    for step in 0..60 {
      let a = random_boundary(&mut rng, &model);
      let b = random_boundary(&mut rng, &model);
      let (a, b) = (a.min(b), a.max(b));
      let max = if rng.next(8) == 0 { 120 } else { 12 };
      let text = random_text(&mut rng, max);

      rope.replace(a..b, &text);
      model.replace_range(a..b, &text);
      check_rope(&rope, &model);

      if step % 7 == 0 {
        snapshots.push((rope.clone(), model.clone()));
      }

      // Check a few random sub-slices.
      for _ in 0..3 {
        let a = random_boundary(&mut rng, &model);
        let b = random_boundary(&mut rng, &model);
        let (a, b) = (a.min(b), a.max(b));
        check_slice(rope.byte_slice(a..b), &model[a..b]);
      }
    }

    // Snapshots are not changed by later edits.
    for (snapshot, text) in snapshots {
      check_rope(&snapshot, &text);
    }
  }
}

#[test]
fn typing_does_not_fragment_chunks() {
  let mut rope = Rope::new();
  let mut model = String::new();
  let mut rng = Rng(77);
  for _ in 0..2000 {
    let at = random_boundary(&mut rng, &model);
    let c = ["a", "b", "\n", "\u{e9}"][rng.next(4)];
    rope.insert(at, c);
    model.insert_str(at, c);
  }
  check_rope(&rope, &model);

  // Chunks should be about half full or better.
  let chunks = rope.chunks().count();
  assert!(
    chunks * MIN_CHUNK <= model.len() + 2 * MAX_CHUNK,
    "{chunks} chunks for {} bytes",
    model.len()
  );

  // And so should deleting.
  while model.len() > 20 {
    let a = random_boundary(&mut rng, &model);
    let b = (a + rng.next(5)).min(model.len());
    let b = (b..=model.len()).find(|&i| model.is_char_boundary(i)).unwrap();
    rope.delete(a..b);
    model.replace_range(a..b, "");
  }
  check_rope(&rope, &model);
}

#[test]
fn clones_are_independent() {
  let a = Rope::from("hello world");
  let mut b = a.clone();
  b.replace(0..5, "goodbye");
  assert_eq!(a, "hello world");
  assert_eq!(b, "goodbye world");
}

#[test]
fn slices_segment_graphemes_on_their_own() {
  // Cutting a slice in the middle of a cluster starts a new one.
  let rope = Rope::from("e\u{301}x");
  let slice = rope.byte_slice(1..);
  assert_eq!(slice.grapheme_len(), 2);
  assert_eq!(slice.byte_of_grapheme(1), 2);

  // Regional indicators pair up from the start of the slice.
  let rope = Rope::from("\u{1f1e6}\u{1f1e7}\u{1f1e8}\u{1f1e9}");
  assert_eq!(rope.grapheme_len(), 2);
  let slice = rope.byte_slice(4..);
  assert_eq!(slice.grapheme_len(), 2);
  assert_eq!(rope.byte_slice(4..12).grapheme_len(), 1);
}

#[test]
fn queries_on_a_huge_line_are_fast() {
  // A million characters on one line.
  let mut builder = RopeBuilder::new();
  let piece = "ab\u{e9}\u{1f600}e\u{301}";
  for _ in 0..170_000 {
    builder.append(piece);
  }
  builder.append("\nsecond line");
  let rope = builder.build();

  let graphemes = 170_000 * 5;
  assert_eq!(rope.line_len(), 2);
  assert_eq!(rope.line(0).grapheme_len(), graphemes);
  assert_eq!(rope.line(1), "second line");

  // These must not scan the line.
  let start = std::time::Instant::now();
  for k in (0..graphemes).step_by(997) {
    let byte = rope.line(0).byte_of_grapheme(k);
    assert_eq!(rope.line(0).grapheme_of_byte(byte), k);
  }
  let elapsed = start.elapsed();
  assert!(elapsed.as_secs_f64() < 5.0, "took {elapsed:?}");
}

/// Text and patterns from a tiny alphabet, so matches are dense and overlap.
const SEARCH_ALPHABET: &[&str] = &["a", "a", "b", "\u{e9}", "\u{1f600}"];

fn random_search_text(rng: &mut Rng, max: usize) -> String {
  let n = rng.next(max + 1);
  (0..n).map(|_| SEARCH_ALPHABET[rng.next(SEARCH_ALPHABET.len())]).collect()
}

#[track_caller]
fn check_search(slice: RopeSlice<'_>, m: &str, pattern: &str) {
  let want: Vec<usize> =
    if pattern.is_empty() { vec![] } else { m.match_indices(pattern).map(|(i, _)| i).collect() };
  assert_eq!(slice.find(pattern).collect::<Vec<_>>(), want, "find {pattern:?} in {m:?}");

  let want: Vec<usize> =
    if pattern.is_empty() { vec![] } else { m.rmatch_indices(pattern).map(|(i, _)| i).collect() };
  assert_eq!(slice.rfind(pattern).collect::<Vec<_>>(), want, "rfind {pattern:?} in {m:?}");
}

#[test]
fn search_basics() {
  let r = Rope::from("foo bar baz ooo quoox");
  assert_eq!(r.find("oo").collect::<Vec<_>>(), [1, 12, 18]);
  assert_eq!(r.rfind("oo").collect::<Vec<_>>(), [18, 13, 1]);
  assert_eq!(r.find("").count(), 0);
  assert_eq!(r.rfind("").count(), 0);
  assert_eq!(r.find("missing").count(), 0);
  assert_eq!(Rope::new().find("a").count(), 0);
  assert_eq!(Rope::new().rfind("a").count(), 0);

  // Offsets are relative to the slice.
  assert_eq!(r.byte_slice(4..).find("ba").collect::<Vec<_>>(), [0, 4]);
  assert_eq!(r.byte_slice(..9).rfind("ba").collect::<Vec<_>>(), [4]);
  assert_eq!(r.byte_slice(..13).rfind("ba").collect::<Vec<_>>(), [8, 4]);
}

#[test]
fn search_across_chunk_boundaries() {
  // Every chunk is a few bytes, and the pattern is longer than a chunk.
  let text = "ab".repeat(100);
  let r = Rope::from(text.as_str());
  assert!(r.chunks().count() > 10);
  for len in 1..=3 * MAX_CHUNK {
    let pattern = &text[1..1 + len];
    check_search(r.as_slice(), &text, pattern);
  }
}

#[test]
fn search_matches_str() {
  let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
  for _ in 0..2000 {
    let text = random_search_text(&mut rng, 300);
    let mut rope = Rope::from(text.as_str());
    let mut m = text;

    // Edit it, so the chunks are not evenly sized.
    for _ in 0..rng.next(4) {
      let a = random_boundary(&mut rng, &m);
      let b = a + random_boundary(&mut rng, &m[a..]);
      let ins = random_search_text(&mut rng, 20);
      rope.replace(a..b, &ins);
      m.replace_range(a..b, &ins);
    }

    let lo = random_boundary(&mut rng, &m);
    let hi = lo + random_boundary(&mut rng, &m[lo..]);
    let slice = rope.byte_slice(lo..hi);

    // A pattern cut from the text, so it matches, or a random one.
    let pattern = if rng.next(3) > 0 && !m.is_empty() {
      let a = random_boundary(&mut rng, &m);
      let b = (a + rng.next(3 * MAX_CHUNK)).min(m.len());
      let b = if m.is_char_boundary(b) { b } else { m.ceil_char_boundary(b) };
      m[a..b].to_string()
    } else {
      random_search_text(&mut rng, 6)
    };

    check_search(rope.as_slice(), &m, &pattern);
    check_search(slice, &m[lo..hi], &pattern);
  }
}
