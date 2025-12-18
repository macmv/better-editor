mod action;
mod key;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Mode {
  Normal,
  Insert,
  Visual,
  Replace,
  Command,
}

pub use action::*;
pub use key::*;
