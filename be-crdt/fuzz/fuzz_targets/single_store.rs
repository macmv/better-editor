#![no_main]

use arbitrary::Arbitrary;
use be_crdt::{ActorId, ChunkId, Store};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
enum Op {
  Insert { after_idx: u8, text: String },
  Split { target_idx: u8, at: u8 },
  Delete { target_idx: u8 },
}

fuzz_target!(|ops: Vec<Op>| {
  let mut store = Store::new(ActorId(1));
  // ids[0] is ROOT, subsequent entries are chunk IDs returned by insert/split.
  let mut ids: Vec<ChunkId> = vec![ChunkId::ROOT];

  for op in ops {
    match op {
      Op::Insert { after_idx, text } => {
        let after = ids[after_idx as usize % ids.len()];
        ids.push(store.insert(after, &text));
      }
      Op::Split { target_idx, at } => {
        let target = ids[target_idx as usize % ids.len()];
        // Only split chunks that are still alive (have text). Skip otherwise
        // so we don't hit the panic -- the fuzz goal is to find *unexpected* panics.
        if target == ChunkId::ROOT {
          return;
        }
        let (l, r) = store.split(target, at as u32);
        ids.extend([l, r]);
      }
      Op::Delete { target_idx } => {
        let id = ids[target_idx as usize % ids.len()];
        store.delete(id);
      }
    }
  }

  let _ = store.materialize();
});
