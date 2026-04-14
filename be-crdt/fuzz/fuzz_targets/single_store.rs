#![no_main]

use arbitrary::Arbitrary;
use be_crdt::{ActorId, Anchor, ChunkId, Store};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
enum Op {
  Insert { after_idx: u8, after_offset: u8, text: String },
  Delete { target_idx: u8 },
}

fuzz_target!(|ops: Vec<Op>| {
  let mut store = Store::new(ActorId(1));
  // ids[0] is ROOT; subsequent entries are chunk IDs from inserts.
  let mut ids: Vec<ChunkId> = vec![ChunkId::ROOT];

  for op in ops {
    match op {
      Op::Insert { after_idx, after_offset, text } => {
        let chunk = ids[after_idx as usize % ids.len()];
        let anchor = Anchor { chunk, offset: after_offset as u32 };
        ids.push(store.insert(anchor, &text));
      }
      Op::Delete { target_idx } => {
        let id = ids[target_idx as usize % ids.len()];
        store.delete(id);
      }
    }
  }

  let _ = store.materialize();
});
