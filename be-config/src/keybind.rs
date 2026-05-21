use std::{collections::HashMap, sync::Arc};

use be_config_macros::Config;
use be_input::KeyStroke;
use smallvec::SmallVec;

use crate::{grammar, parse::ParseValue};

#[derive(Default, Config, Clone)]
pub struct Keybinds {
  pub(crate) settings: KeybindSettings,
  pub(crate) normal:   KeyMap<grammar::Action>,
  pub(crate) visual:   VisualKeybinds,
  pub(crate) movement: KeyMap<grammar::Move>,
}

#[derive(Default, Config, Clone)]
pub struct VisualKeybinds {
  pub(crate) all:       KeyMap<grammar::Action>,
  pub(crate) block:     KeyMap<grammar::Action>,
  pub(crate) line:      KeyMap<grammar::Action>,
  pub(crate) character: KeyMap<grammar::Action>,
}

#[derive(Default, Config, Clone)]
pub struct KeybindSettings {
  navigate: Key,
}

pub struct KeyMap<T> {
  pub(crate) keys: Arc<HashMap<Key, T>>,
}

impl<T> Default for KeyMap<T> {
  fn default() -> Self { Self { keys: Arc::new(HashMap::new()) } }
}

impl<T> Clone for KeyMap<T> {
  fn clone(&self) -> Self { Self { keys: self.keys.clone() } }
}

#[derive(Default, Clone, PartialEq, Eq, Hash)]
pub struct Key(pub SmallVec<[KeyStroke; 4]>);

impl Key {
  fn parse_str(s: &str) -> Result<Self, String> {
    let mut keys = SmallVec::new();

    for key in s.split(' ') {
      keys.push(parse_key_stroke(key)?);
    }

    Ok(Key(keys))
  }
}

// Parses keys like:
//
// 'a' 'M-a' 'C-a' 'C-A-a'
fn parse_key_stroke(key: &str) -> Result<KeyStroke, String> {
  use be_input::Key;

  let mut out = KeyStroke {
    key:     Key::Char(' '),
    sup:     false,
    shift:   false,
    control: false,
    alt:     false,
  };

  let mut iter = key.chars().peekable();
  let first = iter.next().ok_or_else(|| format!("invalid key: {}", key))?;

  if iter.peek().is_none() {
    out.key = Key::Char(first);
    return Ok(out);
  }

  match key {
    "SP" => out.key = Key::Char(' '),
    "ESC" => out.key = Key::Escape,

    _ if iter.peek() == Some(&'-') => {
      let mut c = first;
      loop {
        assert_eq!(iter.next(), Some('-'));

        match c {
          'M' => out.sup = true,
          'C' => out.control = true,
          'A' => out.alt = true,
          _ => return Err(format!("invalid key: {}", key)),
        }

        c = iter.next().ok_or_else(|| format!("invalid key: {}", key))?;

        if iter.peek().is_none() {
          out.key = Key::Char(c);
          break;
        } else if iter.peek() != Some(&'-') {
          return Err(format!("invalid key: {}", key));
        }
      }
    }

    _ => return Err(format!("invalid key: {}", key)),
  }

  Ok(out)
}

impl ParseValue for Key {
  fn parse(
    &mut self,
    value: toml::Spanned<toml::de::DeValue>,
    _: &mut crate::parse::Parser,
  ) -> Result<(), String> {
    match value.as_ref() {
      toml::de::DeValue::String(s) => {
        *self = Key::parse_str(&s)?;
        Ok(())
      }
      _ => return Err("expected string".to_string()),
    }
  }
}

struct KeybindParser<'a> {
  value: &'a toml::de::DeValue<'a>,
}

// Deserialize but worse!!
//
// Believe it or not, this is intentional. I want to keep the config mapping
// separate from `be-input`, so that config changes are easier to track.
trait ParseKey
where
  Self: Sized,
{
  fn parse(de: &KeybindParser) -> Result<Self, String>;
}

impl KeybindParser<'_> {
  fn string(&self) -> Option<&str> {
    match self.value {
      toml::de::DeValue::String(s) => Some(&s),
      _ => None,
    }
  }
  fn string_or(&self, key: &str) -> Option<&str> {
    match self.value {
      toml::de::DeValue::String(s) => Some(&s),
      toml::de::DeValue::Table(table) => match table.get(key)?.as_ref() {
        toml::de::DeValue::String(s) => Some(&s),
        _ => None,
      },
      _ => None,
    }
  }

  fn get<T: ParseKey>(&self, key: &str) -> Result<T, String> {
    let value = self
      .value
      .as_table()
      .ok_or("expected table")?
      .get(key)
      .ok_or_else(|| format!("missing key '{key}'"))?
      .as_ref();

    T::parse(&KeybindParser { value })
  }

  fn get_or<T: ParseKey>(&self, key: &str, default: T) -> Result<T, String> {
    let Some(table) = self.value.as_table() else { return Ok(default) };
    let Some(value) = table.get(key) else { return Ok(default) };

    T::parse(&KeybindParser { value: value.as_ref() })
  }
}

impl ParseKey for bool {
  fn parse(de: &KeybindParser) -> Result<Self, String> {
    match de.value {
      toml::de::DeValue::Boolean(b) => Ok(*b),
      _ => return Err("expected boolean".to_string()),
    }
  }
}

impl ParseKey for i32 {
  fn parse(de: &KeybindParser) -> Result<Self, String> {
    match de.value {
      toml::de::DeValue::Integer(i) => {
        Ok(i.as_str().parse().map_err(|_| "expected integer".to_string())?)
      }
      _ => return Err("expected integer".to_string()),
    }
  }
}

// Simple types

impl ParseKey for be_input::Mode {
  fn parse(p: &KeybindParser) -> Result<Self, String> {
    use be_input::{Mode::*, VisualMode};

    match p.string() {
      Some("normal") => Ok(Normal),
      Some("insert") => Ok(Insert),
      Some("visual_character") => Ok(Visual(VisualMode::Character)),
      Some("visual_line") => Ok(Visual(VisualMode::Line)),
      Some("visual_block") => Ok(Visual(VisualMode::Block)),
      Some("replace") => Ok(Replace),
      Some("command") => Ok(Command),
      Some(v) => return Err(format!("unknown mode '{v}'")),
      _ => return Err(format!("expected string")),
    }
  }
}

impl ParseKey for be_input::ChangeDirection {
  fn parse(p: &KeybindParser) -> Result<Self, String> {
    use be_input::ChangeDirection;

    match p.string() {
      Some("next") => Ok(ChangeDirection::Next),
      Some("prev") => Ok(ChangeDirection::Prev),
      Some(v) => return Err(format!("unknown direction '{v}'")),
      _ => return Err(format!("expected string")),
    }
  }
}

// Grammar types

impl ParseKey for grammar::Action {
  fn parse(p: &KeybindParser) -> Result<Self, String> {
    use grammar::Action::*;

    match p.string_or("action") {
      Some("set_mode") => Ok(SetMode { mode: p.get("mode")?, delta: p.get_or("delta", 0)? }),
      Some("open_search") => Ok(OpenSearch),
      Some("append") => Ok(Append { after: p.get("after")? }),
      Some("yank") => Ok(Yank),
      _ => Ok(Edit { edit: grammar::Edit::parse(p)? }),
    }
  }
}

impl ParseKey for grammar::Edit {
  fn parse(p: &KeybindParser) -> Result<Self, String> {
    use grammar::Edit::*;

    match p.string_or("edit") {
      Some("replace") => Ok(Replace),
      Some("delete") => Ok(Delete),
      Some("cut") => Ok(Cut),
      Some("delete_line") => Ok(DeleteLine),
      Some("cut_line") => Ok(DeleteLine),
      Some("delete_rest_of_line") => Ok(DeleteRestOfLine),
      Some("paste") => Ok(Paste { after: p.get("after")? }),
      Some("backspace") => Ok(Backspace),
      Some("undo") => Ok(Undo),
      Some("redo") => Ok(Redo),
      Some("switch_case") => Ok(SwitchCase),
      Some(v) => return Err(format!("unknown edit '{v}'")),
      _ => return Err(format!("expected string or an object with an 'edit' key")),
    }
  }
}

impl ParseKey for grammar::Move {
  fn parse(p: &KeybindParser) -> Result<Self, String> {
    use be_input::Direction;
    use grammar::Move::*;

    match p.string_or("move") {
      Some("up") => Ok(Single(Direction::Up)),
      Some("down") => Ok(Single(Direction::Down)),
      Some("left") => Ok(Single(Direction::Left)),
      Some("right") => Ok(Single(Direction::Right)),
      Some("next_word") => Ok(NextWord),
      Some("end_word") => Ok(EndWord),
      Some("prev_word") => Ok(PrevWord),
      Some("line_start") => Ok(LineStart),
      Some("line_start_of_text") => Ok(LineStartOfText),
      Some("line_end") => Ok(LineEnd),
      Some("matching_bracket") => Ok(MatchingBracket),
      Some("goto_line") => Ok(FileStart),
      Some("goto_end") => Ok(FileEnd),
      Some("find") => Ok(Char(p.get("direction")?)),
      Some("search_result") => Ok(Result(p.get("direction")?)),
      Some("change") => Ok(Change(p.get("direction")?)),
      Some("diagnostic") => Ok(Diagnostic(p.get("direction")?)),
      Some("goto_definition") => Ok(GotoDefinition),
      Some("goto_reference") => Ok(GotoReference),
      Some("back_definition") => Ok(BackDefinition),
      Some(v) => return Err(format!("unknown movement '{v}'")),
      _ => return Err(format!("expected string or an object with a 'move' key")),
    }
  }
}

impl<T: ParseKey> ParseValue for KeyMap<T> {
  fn parse(
    &mut self,
    value: toml::Spanned<toml::de::DeValue>,
    de: &mut crate::parse::Parser,
  ) -> Result<(), String> {
    match value.as_ref() {
      toml::de::DeValue::Table(table) => {
        let mut keys = HashMap::new();

        for (k, v) in table {
          let k = match Key::parse_str(k.get_ref()) {
            Ok(k) => k,
            Err(e) => {
              de.error(e, k.span());
              continue;
            }
          };
          let v = match T::parse(&KeybindParser { value: v.as_ref() }) {
            Ok(v) => v,
            Err(e) => {
              de.error(e, v.span());
              continue;
            }
          };

          keys.insert(k, v);
        }

        self.keys = keys.into();

        Ok(())
      }

      _ => return Err("expected table".to_string()),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_strokes() {
    use be_input::Key;

    assert_eq!(
      parse_key_stroke("a"),
      Ok(KeyStroke {
        key:     Key::Char('a'),
        sup:     false,
        shift:   false,
        control: false,
        alt:     false,
      })
    );
    assert_eq!(
      parse_key_stroke("C-a"),
      Ok(KeyStroke {
        key:     Key::Char('a'),
        sup:     false,
        shift:   false,
        control: true,
        alt:     false,
      })
    );
    assert_eq!(
      parse_key_stroke("C-A-a"),
      Ok(KeyStroke {
        key:     Key::Char('a'),
        sup:     false,
        shift:   false,
        control: true,
        alt:     true,
      })
    );

    assert!(parse_key_stroke("-a").is_err());
    assert!(parse_key_stroke("Ca").is_err());
    assert!(parse_key_stroke("C-").is_err());

    assert!(parse_key_stroke("C-Aa").is_err());
    assert!(parse_key_stroke("CA-a").is_err());
  }
}
