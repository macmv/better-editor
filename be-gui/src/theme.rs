use std::collections::HashMap;

use be_editor::HighlightKey;

use crate::{Color, oklch};

pub struct Theme {
  pub text:              Color,
  pub background_raised: Color,
  pub background:        Color,
  pub background_lower:  Color,

  pub syntax: SyntaxTheme,
}

pub struct SyntaxTheme {
  entries: HashMap<String, Color>,
}

impl<const N: usize> From<[(&str, Color); N]> for SyntaxTheme {
  fn from(entries: [(&str, Color); N]) -> Self {
    SyntaxTheme { entries: entries.iter().map(|(k, v)| (k.to_string(), *v)).collect() }
  }
}

impl Theme {
  pub fn default_theme() -> Theme {
    Theme {
      text:              oklch(1.0, 0.0, 0.0),
      background_raised: oklch(0.28, 0.03, 288.0),
      background:        oklch(0.23, 0.03, 288.0),
      background_lower:  oklch(0.20, 0.03, 288.0),

      syntax: SyntaxTheme::from([
        ("punctuation", oklch(0.5, 0.0, 0.0)),
        ("keyword", oklch(0.8, 0.14, 288.0)),
        ("string", oklch(0.8, 0.14, 131.0)),
        ("property", oklch(0.8, 0.12, 342.0)),
      ]),
    }
  }
}

impl SyntaxTheme {
  pub fn lookup(&self, keys: &[HighlightKey]) -> Option<Color> {
    for key in keys {
      if let HighlightKey::TreeSitter(key) = key {
        if let Some(color) = self.entries.get(*key) {
          return Some(*color);
        }
      }
    }

    None
  }
}
