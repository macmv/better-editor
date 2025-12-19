#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Key {
  Char(char),
  Backspace,
  Delete,
  Escape,

  ArrowUp,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
}
