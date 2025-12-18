use std::collections::{BTreeSet, HashMap, HashSet};

use ropey::Rope;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorId(u64);

/// Chunk IDs are ordered by actor then by sequence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ChunkId {
  actor: ActorId,
  seq:   u64,
}

#[derive(Debug)]
enum Operation {
  Insert(Insert),
  Split(Split),
  Delete(ChunkId),
}

#[derive(Debug)]
struct Insert {
  id:    ChunkId,
  after: ChunkId,
  text:  String,
}

#[derive(Debug)]
struct Split {
  target: ChunkId,
  at:     u32,
  left:   ChunkId,
  right:  ChunkId,
}

pub struct Store {
  actor:   ActorId,
  next_id: u64,
  state:   State,
}

#[derive(Debug)]
struct State {
  children:  HashMap<ChunkId, BTreeSet<ChunkId>>,
  text:      HashMap<ChunkId, String>,
  tombstone: HashSet<ChunkId>,
  pending:   HashMap<ChunkId, Vec<Insert>>,
  alias:     HashMap<ChunkId, ChunkId>,
}

impl Default for State {
  fn default() -> Self {
    State {
      children:  HashMap::from([(ChunkId::ROOT, BTreeSet::new())]),
      text:      HashMap::new(),
      tombstone: HashSet::new(),
      pending:   HashMap::new(),
      alias:     HashMap::new(),
    }
  }
}

impl ChunkId {
  const ROOT: ChunkId = ChunkId { actor: ActorId(0), seq: u64::MAX };
}

impl Store {
  pub fn new(actor: ActorId) -> Store { Store { actor, next_id: 0, state: State::default() } }

  fn fresh_id(&mut self) -> ChunkId {
    let id = ChunkId { actor: self.actor, seq: self.next_id };
    self.next_id += 1;
    id
  }

  fn insert(&mut self, after: ChunkId, text: &str) -> ChunkId {
    let id = self.fresh_id();
    self.state.apply(Operation::Insert(Insert { id, after, text: text.to_string() }));
    id
  }

  fn split(&mut self, target: ChunkId, at: u32) -> (ChunkId, ChunkId) {
    let l = self.fresh_id();
    let r = self.fresh_id();
    self.state.apply(Operation::Split(Split { target, at, left: l, right: r }));

    (l, r)
  }

  fn delete(&mut self, id: ChunkId) { self.state.apply(Operation::Delete(id)); }
}

impl State {
  pub fn apply(&mut self, op: Operation) {
    match op {
      Operation::Insert(insert) => self.apply_insert(insert),
      Operation::Split(split) => self.apply_split(split),
      Operation::Delete(id) => {
        self.tombstone.insert(id);
      }
    }
  }

  fn apply_insert(&mut self, insert: Insert) {
    let after = self.resolve_after(insert.after);

    if !(after == ChunkId::ROOT
      || self.text.contains_key(&after)
      || self.tombstone.contains(&after))
    {
      self.text.insert(insert.id, insert.text.clone());
      self.pending.entry(after).or_default().push(insert);
      return;
    } else {
      self.text.insert(insert.id, insert.text);
    }

    self.children.entry(after).or_default().insert(insert.id);
    self.children.entry(insert.id).or_default();

    if let Some(pending) = self.pending.remove(&after) {
      for insert in pending {
        self.apply_insert(insert);
      }
    }
  }

  fn apply_split(&mut self, split: Split) {
    let Some(orig) = self.text.remove(&split.target) else { panic!("no text at split target") };

    let left = orig[..split.at as usize].to_string();
    let right = orig[split.at as usize..].to_string();

    self.tombstone.insert(split.target);
    self.text.remove(&split.target);

    self.alias.insert(split.target, split.right);

    self.text.insert(split.left, left);
    self.text.insert(split.right, right);

    self.children.entry(split.left).or_default().insert(split.right);
    let old_children = self.children.remove(&split.target).unwrap();
    let right_children = self.children.entry(split.right).or_default();

    for c in old_children {
      if c != split.left && c != split.right {
        right_children.insert(c);
      }
    }

    self.children.entry(split.target).or_default().insert(split.left);

    if let Some(pending) = self.pending.remove(&split.target) {
      for insert in pending {
        self.apply_insert(insert);
      }
    }
  }

  fn resolve_after(&self, mut id: ChunkId) -> ChunkId {
    while let Some(after) = self.alias.get(&id) {
      id = *after;
    }
    id
  }

  pub fn materialize(&self) -> Rope {
    let mut rope = Rope::new();
    let mut stack = vec![ChunkId::ROOT];
    let mut i = 0;

    while let Some(id) = stack.pop() {
      if !self.tombstone.contains(&id)
        && let Some(text) = self.text.get(&id)
      {
        rope.insert(i, text);
        i += text.len();
      }

      if let Some(children) = self.children.get(&id) {
        for child in children {
          stack.push(*child);
        }
      }
    }

    rope
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const TEST_ACTOR: ActorId = ActorId(0);

  #[test]
  fn insert_works() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(ChunkId::ROOT, "hello");
    let second = store.insert(first, " world");

    assert_eq!(store.state.materialize().to_string(), "hello world");

    store.delete(first);
    assert_eq!(store.state.materialize().to_string(), " world");

    store.delete(second);
    assert_eq!(store.state.materialize().to_string(), "");
  }

  #[test]
  fn split_works() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(ChunkId::ROOT, "hello");
    let (l, _) = store.split(first, 2);
    store.insert(l, " ");

    assert_eq!(store.state.materialize().to_string(), "he llo");
  }

  #[test]
  fn split_children() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(ChunkId::ROOT, "hello");
    store.insert(first, " world");
    let (l, _) = store.split(first, 2);
    store.insert(l, " ");

    assert_eq!(store.state.materialize().to_string(), "he llo world");
  }
}
