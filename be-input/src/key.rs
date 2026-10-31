#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Key {
  Char(char),
  Backspace,
  Delete,
  Escape,
  Tab,
  Menu,

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

impl KeyStroke {
  /// The stroke as it is written in the config, like `C-a`, `SP`, or `TAB`.
  pub fn label(&self) -> String {
    let mut out = String::new();
    if self.control {
      out.push_str("C-");
    }
    if self.alt {
      out.push_str("A-");
    }
    if self.sup {
      out.push_str("M-");
    }
    // Shift is ignored for characters, as the character already carries it.
    if self.shift && !matches!(self.key, Key::Char(_)) {
      out.push_str("S-");
    }

    match self.key {
      Key::Char(' ') => out.push_str("SP"),
      Key::Char('\n') => out.push_str("RET"),
      Key::Char(c) => out.push(c),
      Key::Backspace => out.push_str("BS"),
      Key::Delete => out.push_str("DEL"),
      Key::Escape => out.push_str("ESC"),
      Key::Tab => out.push_str("TAB"),
      Key::Menu => out.push_str("MENU"),
      Key::ArrowUp => out.push_str("UP"),
      Key::ArrowDown => out.push_str("DOWN"),
      Key::ArrowLeft => out.push_str("LEFT"),
      Key::ArrowRight => out.push_str("RIGHT"),
    }
    out
  }
}
