use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crop::Rope;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActorId(pub u64);

/// Chunk IDs are ordered by actor then by sequence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkId {
  pub actor: ActorId,
  pub seq:   u64,
}

#[derive(Clone, Debug)]
pub enum Operation {
  Insert(Insert),
  Delete(ChunkId),
}

#[derive(Clone, Debug)]
pub struct Insert {
  pub id:    ChunkId,
  pub after: Anchor,
  pub text:  String,
}

/// A position within a chunk: byte `offset` bytes into `chunk`'s text.
#[derive(Clone, Debug)]
pub struct Anchor {
  pub chunk:  ChunkId,
  pub offset: u32,
}

pub struct Store {
  actor:   ActorId,
  next_id: u64,
  state:   State,
}

#[derive(Debug)]
struct State {
  text:      HashMap<ChunkId, String>,
  tombstone: HashSet<ChunkId>,
  /// splits[chunk][offset] = chunks inserted at byte `offset` within `chunk`.
  splits:    HashMap<ChunkId, BTreeMap<u32, BTreeSet<ChunkId>>>,
  /// Inserts waiting for their anchor chunk to arrive.
  pending:   HashMap<ChunkId, Vec<Insert>>,
}

impl Default for State {
  fn default() -> Self {
    State {
      text:      HashMap::new(),
      tombstone: HashSet::new(),
      splits:    HashMap::new(),
      pending:   HashMap::new(),
    }
  }
}

impl ChunkId {
  pub const ROOT: ChunkId = ChunkId { actor: ActorId(0), seq: u64::MAX };

  pub fn at(self, offset: u32) -> Anchor { Anchor { chunk: self, offset } }
}

impl Anchor {
  pub const ROOT: Anchor = Anchor { chunk: ChunkId::ROOT, offset: 0 };
}

impl Store {
  pub fn new(actor: ActorId) -> Store { Store { actor, next_id: 0, state: State::default() } }

  fn fresh_id(&mut self) -> ChunkId {
    let id = ChunkId { actor: self.actor, seq: self.next_id };
    self.next_id += 1;
    id
  }

  pub fn insert(&mut self, after: Anchor, text: &str) -> ChunkId {
    let id = self.fresh_id();
    self.state.apply(Operation::Insert(Insert { id, after, text: text.to_string() }));
    id
  }

  pub fn delete(&mut self, id: ChunkId) { self.state.apply(Operation::Delete(id)); }

  pub fn apply_remote(&mut self, op: Operation) { self.state.apply(op); }
  pub fn materialize(&self) -> Rope { self.state.materialize() }
}

impl State {
  pub fn apply(&mut self, op: Operation) {
    match op {
      Operation::Insert(insert) => self.apply_insert(insert),
      Operation::Delete(id) => {
        self.tombstone.insert(id);
      }
    }
  }

  fn apply_insert(&mut self, insert: Insert) {
    let anchor_chunk = insert.after.chunk;
    let anchor_offset = insert.after.offset;

    // Defer if the anchor chunk hasn't arrived yet.
    let chunk_known = anchor_chunk == ChunkId::ROOT
      || self.text.contains_key(&anchor_chunk)
      || self.tombstone.contains(&anchor_chunk);

    if !chunk_known {
      self.pending.entry(anchor_chunk).or_default().push(insert);
      return;
    }

    // Register this chunk's text and slot it into the anchor's split map.
    self.text.insert(insert.id, insert.text);
    self
      .splits
      .entry(anchor_chunk)
      .or_default()
      .entry(anchor_offset)
      .or_default()
      .insert(insert.id);

    // Release any inserts that were waiting for this chunk.
    if let Some(pending) = self.pending.remove(&insert.id) {
      for p in pending {
        self.apply_insert(p);
      }
    }
  }

  pub fn materialize(&self) -> Rope {
    let mut out = String::new();
    self.collect_chunk(ChunkId::ROOT, &mut out);
    let mut rope = Rope::new();
    if !out.is_empty() {
      rope.insert(0, &out);
    }
    rope
  }

  /// Recursively emit the content of `id` into `out`, interleaving the
  /// chunk's own text with any chunks inserted at offsets within it.
  fn collect_chunk(&self, id: ChunkId, out: &mut String) {
    let text = self.text.get(&id).map(|s| s.as_str()).unwrap_or("");
    let is_deleted = self.tombstone.contains(&id);
    let text_len = text.len();
    let mut text_pos = 0;

    if let Some(split_map) = self.splits.get(&id) {
      for (&offset, children) in split_map {
        let offset = (offset as usize).min(text_len);
        let offset = (0..=offset).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        // Emit text up to this split point.
        if !is_deleted && text_pos < offset {
          out.push_str(&text[text_pos..offset]);
        }
        text_pos = offset;
        // Recurse into each chunk inserted at this offset.
        for &child in children {
          self.collect_chunk(child, out);
        }
      }
    }

    // Emit any text after the last split point.
    if !is_deleted && text_pos < text_len {
      out.push_str(&text[text_pos..]);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const TEST_ACTOR: ActorId = ActorId(0);

  #[test]
  fn insert_start() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(Anchor::ROOT, "hello");
    store.insert(first.at(0), "foo");

    assert_eq!(store.state.materialize(), "foohello");
  }

  #[test]
  fn insert_middle() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(Anchor::ROOT, "fooo");
    store.insert(first.at(1), "a");

    assert_eq!(store.state.materialize(), "faooo");
  }

  #[test]
  fn insert_end() {
    let mut store = Store::new(TEST_ACTOR);

    let first = store.insert(Anchor::ROOT, "hello");
    let second = store.insert(first.at(5), " world");

    assert_eq!(store.state.materialize(), "hello world");

    store.delete(first);
    assert_eq!(store.state.materialize(), " world");

    store.delete(second);
    assert_eq!(store.state.materialize(), "");
  }
}
