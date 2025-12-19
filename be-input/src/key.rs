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

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct KeyStroke {
  pub key:     Key,
  pub control: bool,
  pub alt:     bool,
}
