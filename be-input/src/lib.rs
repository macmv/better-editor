mod action;
mod key;

#[derive(Default, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Mode {
  #[default]
  Normal,
  Insert,
  Visual,
  Replace,
  Command,
}

pub use action::*;
pub use key::*;
