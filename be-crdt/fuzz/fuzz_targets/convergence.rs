#![no_main]

use arbitrary::Arbitrary;
use be_crdt::{ActorId, Anchor, ChunkId, Insert, Operation, Store};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
enum AbstractOp {
  Insert { after_idx: u8, after_offset: u8, text: String },
  Delete { target_idx: u8 },
}

// Build a concrete operation log for one actor. Chunks are tracked with their
// text length so we can generate valid offsets without going out of bounds.
fn build_ops(actor: ActorId, abstract_ops: &[AbstractOp]) -> Vec<Operation> {
  let mut seq: u64 = 0;
  // (chunk_id, text_len)
  let mut chunks: Vec<(ChunkId, u32)> = vec![(ChunkId::ROOT, 0)];
  let mut ops = vec![];

  for op in abstract_ops {
    match op {
      AbstractOp::Insert { after_idx, after_offset, text } => {
        let (chunk, text_len) = chunks[*after_idx as usize % chunks.len()];
        let offset = (*after_offset as u32).min(text_len);
        let id = ChunkId { actor, seq };
        seq += 1;
        chunks.push((id, text.len() as u32));
        ops.push(Operation::Insert(Insert {
          id,
          after: Anchor { chunk, offset },
          text: text.clone(),
        }));
      }
      AbstractOp::Delete { target_idx } => {
        let (id, _) = chunks[*target_idx as usize % chunks.len()];
        if id != ChunkId::ROOT {
          ops.push(Operation::Delete(id));
        }
      }
    }
  }

  ops
}

fuzz_target!(|input: (Vec<AbstractOp>, Vec<AbstractOp>)| {
  let (abstract_a, abstract_b) = input;

  let ops_a = build_ops(ActorId(1), &abstract_a);
  let ops_b = build_ops(ActorId(2), &abstract_b);

  // Store A: sees actor 1's ops first, then actor 2's.
  let mut store_a = Store::new(ActorId(1));
  for op in ops_a.iter().chain(ops_b.iter()) {
    store_a.apply_remote(op.clone());
  }

  // Store B: sees actor 2's ops first, then actor 1's.
  let mut store_b = Store::new(ActorId(2));
  for op in ops_b.iter().chain(ops_a.iter()) {
    store_b.apply_remote(op.clone());
  }

  assert_eq!(store_a.materialize(), store_b.materialize());
});
