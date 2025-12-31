use std::collections::HashMap;

use be_editor::{DiagnosticLevel, HighlightKey};

use crate::{Color, oklch};

pub struct Theme {
  pub text:              Color,
  pub background_raised: Color,
  pub background:        Color,
  pub background_lower:  Color,

  pub syntax: SyntaxTheme,
}

pub struct SyntaxTheme {
  entries: HashMap<String, Highlight>,
}

pub struct Highlight {
  pub foreground:    Option<Color>,
  pub background:    Option<Color>,
  pub weight:        Option<FontWeight>,
  pub style:         Option<FontStyle>,
  pub underline:     Option<Underline>,
  pub strikethrough: Option<Strikethrough>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FontWeight {
  Normal,
  Bold,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FontStyle {
  Normal,
  Italic,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Underline {
  Foreground,
  Color(Color),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Strikethrough {
  Foreground,
  Color(Color),
}

impl From<Color> for Highlight {
  fn from(color: Color) -> Self { Highlight::empty().with_foreground(color) }
}

impl<const N: usize> From<[(&str, Highlight); N]> for SyntaxTheme {
  fn from(entries: [(&str, Highlight); N]) -> Self {
    SyntaxTheme { entries: entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
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
        ("constant", Highlight::from(oklch(0.8, 0.13, 50.0))),
        ("function", oklch(0.8, 0.12, 260.0).into()),
        ("keyword", oklch(0.8, 0.14, 295.0).into()),
        ("operator", oklch(0.6, 0.20, 300.0).into()),
        ("property", oklch(0.8, 0.12, 340.0).into()),
        ("punctuation", oklch(0.5, 0.0, 0.0).into()),
        ("string", oklch(0.8, 0.14, 131.0).into()),
        ("type", oklch(0.8, 0.12, 170.0).into()),
        ("variable.builtin", oklch(0.8, 0.13, 50.0).into()),
        ("variable.parameter", oklch(0.8, 0.14, 20.0).into()),
        ("error", Highlight::from(oklch(0.8, 0.12, 30.0))),
        ("warning", Highlight::from(oklch(0.8, 0.12, 120.0))),
      ]),
    }
  }
}

impl Highlight {
  pub const fn empty() -> Highlight {
    Highlight {
      foreground:    None,
      background:    None,
      weight:        None,
      style:         None,
      underline:     None,
      strikethrough: None,
    }
  }

  pub const fn with_foreground(mut self, color: Color) -> Highlight {
    self.foreground = Some(color);
    self
  }

  pub const fn with_background(mut self, color: Color) -> Highlight {
    self.background = Some(color);
    self
  }

  pub const fn with_weight(mut self, weight: FontWeight) -> Highlight {
    self.weight = Some(weight);
    self
  }

  pub const fn with_style(mut self, style: FontStyle) -> Highlight {
    self.style = Some(style);
    self
  }

  pub const fn with_underline(mut self, underline: Underline) -> Highlight {
    self.underline = Some(underline);
    self
  }

  pub const fn with_strikethrough(mut self, strikethrough: Strikethrough) -> Highlight {
    self.strikethrough = Some(strikethrough);
    self
  }

  pub const fn is_empty(&self) -> bool {
    self.foreground.is_none()
      && self.background.is_none()
      && self.weight.is_none()
      && self.style.is_none()
      && self.underline.is_none()
      && self.strikethrough.is_none()
  }

  pub const fn merge_from(&mut self, other: &Highlight) {
    macro_rules! merge {
      ($field:ident) => {
        if self.$field.is_none()
          && let Some(v) = other.$field
        {
          self.$field = Some(v);
        }
      };
    }

    merge!(foreground);
    merge!(background);
    merge!(weight);
    merge!(style);
    merge!(underline);
    merge!(strikethrough);
  }
}

impl FontWeight {
  pub const fn to_parley(&self) -> parley::FontWeight {
    match self {
      FontWeight::Normal => parley::FontWeight::NORMAL,
      FontWeight::Bold => parley::FontWeight::BOLD,
    }
  }
}

impl SyntaxTheme {
  pub fn lookup(&self, keys: &[HighlightKey]) -> Option<Highlight> {
    let mut highlight = Highlight::empty();

    for key in keys {
      match key {
        HighlightKey::TreeSitter(key) => {
          let mut cur = *key;

          loop {
            if let Some(v) = self.entries.get(cur) {
              highlight.merge_from(v);
              break;
            }

            match cur.rfind('.') {
              Some(idx) => cur = &cur[..idx],
              None => break,
            }
          }
        }

        HighlightKey::SemanticToken(_) => {}

        HighlightKey::Diagnostic(level) => {
          let key = match level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
          };

          if let Some(v) = self.entries.get(key) {
            highlight.merge_from(v);
          }
        }
      }
    }

    if !highlight.is_empty() { Some(highlight) } else { None }
  }
}
