#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Key {
  Char(char),
  Backspace,
  Delete,
  Escape,
  Tab,

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

impl PartialEq<char> for Key {
  fn eq(&self, other: &char) -> bool {
    match self {
      Key::Char(c) => c == other,
      _ => false,
    }
  }
}
