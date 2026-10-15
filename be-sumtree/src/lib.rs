//! A persistent summary tree (a B-tree whose nodes cache a monoidal summary of
//! their subtree).
//!
//! Every item produces a [`Summary`]; summaries combine associatively. Any
//! number of [`Dimension`]s can be derived from a summary (bytes, lines,
//! UTF-16 units, ...), and any monotonic dimension can be seeked in O(log n)
//! with a [`Cursor`].
//!
//! Nodes are shared through `Arc`, so cloning a tree is O(1) and snapshots are
//! immutable and thread safe. Mutation copies only the path it touches.
//!
//! The tree is generic over its item type. An item is the unit of splitting:
//! [`SumTree::split_at`] never cuts inside an item, so a rope built on this
//! would use leaf items such as `(buffer, range)` slices and split those
//! itself.

use std::{cmp::Ordering, sync::Arc};

#[cfg(not(test))]
const MAX: usize = 12;
// A tiny fan-out makes the tests exercise deep trees.
#[cfg(test)]
const MAX: usize = 4;

/// A monoid element. `Default` is the identity.
pub trait Summary: Clone + Default {
  fn add_summary(&mut self, other: &Self);
}

impl Summary for () {
  fn add_summary(&mut self, _: &Self) {}
}

/// A value stored in a [`SumTree`].
pub trait Item: Clone {
  type Summary: Summary;

  fn summary(&self) -> Self::Summary;
}

/// A coordinate that is accumulated from summaries. For seeking to work, the
/// dimension must be monotonic: adding a summary never moves it backwards.
pub trait Dimension<S: Summary>: Clone + Default {
  fn add_summary(&mut self, summary: &S);
}

/// Every summary is a dimension of itself.
impl<S: Summary> Dimension<S> for S {
  fn add_summary(&mut self, summary: &S) { Summary::add_summary(self, summary) }
}

/// Something a cursor can seek to, compared against a dimension value.
pub trait SeekTarget<D> {
  /// Compares the target against a location. `Less` means the target is before
  /// the location.
  fn seek_cmp(&self, location: &D) -> Ordering;
}

impl<D: Ord> SeekTarget<D> for D {
  fn seek_cmp(&self, location: &D) -> Ordering { Ord::cmp(self, location) }
}

/// Decides what happens when the target lands exactly on an item boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
  /// Stop at the first item whose end is `>= target`. Items ending exactly at
  /// the target are not skipped.
  Left,
  /// Stop at the first item whose end is `> target`. Items ending exactly at
  /// the target are skipped (they are "before" the target).
  Right,
}

fn reaches<D, Tg: SeekTarget<D>>(target: &Tg, end: &D, bias: Bias) -> bool {
  match target.seek_cmp(end) {
    Ordering::Less => true,
    Ordering::Equal => bias == Bias::Left,
    Ordering::Greater => false,
  }
}

enum Node<T: Item> {
  Internal { height: u8, summary: T::Summary, children: Vec<Arc<Node<T>>> },
  Leaf { summary: T::Summary, items: Vec<T>, summaries: Vec<T::Summary> },
}

impl<T: Item> Clone for Node<T> {
  fn clone(&self) -> Self {
    match self {
      Node::Internal { height, summary, children } => {
        Node::Internal { height: *height, summary: summary.clone(), children: children.clone() }
      }
      Node::Leaf { summary, items, summaries } => Node::Leaf {
        summary:   summary.clone(),
        items:     items.clone(),
        summaries: summaries.clone(),
      },
    }
  }
}

fn sum_of<'a, S: Summary + 'a>(summaries: impl Iterator<Item = &'a S>) -> S {
  let mut total = S::default();
  for s in summaries {
    total.add_summary(s);
  }
  total
}

impl<T: Item> Node<T> {
  fn leaf(items: Vec<T>) -> Node<T> {
    let summaries: Vec<_> = items.iter().map(T::summary).collect();
    Node::Leaf { summary: sum_of(summaries.iter()), items, summaries }
  }

  fn internal(height: u8, children: Vec<Arc<Node<T>>>) -> Node<T> {
    Node::Internal { height, summary: sum_of(children.iter().map(|c| c.summary())), children }
  }

  fn height(&self) -> u8 {
    match self {
      Node::Internal { height, .. } => *height,
      Node::Leaf { .. } => 0,
    }
  }

  fn summary(&self) -> &T::Summary {
    match self {
      Node::Internal { summary, .. } | Node::Leaf { summary, .. } => summary,
    }
  }

  fn update_summary(&mut self) {
    match self {
      Node::Internal { summary, children, .. } => {
        *summary = sum_of(children.iter().map(|c| c.summary()))
      }
      Node::Leaf { summary, summaries, .. } => *summary = sum_of(summaries.iter()),
    }
  }

  /// Recomputes the summary, and splits the node in half if it has too many
  /// entries. Returns the new right half, if any.
  fn finish(&mut self) -> Option<Node<T>> {
    let right = match self {
      Node::Leaf { items, summaries, .. } if items.len() > MAX => {
        let mid = items.len() / 2;
        Some(Node::Leaf {
          summary:   Default::default(),
          items:     items.split_off(mid),
          summaries: summaries.split_off(mid),
        })
      }
      Node::Internal { height, children, .. } if children.len() > MAX => {
        let mid = children.len() / 2;
        Some(Node::Internal {
          height:   *height,
          summary:  Default::default(),
          children: children.split_off(mid),
        })
      }
      _ => None,
    };
    self.update_summary();
    right.map(|mut r| {
      r.update_summary();
      r
    })
  }

  /// Appends an item at the rightmost leaf. Returns an overflow sibling.
  fn push_item(&mut self, item: T) -> Option<Node<T>> {
    match self {
      Node::Leaf { items, summaries, .. } => {
        summaries.push(item.summary());
        items.push(item);
      }
      Node::Internal { children, .. } => {
        let last = children.len() - 1;
        if let Some(sibling) = Arc::make_mut(&mut children[last]).push_item(item) {
          children.push(Arc::new(sibling));
        }
      }
    }
    self.finish()
  }

  fn update_last(&mut self, f: impl FnOnce(&mut T)) {
    match self {
      Node::Leaf { items, summaries, .. } => {
        let last = items.len() - 1;
        f(&mut items[last]);
        summaries[last] = items[last].summary();
      }
      Node::Internal { children, .. } => {
        let last = children.len() - 1;
        Arc::make_mut(&mut children[last]).update_last(f);
      }
    }
    self.update_summary();
  }

  /// Attaches `other` (which must be shorter than `self`) to the left or right
  /// edge of this node, at the matching depth. Returns an overflow sibling.
  fn insert_edge(&mut self, other: Arc<Node<T>>, front: bool) -> Option<Node<T>> {
    let Node::Internal { height, children, .. } = self else {
      unreachable!("insert_edge on a leaf")
    };

    if *height == other.height() + 1 {
      if front {
        children.insert(0, other);
      } else {
        children.push(other);
      }
    } else {
      let i = if front { 0 } else { children.len() - 1 };
      if let Some(sibling) = Arc::make_mut(&mut children[i]).insert_edge(other, front) {
        children.insert(i + 1, Arc::new(sibling));
      }
    }
    self.finish()
  }

  #[cfg(test)]
  fn check(&self) -> (u8, T::Summary)
  where
    T::Summary: PartialEq + std::fmt::Debug,
  {
    match self {
      Node::Leaf { summary, items, summaries } => {
        assert!(!items.is_empty() && items.len() <= MAX);
        assert_eq!(items.len(), summaries.len());
        let expected = sum_of(items.iter().map(T::summary).collect::<Vec<_>>().iter());
        assert_eq!(*summary, expected);
        (0, expected)
      }
      Node::Internal { height, summary, children } => {
        assert!(!children.is_empty() && children.len() <= MAX);
        let mut total = T::Summary::default();
        for c in children {
          let (h, s) = c.check();
          assert_eq!(h + 1, *height);
          Summary::add_summary(&mut total, &s);
        }
        assert_eq!(*summary, total);
        (*height, total)
      }
    }
  }
}

/// A persistent sequence of `T`, indexed by summary dimensions.
pub struct SumTree<T: Item> {
  root: Option<Arc<Node<T>>>,
}

impl<T: Item> Clone for SumTree<T> {
  fn clone(&self) -> Self { SumTree { root: self.root.clone() } }
}

impl<T: Item> Default for SumTree<T> {
  fn default() -> Self { SumTree::new() }
}

impl<T: Item> SumTree<T> {
  pub fn new() -> Self { SumTree { root: None } }

  pub fn is_empty(&self) -> bool { self.root.is_none() }

  /// The summary of the whole tree.
  pub fn summary(&self) -> T::Summary {
    match &self.root {
      Some(root) => root.summary().clone(),
      None => T::Summary::default(),
    }
  }

  /// The total of a single dimension.
  pub fn dimension<D: Dimension<T::Summary>>(&self) -> D {
    let mut d = D::default();
    if let Some(root) = &self.root {
      d.add_summary(root.summary());
    }
    d
  }

  pub fn push(&mut self, item: T) {
    match &mut self.root {
      None => self.root = Some(Arc::new(Node::leaf(vec![item]))),
      Some(root) => {
        if let Some(sibling) = Arc::make_mut(root).push_item(item) {
          let height = root.height() + 1;
          let left = self.root.take().unwrap();
          self.root = Some(Arc::new(Node::internal(height, vec![left, Arc::new(sibling)])));
        }
      }
    }
  }

  /// Concatenates `other` onto the end of this tree in O(log n).
  pub fn append(&mut self, other: SumTree<T>) { *self = concat(std::mem::take(self), other); }

  pub fn first(&self) -> Option<&T> { self.iter().next() }

  pub fn last(&self) -> Option<&T> {
    let mut node = &**self.root.as_ref()?;
    loop {
      match node {
        Node::Internal { children, .. } => node = children.last().unwrap(),
        Node::Leaf { items, .. } => return items.last(),
      }
    }
  }

  pub fn iter(&self) -> Iter<'_, T> { Iter { cursor: self.cursor() } }

  pub fn cursor<D: Dimension<T::Summary>>(&self) -> Cursor<'_, T, D> {
    let mut cursor = Cursor { tree: self, stack: Vec::new(), start: D::default() };
    cursor.seek_first();
    cursor
  }

  /// Splits the tree at an item boundary. With [`Bias::Right`], every item
  /// that ends at or before `target` goes left; with [`Bias::Left`], only
  /// items that end strictly before it do.
  pub fn split_at<D, Tg>(&self, target: &Tg, bias: Bias) -> (SumTree<T>, SumTree<T>)
  where
    D: Dimension<T::Summary>,
    Tg: SeekTarget<D>,
  {
    match &self.root {
      None => (SumTree::new(), SumTree::new()),
      Some(root) => split_node(root, target, bias, &mut D::default()),
    }
  }

  /// Mutates the last item in place (copying only the path to it).
  pub fn update_last(&mut self, f: impl FnOnce(&mut T)) {
    if let Some(root) = &mut self.root {
      Arc::make_mut(root).update_last(f);
    }
  }

  #[cfg(test)]
  fn check(&self)
  where
    T::Summary: PartialEq + std::fmt::Debug,
  {
    if let Some(root) = &self.root {
      root.check();
    }
  }
}

fn tree_of<T: Item>(node: Arc<Node<T>>) -> SumTree<T> { SumTree { root: Some(node) } }

fn leaf_tree<T: Item>(items: &[T]) -> SumTree<T> {
  if items.is_empty() { SumTree::new() } else { tree_of(Arc::new(Node::leaf(items.to_vec()))) }
}

fn concat<T: Item>(a: SumTree<T>, b: SumTree<T>) -> SumTree<T> {
  let (a, b) = match (a.root, b.root) {
    (None, r) | (r, None) => return SumTree { root: r },
    (Some(a), Some(b)) => (a, b),
  };

  match a.height().cmp(&b.height()) {
    Ordering::Equal => join_equal(a, b),
    Ordering::Greater => {
      let mut a = a;
      let extra = Arc::make_mut(&mut a).insert_edge(b, false);
      with_extra(a, extra)
    }
    Ordering::Less => {
      let mut b = b;
      let extra = Arc::make_mut(&mut b).insert_edge(a, true);
      with_extra(b, extra)
    }
  }
}

/// Wraps `root` and its overflow sibling (if any) in a new root.
fn with_extra<T: Item>(root: Arc<Node<T>>, extra: Option<Node<T>>) -> SumTree<T> {
  match extra {
    None => tree_of(root),
    Some(extra) => {
      let height = root.height() + 1;
      tree_of(Arc::new(Node::internal(height, vec![root, Arc::new(extra)])))
    }
  }
}

fn join_equal<T: Item>(mut a: Arc<Node<T>>, b: Arc<Node<T>>) -> SumTree<T> {
  let fits = match (&*a, &*b) {
    (Node::Leaf { items: x, .. }, Node::Leaf { items: y, .. }) => x.len() + y.len() <= MAX,
    (Node::Internal { children: x, .. }, Node::Internal { children: y, .. }) => {
      x.len() + y.len() <= MAX
    }
    _ => unreachable!("join_equal with different heights"),
  };

  if !fits {
    let height = a.height() + 1;
    return tree_of(Arc::new(Node::internal(height, vec![a, b])));
  }

  let b = Arc::unwrap_or_clone(b);
  let node = Arc::make_mut(&mut a);
  match (&mut *node, b) {
    (
      Node::Leaf { items, summaries, .. },
      Node::Leaf { items: items2, summaries: summaries2, .. },
    ) => {
      items.extend(items2);
      summaries.extend(summaries2);
    }
    (Node::Internal { children, .. }, Node::Internal { children: children2, .. }) => {
      children.extend(children2)
    }
    _ => unreachable!(),
  }
  node.update_summary();
  tree_of(a)
}

fn split_node<T, D, Tg>(
  node: &Arc<Node<T>>,
  target: &Tg,
  bias: Bias,
  pos: &mut D,
) -> (SumTree<T>, SumTree<T>)
where
  T: Item,
  D: Dimension<T::Summary>,
  Tg: SeekTarget<D>,
{
  match &**node {
    Node::Leaf { items, summaries, .. } => {
      let mut idx = items.len();
      for (i, s) in summaries.iter().enumerate() {
        let mut end = pos.clone();
        end.add_summary(s);
        if reaches(target, &end, bias) {
          idx = i;
          break;
        }
        *pos = end;
      }
      (leaf_tree(&items[..idx]), leaf_tree(&items[idx..]))
    }
    Node::Internal { children, .. } => {
      let mut found = None;
      for (i, c) in children.iter().enumerate() {
        let mut end = pos.clone();
        end.add_summary(c.summary());
        if reaches(target, &end, bias) {
          found = Some(i);
          break;
        }
        *pos = end;
      }
      let Some(i) = found else { return (tree_of(node.clone()), SumTree::new()) };

      let (child_left, child_right) = split_node(&children[i], target, bias, pos);
      let mut left = SumTree::new();
      for c in &children[..i] {
        left = concat(left, tree_of(c.clone()));
      }
      left = concat(left, child_left);
      let mut right = child_right;
      for c in &children[i + 1..] {
        right = concat(right, tree_of(c.clone()));
      }
      (left, right)
    }
  }
}

impl<T: Item> FromIterator<T> for SumTree<T> {
  /// Builds the tree bottom-up with full nodes.
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    let mut items = iter.into_iter().peekable();
    let mut level: Vec<Arc<Node<T>>> = Vec::new();
    while items.peek().is_some() {
      let chunk: Vec<T> = items.by_ref().take(MAX).collect();
      level.push(Arc::new(Node::leaf(chunk)));
    }

    let mut height = 0;
    while level.len() > 1 {
      height += 1;
      level =
        level.chunks(MAX).map(|chunk| Arc::new(Node::internal(height, chunk.to_vec()))).collect();
    }
    SumTree { root: level.pop() }
  }
}

impl<T: Item> Extend<T> for SumTree<T> {
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    for item in iter {
      self.push(item);
    }
  }
}

impl<'a, T: Item> IntoIterator for &'a SumTree<T> {
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;

  fn into_iter(self) -> Iter<'a, T> { self.iter() }
}

/// A position inside a [`SumTree`], tracking dimension `D` as it moves.
///
/// The cursor points at an item, or past the end (where [`Cursor::item`] is
/// `None` and [`Cursor::start`] is the total).
pub struct Cursor<'a, T: Item, D: Dimension<T::Summary>> {
  tree:  &'a SumTree<T>,
  /// Path from the root to the current leaf; the last entry is the leaf and
  /// the index of the current item. Empty when past the end.
  stack: Vec<(&'a Node<T>, usize)>,
  /// The dimension summed over everything before the current item.
  start: D,
}

impl<'a, T: Item, D: Dimension<T::Summary>> Cursor<'a, T, D> {
  fn seek_first(&mut self) {
    self.stack.clear();
    self.start = D::default();
    if let Some(root) = &self.tree.root {
      self.descend_leftmost(root);
    }
  }

  fn descend_leftmost(&mut self, mut node: &'a Node<T>) {
    loop {
      self.stack.push((node, 0));
      match node {
        Node::Internal { children, .. } => node = &children[0],
        Node::Leaf { .. } => return,
      }
    }
  }

  pub fn item(&self) -> Option<&'a T> {
    let (node, idx) = self.stack.last()?;
    match node {
      Node::Leaf { items, .. } => Some(&items[*idx]),
      Node::Internal { .. } => unreachable!(),
    }
  }

  /// The dimension at the start of the current item.
  pub fn start(&self) -> &D { &self.start }

  /// The dimension at the end of the current item.
  pub fn end(&self) -> D {
    let mut end = self.start.clone();
    if let Some((Node::Leaf { summaries, .. }, idx)) = self.stack.last() {
      end.add_summary(&summaries[*idx]);
    }
    end
  }

  /// Moves to the next item.
  pub fn advance(&mut self) {
    let Some(&mut (leaf, ref mut idx)) = self.stack.last_mut() else { return };
    let Node::Leaf { summaries, .. } = leaf else { unreachable!() };
    self.start.add_summary(&summaries[*idx]);
    *idx += 1;
    if *idx < summaries.len() {
      return;
    }

    self.stack.pop();
    while let Some(&mut (node, ref mut i)) = self.stack.last_mut() {
      *i += 1;
      let Node::Internal { children, .. } = node else { unreachable!() };
      if *i < children.len() {
        let next: &'a Node<T> = &children[*i];
        self.descend_leftmost(next);
        return;
      }
      self.stack.pop();
    }
  }

  /// Seeks to the first item selected by `target` and `bias` (see [`Bias`]),
  /// in O(log n). Seeks from the root, so it may move in either direction. If
  /// no item qualifies, the cursor ends up past the end.
  pub fn seek<Tg: SeekTarget<D>>(&mut self, target: &Tg, bias: Bias) {
    self.stack.clear();
    self.start = D::default();
    let Some(root) = &self.tree.root else { return };

    let mut node: &'a Node<T> = root;
    loop {
      let mut found = None;
      match node {
        Node::Internal { children, .. } => {
          for (i, c) in children.iter().enumerate() {
            let mut end = self.start.clone();
            end.add_summary(c.summary());
            if reaches(target, &end, bias) {
              found = Some(i);
              break;
            }
            self.start = end;
          }
        }
        Node::Leaf { summaries, .. } => {
          for (i, s) in summaries.iter().enumerate() {
            let mut end = self.start.clone();
            end.add_summary(s);
            if reaches(target, &end, bias) {
              found = Some(i);
              break;
            }
            self.start = end;
          }
        }
      }

      let Some(i) = found else {
        // Past the end; `start` is now the total.
        self.stack.clear();
        return;
      };
      self.stack.push((node, i));
      match node {
        Node::Internal { children, .. } => node = &children[i],
        Node::Leaf { .. } => return,
      }
    }
  }
}

pub struct Iter<'a, T: Item> {
  cursor: Cursor<'a, T, T::Summary>,
}

impl<'a, T: Item> Iterator for Iter<'a, T> {
  type Item = &'a T;

  fn next(&mut self) -> Option<&'a T> {
    let item = self.cursor.item()?;
    self.cursor.advance();
    Some(item)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Clone, Default, Debug, PartialEq)]
  struct Sum {
    count: usize,
    total: usize,
  }

  impl Summary for Sum {
    fn add_summary(&mut self, o: &Self) {
      self.count += o.count;
      self.total += o.total;
    }
  }

  #[derive(Clone, Debug, PartialEq)]
  struct Num(usize);

  impl Item for Num {
    type Summary = Sum;

    fn summary(&self) -> Sum { Sum { count: 1, total: self.0 } }
  }

  #[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
  struct Count(usize);
  #[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
  struct Total(usize);

  impl Dimension<Sum> for Count {
    fn add_summary(&mut self, s: &Sum) { self.0 += s.count }
  }
  impl Dimension<Sum> for Total {
    fn add_summary(&mut self, s: &Sum) { self.0 += s.total }
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

  fn tree(v: &[usize]) -> SumTree<Num> { v.iter().map(|&n| Num(n)).collect() }

  fn contents(t: &SumTree<Num>) -> Vec<usize> { t.iter().map(|n| n.0).collect() }

  #[test]
  fn empty() {
    let t = SumTree::<Num>::new();
    assert!(t.is_empty());
    assert_eq!(t.summary(), Sum::default());
    assert!(t.first().is_none() && t.last().is_none());
    assert_eq!(t.iter().count(), 0);
    let (l, r) = t.split_at(&Count(0), Bias::Right);
    assert!(l.is_empty() && r.is_empty());
  }

  #[test]
  fn push_and_iter() {
    let mut t = SumTree::new();
    let mut model = vec![];
    for i in 0..200 {
      t.push(Num(i));
      model.push(i);
      t.check();
    }
    assert_eq!(contents(&t), model);
    assert_eq!(t.summary(), Sum { count: 200, total: (0..200).sum() });
    assert_eq!(t.first(), Some(&Num(0)));
    assert_eq!(t.last(), Some(&Num(199)));
  }

  #[test]
  fn from_iter_matches_push() {
    for n in 0..100 {
      let v: Vec<usize> = (0..n).collect();
      let t = tree(&v);
      t.check();
      assert_eq!(contents(&t), v);
    }
  }

  #[test]
  fn seek() {
    let v: Vec<usize> = (0..50).map(|i| i % 3).collect(); // includes zero-width items
    let t = tree(&v);

    let mut c = t.cursor::<Count>();
    c.seek(&Count(17), Bias::Right);
    assert_eq!(c.item(), Some(&Num(v[17])));
    assert_eq!(c.start(), &Count(17));
    assert_eq!(c.end(), Count(18));

    let mut c = t.cursor::<Total>();
    c.seek(&Total(10), Bias::Left);
    let start = c.start().0;
    let item = c.item().unwrap().0;
    assert!(start < 10 && start + item >= 10);

    c.seek(&Total(10), Bias::Right);
    assert!(c.start().0 <= 10 && c.end().0 > 10);

    c.seek(&Total(10_000), Bias::Right);
    assert!(c.item().is_none());
    assert_eq!(c.start().0, v.iter().sum::<usize>());
  }

  #[test]
  fn cursor_advance_walks_everything() {
    let v: Vec<usize> = (0..137).collect();
    let t = tree(&v);
    let mut c = t.cursor::<Total>();
    let mut sum = 0;
    for &x in &v {
      assert_eq!(c.item(), Some(&Num(x)));
      assert_eq!(c.start().0, sum);
      sum += x;
      c.advance();
    }
    assert!(c.item().is_none());
    assert_eq!(c.start().0, sum);
  }

  #[test]
  fn split_and_append() {
    let v: Vec<usize> = (0..100).collect();
    let t = tree(&v);
    for at in 0..=100 {
      let (l, r) = t.split_at(&Count(at), Bias::Right);
      l.check();
      r.check();
      assert_eq!(contents(&l), v[..at]);
      assert_eq!(contents(&r), v[at..]);
      let mut joined = l;
      joined.append(r);
      joined.check();
      assert_eq!(contents(&joined), v);
    }
  }

  #[test]
  fn persistence() {
    let a = tree(&[1, 2, 3]);
    let mut b = a.clone();
    b.push(Num(4));
    assert_eq!(contents(&a), [1, 2, 3]);
    assert_eq!(contents(&b), [1, 2, 3, 4]);
  }

  #[test]
  fn update_last() {
    let mut t = tree(&(0..30).collect::<Vec<_>>());
    t.update_last(|n| n.0 = 1000);
    t.check();
    assert_eq!(t.last(), Some(&Num(1000)));
    assert_eq!(t.iter().count(), 30);
  }

  #[test]
  fn randomized_against_vec() {
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let mut t = SumTree::<Num>::new();
    let mut model: Vec<usize> = vec![];

    for _ in 0..2000 {
      match rng.next(4) {
        0 => {
          let n = rng.next(5);
          t.push(Num(n));
          model.push(n);
        }
        1 => {
          let extra: Vec<usize> = (0..rng.next(40)).map(|_| rng.next(5)).collect();
          t.append(tree(&extra));
          model.extend(extra);
        }
        2 => {
          // Splice: replace a range with new items.
          let a = rng.next(model.len() + 1);
          let b = a + rng.next(model.len() - a + 1);
          let new: Vec<usize> = (0..rng.next(30)).map(|_| rng.next(5)).collect();
          let (left, rest) = t.split_at(&Count(a), Bias::Right);
          let (_, right) = rest.split_at(&Count(b - a), Bias::Right);
          t = left;
          t.append(tree(&new));
          t.append(right);
          model.splice(a..b, new);
        }
        _ => {
          // Seeks agree with a linear scan.
          let total: usize = model.iter().sum();
          let target = rng.next(total + 2);
          let mut c = t.cursor::<Total>();
          c.seek(&Total(target), Bias::Right);
          let mut sum = 0;
          let expected = model.iter().position(|&x| {
            sum += x;
            sum > target
          });
          match expected {
            Some(i) => {
              assert_eq!(c.item(), Some(&Num(model[i])));
              assert_eq!(c.start().0, model[..i].iter().sum::<usize>());
            }
            None => assert!(c.item().is_none()),
          }
        }
      }
      t.check();
      assert_eq!(contents(&t), model);
    }
  }
}
