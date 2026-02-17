use std::{collections::HashSet, fmt};

pub use toml::de::{DeTable, DeValue};

pub struct ParseResult<T> {
  pub value:       T,
  pub diagnostics: Vec<Diagnostic>,
}

pub struct Diagnostic {
  pub title: String,
  pub line:  u32,
  pub level: DiagnosticLevel,
}

pub enum DiagnosticLevel {
  Error,
  Warning,
}

impl<T> ParseResult<T> {
  pub(crate) fn ok(value: T) -> Self { ParseResult { value, diagnostics: vec![] } }

  pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> ParseResult<U> {
    ParseResult { value: f(self.value), diagnostics: self.diagnostics }
  }
}

pub(crate) struct Parser {
  diagnostics: Vec<Diagnostic>,
}

pub(crate) trait ParseTable {
  /// Returns all keys that are required.
  fn required_keys() -> &'static [&'static str];
  /// Sets the key from a table entry. Returns `true` if the struct was
  /// modified, and `false` if the struct does not respond to the given key.
  fn set_key(&mut self, key: &str, value: DeValue, de: &mut Parser) -> bool;
}

pub(crate) trait ParseValue
where
  Self: Sized,
{
  fn parse(value: DeValue, de: &mut Parser) -> Result<Self, String>;
}

impl<T> ParseValue for T
where
  T: Default + ParseTable,
{
  fn parse(value: DeValue, de: &mut Parser) -> Result<Self, String> {
    match value {
      DeValue::Table(table) => Ok(de.table(table)),
      _ => Err("expected table".to_string()),
    }
  }
}

pub fn parse<T: Default + ParseTable>(content: &str) -> ParseResult<T> {
  let mut parser = Parser { diagnostics: vec![] };

  let value = if let Some(table) = parser.check(DeTable::parse(content)) {
    parser.table(table.into_inner())
  } else {
    T::default()
  };

  ParseResult { value, diagnostics: parser.diagnostics }
}

impl Parser {
  pub fn table<T: Default + ParseTable>(&mut self, table: DeTable) -> T {
    let mut res = T::default();
    let mut required = HashSet::<&str>::from_iter(T::required_keys().iter().copied());

    for (k, v) in table {
      required.remove(&**k.get_ref());

      if !res.set_key(k.get_ref(), v.into_inner(), self) {
        self.warn(format!("unknown key: {}", k.get_ref()), k.span());
      }
    }

    for key in required {
      self.error(format!("missing key: '{}'", key), 0..0); // todo: bah this library is bad
    }

    res
  }

  pub fn value<T: Default + ParseValue>(&mut self, value: DeValue) -> T {
    let res = T::parse(value, self);
    self.check(res).unwrap_or_default()
  }

  fn check<U, E: std::fmt::Display>(&mut self, result: Result<U, E>) -> Option<U> {
    match result {
      Ok(value) => Some(value),
      Err(err) => {
        self.diagnostics.push(Diagnostic {
          title: err.to_string(),
          line:  0,
          level: DiagnosticLevel::Error,
        });
        None
      }
    }
  }

  fn error(&mut self, title: String, span: std::ops::Range<usize>) {
    self.diagnostics.push(Diagnostic {
      title,
      line: span.start as u32,
      level: DiagnosticLevel::Error,
    })
  }

  fn warn(&mut self, title: String, span: std::ops::Range<usize>) {
    self.diagnostics.push(Diagnostic {
      title,
      line: span.start as u32,
      level: DiagnosticLevel::Warning,
    })
  }
}

impl fmt::Display for Diagnostic {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "{}: {} at line {}",
      match self.level {
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Warning => "warning",
      },
      self.title,
      self.line
    )
  }
}

impl ParseValue for i32 {
  fn parse(value: DeValue, _de: &mut Parser) -> Result<Self, String> {
    match value {
      DeValue::Integer(i) => {
        i32::from_str_radix(i.as_str(), i.radix()).map_err(|_| "expected integer".to_string())
      }
      _ => Err("expected integer".to_string()),
    }
  }
}

impl ParseValue for String {
  fn parse(value: DeValue, _de: &mut Parser) -> Result<Self, String> {
    match value {
      DeValue::String(s) => Ok(s.into()),
      _ => Err("expected integer".to_string()),
    }
  }
}

#[cfg(test)]
mod tests {
  use be_config_macros::Config;

  use super::*;

  #[derive(Default, Config)]
  struct Foo {
    a:      i32,
    b:      String,
    nested: Bar,
  }

  #[derive(Default, Config)]
  struct Bar {
    c: i32,
  }

  #[test]
  fn foo() {
    let res = parse::<Foo>("a = 1\nb = \"foo\"\n[nested]\nc = 3");
    assert_eq!(res.value.a, 1);
    assert_eq!(res.value.b, "foo");
    assert_eq!(res.value.nested.c, 3);
  }
}
