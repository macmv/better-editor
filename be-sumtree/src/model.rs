//! A model-checking harness: applies random operations to a [`SumTree`] and a
//! plain `Vec`, and asserts they agree. Shared by the unit tests and the
//! fuzzer.

use crate::{Bias, Cursor, Dimension, Item, SumTree, Summary};

/// Items are small numbers (mod 5, so zero-width items are common). The
/// summary tracks the item count and the sum, giving two dimensions.
#[derive(Clone, Debug, PartialEq)]
pub struct Num(pub usize);

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Sum {
  pub count: usize,
  pub total: usize,
}

impl Summary for Sum {
  fn add_summary(&mut self, o: &Self) {
    self.count += o.count;
    self.total += o.total;
  }
}

impl Item for Num {
  type Summary = Sum;

  fn summary(&self) -> Sum { Sum { count: 1, total: self.0 } }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Count(pub usize);
#[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Total(pub usize);

impl Dimension<Sum> for Count {
  fn add_summary(&mut self, s: &Sum) { self.0 += s.count }
}
impl Dimension<Sum> for Total {
  fn add_summary(&mut self, s: &Sum) { self.0 += s.total }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "fuzz", derive(arbitrary::Arbitrary))]
pub enum Op {
  Push(u8),
  Append(Vec<u8>),
  /// Replace the item range `a..b` (clamped, reordered) with new items.
  Splice {
    a:   u16,
    b:   u16,
    new: Vec<u8>,
  },
  SeekTotal {
    target: u16,
    left:   bool,
  },
  SeekCount {
    target: u16,
    left:   bool,
  },
  /// Split by item count.
  Split {
    at:   u16,
    left: bool,
  },
  /// Split by sum.
  SplitTotal {
    at:   u16,
    left: bool,
  },
  /// Seek, then seek forward through a chain of non-decreasing targets. Each
  /// step must agree with a fresh seek from the root.
  SeekForward {
    target: u16,
    left:   bool,
    steps:  Vec<(u8, bool)>,
  },
  /// Advance `start` items from the first, then step forward and back.
  Walk {
    start: u16,
    steps: Vec<bool>,
  },
  /// Remember the current tree; at the end it must be unchanged.
  Snapshot,
  UpdateLast(u8),
}

fn num(b: u8) -> usize { (b % 5) as usize }

fn bias(left: bool) -> Bias { if left { Bias::Left } else { Bias::Right } }

fn tree(v: &[usize]) -> SumTree<Num> { v.iter().map(|&n| Num(n)).collect() }

fn contents(t: &SumTree<Num>) -> Vec<usize> { t.iter().map(|n| n.0).collect() }

/// The index of the first item whose end (under `width`) reaches `target`, or
/// the length if none does.
fn first_reaching(
  model: &[usize],
  target: usize,
  left: bool,
  width: impl Fn(usize) -> usize,
) -> usize {
  let mut end = 0;
  for (i, &x) in model.iter().enumerate() {
    end += width(x);
    if end > target || (left && end == target) {
      return i;
    }
  }
  model.len()
}

fn check_seek<D: Dimension<Sum> + Ord>(
  t: &SumTree<Num>,
  model: &[usize],
  target: usize,
  left: bool,
  make: impl Fn(usize) -> D,
  width: impl Fn(usize) -> usize + Copy,
) {
  let mut c = t.cursor::<D>();
  c.seek(&make(target), bias(left));
  assert_cursor(&c, model, target, left, make, width);
}

/// Asserts the cursor is where a seek to `target` should have left it.
fn assert_cursor<D: Dimension<Sum> + Ord>(
  c: &Cursor<Num, D>,
  model: &[usize],
  target: usize,
  left: bool,
  make: impl Fn(usize) -> D,
  width: impl Fn(usize) -> usize + Copy,
) {
  let i = first_reaching(model, target, left, width);
  let before: usize = model[..i].iter().map(|&x| width(x)).sum();
  assert!(c.start() == &make(before), "seek start mismatch");
  match model.get(i) {
    Some(&x) => assert_eq!(c.item(), Some(&Num(x))),
    None => assert!(c.item().is_none()),
  }
}

fn check_split<D: Dimension<Sum> + Ord>(
  t: &SumTree<Num>,
  model: &[usize],
  at: usize,
  left: bool,
  make: impl Fn(usize) -> D,
  width: impl Fn(usize) -> usize,
) {
  let (l, r) = t.split_at(&make(at), bias(left));
  l.check();
  r.check();
  let i = first_reaching(model, at, left, width);
  assert_eq!(contents(&l), model[..i]);
  assert_eq!(contents(&r), model[i..]);
}

/// Applies `ops` to a tree and a `Vec`, checking invariants and agreement after
/// every step. Panics on any mismatch.
pub fn run(ops: &[Op]) {
  let mut t = SumTree::<Num>::new();
  let mut model: Vec<usize> = vec![];
  let mut snapshots: Vec<(SumTree<Num>, Vec<usize>)> = vec![];

  for op in ops {
    match op {
      Op::Push(n) => {
        t.push(Num(num(*n)));
        model.push(num(*n));
      }
      Op::Append(items) => {
        let items: Vec<usize> = items.iter().map(|&b| num(b)).collect();
        t.append(tree(&items));
        model.extend(items);
      }
      Op::Splice { a, b, new } => {
        let a = *a as usize % (model.len() + 1);
        let b = *b as usize % (model.len() + 1);
        let (a, b) = (a.min(b), a.max(b));
        let new: Vec<usize> = new.iter().map(|&b| num(b)).collect();

        let (left, rest) = t.split_at(&Count(a), Bias::Right);
        let (_, right) = rest.split_at(&Count(b - a), Bias::Right);
        t = left;
        t.append(tree(&new));
        t.append(right);
        model.splice(a..b, new);
      }
      Op::SeekTotal { target, left } => {
        let total: usize = model.iter().sum();
        let target = *target as usize % (total + 2);
        check_seek(&t, &model, target, *left, Total, |x| x);
      }
      Op::SeekCount { target, left } => {
        let target = *target as usize % (model.len() + 2);
        check_seek(&t, &model, target, *left, Count, |_| 1);
      }
      Op::Split { at, left } => {
        let at = *at as usize % (model.len() + 2);
        check_split(&t, &model, at, *left, Count, |_| 1);
      }
      Op::SplitTotal { at, left } => {
        let total: usize = model.iter().sum();
        let at = *at as usize % (total + 2);
        check_split(&t, &model, at, *left, Total, |x| x);
      }
      Op::SeekForward { target, left, steps } => {
        let total: usize = model.iter().sum();
        let mut target = *target as usize % (total + 2);
        let mut left = *left;
        let mut c = t.cursor::<Total>();
        c.seek(&Total(target), bias(left));
        for (delta, l) in steps {
          let delta = *delta as usize % 4;
          target += delta;
          // The same target with a Right bias cannot be followed by Left.
          left = if delta == 0 && !left { false } else { *l };
          c.seek_forward(&Total(target), bias(left));
          assert_cursor(&c, &model, target, left, Total, |x| x);
        }
      }
      Op::Walk { start, steps } => {
        let mut c = t.cursor::<Total>();
        let mut i = *start as usize % (model.len() + 1);
        for _ in 0..i {
          c.advance();
        }
        for forward in steps {
          if *forward {
            c.advance();
            i = (i + 1).min(model.len());
          } else {
            let moved = c.prev();
            assert_eq!(moved, i > 0);
            i = i.saturating_sub(1);
          }
          assert_eq!(c.start().0, model[..i].iter().sum::<usize>());
          assert_eq!(c.item().map(|n| n.0), model.get(i).copied());
        }
      }
      Op::Snapshot => {
        if snapshots.len() < 8 {
          snapshots.push((t.clone(), model.clone()));
        }
      }
      Op::UpdateLast(n) => {
        if let Some(last) = model.last_mut() {
          *last = num(*n);
          t.update_last(|item| item.0 = num(*n));
        }
      }
    }

    t.check();
    assert_eq!(contents(&t), model);
    assert_eq!(t.summary(), Sum { count: model.len(), total: model.iter().sum() });
    assert_eq!(t.first().map(|n| n.0), model.first().copied());
    assert_eq!(t.last().map(|n| n.0), model.last().copied());
  }

  // Snapshots must be unaffected by everything that happened after them.
  for (snap, expected) in &snapshots {
    snap.check();
    assert_eq!(&contents(snap), expected);
  }
}
