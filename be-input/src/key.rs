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

#[derive(Copy, Clone, Debug, Hash, Eq)]
pub struct KeyStroke {
  pub key:     Key,
  pub sup:     bool,
  pub shift:   bool,
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

impl PartialEq for KeyStroke {
  fn eq(&self, other: &Self) -> bool {
    match (self.key, other.key) {
      // Ignore shift when comparing character keys
      (Key::Char(c1), Key::Char(c2)) => {
        c1 == c2 && self.sup == other.sup && self.control == other.control && self.alt == other.alt
      }
      _ => {
        self.key == other.key
          && self.sup == other.sup
          && self.shift == other.shift
          && self.control == other.control
          && self.alt == other.alt
      }
    }
  }
}
