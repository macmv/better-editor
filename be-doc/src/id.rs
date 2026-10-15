use std::fmt;

/// Identifies a document across a client and server. Random, so either side can
/// mint one without coordination (eg, for a new buffer created offline).
#[derive(
  Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct DocId(pub u128);

impl DocId {
  pub fn random() -> DocId {
    let mut buf = [0; 16];
    getrandom::fill(&mut buf).expect("the OS should be able to provide random bytes");
    DocId(u128::from_le_bytes(buf))
  }
}

impl fmt::Debug for DocId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DocId({:032x})", self.0) }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ids_are_unique() {
    assert_ne!(DocId::random(), DocId::random());
  }
}
