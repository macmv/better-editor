//! Benchmarks with a document-shaped item: a chunk of text summarized by its
//! byte and line counts. Run with `cargo bench -p be-sumtree`.

use std::hint::black_box;

use be_sumtree::{Bias, Dimension, Item, SumTree, Summary};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

#[derive(Clone)]
struct Chunk {
  bytes: usize,
  lines: usize,
}

#[derive(Clone, Default)]
struct Sum {
  bytes: usize,
  lines: usize,
}

impl Summary for Sum {
  fn add_summary(&mut self, o: &Self) {
    self.bytes += o.bytes;
    self.lines += o.lines;
  }
}

impl Item for Chunk {
  type Summary = Sum;

  fn summary(&self) -> Sum { Sum { bytes: self.bytes, lines: self.lines } }
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Bytes(usize);
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Lines(usize);

impl Dimension<Sum> for Bytes {
  fn add_summary(&mut self, s: &Sum) { self.0 += s.bytes }
}
impl Dimension<Sum> for Lines {
  fn add_summary(&mut self, s: &Sum) { self.0 += s.lines }
}

struct Rng(u64);

impl Rng {
  fn next(&mut self, n: usize) -> usize {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    (self.0 % n as u64) as usize
  }
}

const N: usize = 1_000_000;

fn chunk(rng: &mut Rng) -> Chunk {
  Chunk { bytes: 1000 + rng.next(3000), lines: 20 + rng.next(60) }
}

fn build(n: usize) -> SumTree<Chunk> {
  let mut rng = Rng(1);
  (0..n).map(|_| chunk(&mut rng)).collect()
}

/// A tree that has been through many random edits, so it is not freshly packed.
fn edited(n: usize, edits: usize) -> SumTree<Chunk> {
  let mut rng = Rng(7);
  let mut t = build(n);
  for _ in 0..edits {
    let total = t.dimension::<Bytes>().0;
    let at = rng.next(total);
    let (l, r) = t.split_at(&Bytes(at), Bias::Right);
    t = l;
    t.push(chunk(&mut rng));
    t.append(r);
  }
  t
}

fn benches(c: &mut Criterion) {
  let mut g = c.benchmark_group("1M items");
  g.sample_size(20);

  g.bench_function("from_iter", |b| b.iter(|| black_box(build(N))));

  g.bench_function("push", |b| {
    b.iter(|| {
      let mut rng = Rng(1);
      let mut t = SumTree::new();
      for _ in 0..N {
        t.push(chunk(&mut rng));
      }
      black_box(t)
    })
  });

  let t = build(N);
  let total_bytes = t.dimension::<Bytes>().0;
  let total_lines = t.dimension::<Lines>().0;

  g.bench_function("iterate all", |b| b.iter(|| black_box(t.iter().count())));

  g.bench_function("clone", |b| b.iter(|| black_box(t.clone())));

  g.bench_function("seek bytes (x1000)", |b| {
    let mut rng = Rng(2);
    b.iter(|| {
      let mut c = t.cursor::<Bytes>();
      for _ in 0..1000 {
        c.seek(&Bytes(rng.next(total_bytes)), Bias::Right);
        black_box(c.item());
      }
    })
  });

  g.bench_function("seek lines (x1000)", |b| {
    let mut rng = Rng(3);
    b.iter(|| {
      let mut c = t.cursor::<Lines>();
      for _ in 0..1000 {
        c.seek(&Lines(rng.next(total_lines)), Bias::Right);
        black_box(c.item());
      }
    })
  });

  // Walking forward through nearby positions: seek from the root every time vs
  // reusing the cursor.
  g.bench_function("nearby seeks, from root (x1000)", |b| {
    b.iter(|| {
      let mut c = t.cursor::<Bytes>();
      for i in 0..1000 {
        c.seek(&Bytes(i * 5000), Bias::Right);
        black_box(c.item());
      }
    })
  });
  g.bench_function("nearby seeks, seek_forward (x1000)", |b| {
    b.iter(|| {
      let mut c = t.cursor::<Bytes>();
      for i in 0..1000 {
        c.seek_forward(&Bytes(i * 5000), Bias::Right);
        black_box(c.item());
      }
    })
  });

  g.bench_function("split_at middle", |b| {
    b.iter(|| black_box(t.split_at(&Bytes(total_bytes / 2), Bias::Right)))
  });

  g.bench_function("append (two halves)", |b| {
    let (l, r) = t.split_at(&Bytes(total_bytes / 2), Bias::Right);
    b.iter_batched(|| (l.clone(), r.clone()), |(mut l, r)| l.append(r), BatchSize::SmallInput)
  });

  // The editing hot path: replace a chunk somewhere random.
  g.bench_function("edit (split, push, append)", |b| {
    let mut rng = Rng(4);
    b.iter_batched(
      || (t.clone(), rng.next(total_bytes), chunk(&mut rng)),
      |(t, at, new)| {
        let (mut l, r) = t.split_at(&Bytes(at), Bias::Right);
        l.push(new);
        l.append(r);
        black_box(l)
      },
      BatchSize::SmallInput,
    )
  });

  let e = edited(N, 20_000);
  let e_bytes = e.dimension::<Bytes>().0;
  g.bench_function("seek bytes after 20k edits (x1000)", |b| {
    let mut rng = Rng(5);
    b.iter(|| {
      let mut c = e.cursor::<Bytes>();
      for _ in 0..1000 {
        c.seek(&Bytes(rng.next(e_bytes)), Bias::Right);
        black_box(c.item());
      }
    })
  });
  g.bench_function("iterate all after 20k edits", |b| b.iter(|| black_box(e.iter().count())));
  g.finish();
}

criterion_group!(group, benches);
criterion_main!(group);
